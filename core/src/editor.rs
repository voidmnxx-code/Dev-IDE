//! Text buffer with cursor, selection and undo/redo.
//! All positions are in `char` units (Unicode scalar values), not bytes.

use ropey::Rope;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    Left,
    Right,
    Up,
    Down,
    WordLeft,
    WordRight,
    LineStart,
    LineEnd,
    DocStart,
    DocEnd,
}

#[derive(Clone)]
struct Snapshot {
    text: Rope,
    cursor: usize,
    anchor: Option<usize>,
}

pub struct Buffer {
    text: Rope,
    cursor: usize,
    anchor: Option<usize>,
    /// Remembered column for vertical movement.
    goal_col: Option<usize>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    dirty: bool,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn closing_pair(c: char) -> Option<char> {
    match c {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '"' => Some('"'),
        '\'' => Some('\''),
        '`' => Some('`'),
        _ => None,
    }
}

impl Buffer {
    pub fn new(text: &str) -> Self {
        Buffer {
            text: Rope::from_str(text),
            cursor: 0,
            anchor: None,
            goal_col: None,
            undo: Vec::new(),
            redo: Vec::new(),
            dirty: false,
        }
    }

    // ---- queries -------------------------------------------------------

    pub fn text(&self) -> String {
        self.text.to_string()
    }

    pub fn len_chars(&self) -> usize {
        self.text.len_chars()
    }

    pub fn line_count(&self) -> usize {
        // ropey counts a trailing empty line after a final '\n'; that matches editors.
        self.text.len_lines()
    }

    pub fn line(&self, idx: usize) -> String {
        if idx >= self.text.len_lines() {
            return String::new();
        }
        let s = self.text.line(idx).to_string();
        s.trim_end_matches(&['\n', '\r'][..]).to_string()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// (line, column), both zero-based.
    pub fn cursor_position(&self) -> (usize, usize) {
        let line = self.text.char_to_line(self.cursor);
        (line, self.cursor - self.text.line_to_char(line))
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn mark_saved(&mut self) {
        self.dirty = false;
    }

    pub fn selection(&self) -> Option<(usize, usize)> {
        self.anchor.and_then(|a| {
            if a == self.cursor {
                None
            } else {
                Some((a.min(self.cursor), a.max(self.cursor)))
            }
        })
    }

    pub fn selected_text(&self) -> String {
        match self.selection() {
            Some((s, e)) => self.text.slice(s..e).to_string(),
            None => String::new(),
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    #[allow(dead_code)]
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    // ---- history -------------------------------------------------------

    fn checkpoint(&mut self) {
        self.undo.push(Snapshot {
            text: self.text.clone(), // Rope clone is O(1) (shared tree)
            cursor: self.cursor,
            anchor: self.anchor,
        });
        if self.undo.len() > 500 {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.dirty = true;
    }

    pub fn undo(&mut self) -> bool {
        match self.undo.pop() {
            Some(prev) => {
                self.redo.push(Snapshot {
                    text: self.text.clone(),
                    cursor: self.cursor,
                    anchor: self.anchor,
                });
                self.text = prev.text;
                self.cursor = prev.cursor;
                self.anchor = prev.anchor;
                self.goal_col = None;
                self.dirty = true;
                true
            }
            None => false,
        }
    }

    pub fn redo(&mut self) -> bool {
        match self.redo.pop() {
            Some(next) => {
                self.undo.push(Snapshot {
                    text: self.text.clone(),
                    cursor: self.cursor,
                    anchor: self.anchor,
                });
                self.text = next.text;
                self.cursor = next.cursor;
                self.anchor = next.anchor;
                self.goal_col = None;
                self.dirty = true;
                true
            }
            None => false,
        }
    }

    // ---- cursor & selection -------------------------------------------

    pub fn set_cursor(&mut self, pos: usize, extend: bool) {
        let pos = pos.min(self.text.len_chars());
        if extend {
            if self.anchor.is_none() {
                self.anchor = Some(self.cursor);
            }
        } else {
            self.anchor = None;
        }
        self.cursor = pos;
        self.goal_col = None;
    }

    pub fn move_cursor(&mut self, m: Move, extend: bool) {
        // Collapsing a selection with plain Left/Right jumps to its edge.
        if !extend {
            if let Some((s, e)) = self.selection() {
                match m {
                    Move::Left => return self.set_cursor(s, false),
                    Move::Right => return self.set_cursor(e, false),
                    _ => {}
                }
            }
        }
        let len = self.text.len_chars();
        let (line, col) = self.cursor_position();
        let mut keep_goal = false;
        let new = match m {
            Move::Left => self.cursor.saturating_sub(1),
            Move::Right => (self.cursor + 1).min(len),
            Move::Up => {
                keep_goal = true;
                if line == 0 {
                    0
                } else {
                    self.at_col(line - 1, self.goal_col.unwrap_or(col))
                }
            }
            Move::Down => {
                keep_goal = true;
                if line + 1 >= self.text.len_lines() {
                    len
                } else {
                    self.at_col(line + 1, self.goal_col.unwrap_or(col))
                }
            }
            Move::WordLeft => self.word_boundary_left(self.cursor),
            Move::WordRight => self.word_boundary_right(self.cursor),
            Move::LineStart => self.text.line_to_char(line),
            Move::LineEnd => self.line_end(line),
            Move::DocStart => 0,
            Move::DocEnd => len,
        };
        let goal = if keep_goal {
            Some(self.goal_col.unwrap_or(col))
        } else {
            None
        };
        self.set_cursor(new, extend);
        self.goal_col = goal;
    }

    fn line_end(&self, line: usize) -> usize {
        let start = self.text.line_to_char(line);
        start + self.line(line).chars().count()
    }

    fn at_col(&self, line: usize, col: usize) -> usize {
        let start = self.text.line_to_char(line);
        start + col.min(self.line(line).chars().count())
    }

    fn word_boundary_left(&self, mut p: usize) -> usize {
        while p > 0 && !is_word(self.text.char(p - 1)) {
            p -= 1;
        }
        while p > 0 && is_word(self.text.char(p - 1)) {
            p -= 1;
        }
        p
    }

    fn word_boundary_right(&self, mut p: usize) -> usize {
        let len = self.text.len_chars();
        while p < len && !is_word(self.text.char(p)) {
            p += 1;
        }
        while p < len && is_word(self.text.char(p)) {
            p += 1;
        }
        p
    }

    pub fn select_word(&mut self) {
        let len = self.text.len_chars();
        let mut s = self.cursor;
        let mut e = self.cursor;
        while s > 0 && is_word(self.text.char(s - 1)) {
            s -= 1;
        }
        while e < len && is_word(self.text.char(e)) {
            e += 1;
        }
        self.anchor = Some(s);
        self.cursor = e;
    }

    pub fn select_line(&mut self) {
        let (line, _) = self.cursor_position();
        let s = self.text.line_to_char(line);
        let e = if line + 1 < self.text.len_lines() {
            self.text.line_to_char(line + 1)
        } else {
            self.text.len_chars()
        };
        self.anchor = Some(s);
        self.cursor = e;
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.cursor = self.text.len_chars();
    }

    // ---- editing -------------------------------------------------------

    fn delete_selection_raw(&mut self) -> bool {
        if let Some((s, e)) = self.selection() {
            self.text.remove(s..e);
            self.cursor = s;
            self.anchor = None;
            true
        } else {
            self.anchor = None;
            false
        }
    }

    /// Insert text at the cursor, replacing any selection.
    pub fn insert(&mut self, s: &str) {
        if s.is_empty() && self.selection().is_none() {
            return;
        }
        self.checkpoint();
        self.delete_selection_raw();
        self.text.insert(self.cursor, s);
        self.cursor += s.chars().count();
        self.goal_col = None;
    }

    /// Smart insert for a single typed character: pairs brackets/quotes,
    /// wraps selections, and steps over an existing closing character.
    pub fn type_char(&mut self, c: char) {
        if let Some(close) = closing_pair(c) {
            if let Some((s, e)) = self.selection() {
                self.checkpoint();
                let inner = self.text.slice(s..e).to_string();
                self.text.remove(s..e);
                let wrapped = format!("{c}{inner}{close}");
                self.text.insert(s, &wrapped);
                self.anchor = Some(s + 1);
                self.cursor = s + 1 + inner.chars().count();
                return;
            }
            // Closing quote typed right before the same quote: step over it.
            if c == close && self.char_at_cursor() == Some(c) {
                self.set_cursor(self.cursor + 1, false);
                return;
            }
            // Don't auto-pair quotes in the middle of a word (e.g. don't).
            let prev_is_word = self.cursor > 0 && is_word(self.text.char(self.cursor - 1));
            if c == close && prev_is_word {
                self.insert(&c.to_string());
                return;
            }
            self.insert(&format!("{c}{close}"));
            self.cursor -= 1;
            return;
        }
        if matches!(c, ')' | ']' | '}') && self.char_at_cursor() == Some(c) {
            self.set_cursor(self.cursor + 1, false);
            return;
        }
        self.insert(&c.to_string());
    }

    /// Insert an opening/closing pair such as "{}" and put the cursor between.
    pub fn insert_pair(&mut self, open: char, close: char) {
        if self.selection().is_some() {
            self.type_char(open);
            return;
        }
        self.insert(&format!("{open}{close}"));
        self.cursor -= 1;
    }

    fn char_at_cursor(&self) -> Option<char> {
        if self.cursor < self.text.len_chars() {
            Some(self.text.char(self.cursor))
        } else {
            None
        }
    }

    /// Enter key: keep the current line's indentation, and indent one extra
    /// level (with the closer on its own line) when between `{ }`.
    pub fn newline(&mut self) {
        let (line, _) = self.cursor_position();
        let indent: String = self
            .line(line)
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        let before = if self.cursor > 0 {
            Some(self.text.char(self.cursor - 1))
        } else {
            None
        };
        let after = self.char_at_cursor();
        if matches!((before, after), (Some('{'), Some('}')) | (Some('['), Some(']')) | (Some('('), Some(')')))
        {
            let inner = format!("{indent}  ");
            self.insert(&format!("\n{inner}\n{indent}"));
            self.cursor -= 1 + indent.chars().count();
        } else if before == Some('{') || before == Some('[') || before == Some('(') {
            self.insert(&format!("\n{indent}  "));
        } else {
            self.insert(&format!("\n{indent}"));
        }
    }

    pub fn delete_backward(&mut self) {
        if self.selection().is_some() {
            self.checkpoint();
            self.delete_selection_raw();
            return;
        }
        if self.cursor == 0 {
            return;
        }
        self.checkpoint();
        // Remove an empty auto-inserted pair together.
        let prev = self.text.char(self.cursor - 1);
        let remove_pair = closing_pair(prev)
            .map(|cl| self.char_at_cursor() == Some(cl))
            .unwrap_or(false);
        let end = if remove_pair { self.cursor + 1 } else { self.cursor };
        self.text.remove(self.cursor - 1..end);
        self.cursor -= 1;
        self.goal_col = None;
    }

    pub fn delete_forward(&mut self) {
        if self.selection().is_some() {
            self.checkpoint();
            self.delete_selection_raw();
            return;
        }
        if self.cursor >= self.text.len_chars() {
            return;
        }
        self.checkpoint();
        self.text.remove(self.cursor..self.cursor + 1);
    }

    pub fn cut(&mut self) -> String {
        let s = self.selected_text();
        if !s.is_empty() {
            self.checkpoint();
            self.delete_selection_raw();
        }
        s
    }

    pub fn duplicate_line(&mut self) {
        self.checkpoint();
        let (line, col) = self.cursor_position();
        let content = self.line(line);
        let end = self.line_end(line);
        self.text.insert(end, &format!("\n{content}"));
        self.anchor = None;
        self.cursor = self.text.line_to_char(line + 1) + col;
    }

    /// Toggle a line comment on every line touched by the selection.
    pub fn toggle_comment(&mut self, prefix: &str) {
        let (first, last) = match self.selection() {
            Some((s, e)) => {
                let l1 = self.text.char_to_line(s);
                let mut l2 = self.text.char_to_line(e);
                if l2 > l1 && self.text.line_to_char(l2) == e {
                    l2 -= 1; // selection ends at start of a line: exclude it
                }
                (l1, l2)
            }
            None => {
                let l = self.cursor_position().0;
                (l, l)
            }
        };
        let marker = format!("{prefix} ");
        let all_commented = (first..=last).all(|l| {
            let t = self.line(l);
            let t = t.trim_start();
            t.is_empty() || t.starts_with(prefix)
        });
        self.checkpoint();
        let mut delta: isize = 0;
        for l in first..=last {
            let text = self.line(l);
            let indent_len = text.chars().take_while(|c| *c == ' ' || *c == '\t').count();
            if text.trim().is_empty() {
                continue;
            }
            let at = self.text.line_to_char(l) + indent_len;
            if all_commented {
                let rest: String = text.chars().skip(indent_len).collect();
                let n = if rest.starts_with(&marker) {
                    marker.chars().count()
                } else {
                    prefix.chars().count()
                };
                self.text.remove(at..at + n);
                delta -= n as isize;
            } else {
                self.text.insert(at, &marker);
                delta += marker.chars().count() as isize;
            }
        }
        self.anchor = None;
        self.cursor = (self.cursor as isize + delta).max(0) as usize;
        self.cursor = self.cursor.min(self.text.len_chars());
    }

    #[allow(dead_code)]
    pub fn replace_all_text(&mut self, new_text: &str) {
        self.checkpoint();
        self.text = Rope::from_str(new_text);
        self.cursor = self.cursor.min(self.text.len_chars());
        self.anchor = None;
    }

    // ---- search --------------------------------------------------------

    /// Find all non-overlapping matches; returns (start, end) in chars.
    pub fn find_all(&self, needle: &str, case_sensitive: bool) -> Vec<(usize, usize)> {
        if needle.is_empty() {
            return Vec::new();
        }
        let hay: Vec<char> = self.text.chars().collect();
        let pat: Vec<char> = needle.chars().collect();
        let eq = |a: char, b: char| {
            if case_sensitive {
                a == b
            } else {
                a.to_lowercase().eq(b.to_lowercase())
            }
        };
        let mut out = Vec::new();
        let mut i = 0;
        while i + pat.len() <= hay.len() {
            if (0..pat.len()).all(|k| eq(hay[i + k], pat[k])) {
                out.push((i, i + pat.len()));
                i += pat.len();
            } else {
                i += 1;
            }
        }
        out
    }

    pub fn replace_all(&mut self, needle: &str, with: &str, case_sensitive: bool) -> usize {
        let hits = self.find_all(needle, case_sensitive);
        if hits.is_empty() {
            return 0;
        }
        self.checkpoint();
        for (s, e) in hits.iter().rev() {
            self.text.remove(*s..*e);
            self.text.insert(*s, with);
        }
        self.cursor = self.cursor.min(self.text.len_chars());
        self.anchor = None;
        hits.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_undo_redo() {
        let mut b = Buffer::new("");
        b.insert("hello");
        b.insert(" world");
        assert_eq!(b.text(), "hello world");
        assert!(b.undo());
        assert_eq!(b.text(), "hello");
        assert!(b.redo());
        assert_eq!(b.text(), "hello world");
        assert!(b.is_dirty());
    }

    #[test]
    fn smart_braces_put_cursor_between() {
        let mut b = Buffer::new("");
        b.insert_pair('{', '}');
        assert_eq!(b.text(), "{}");
        assert_eq!(b.cursor(), 1);
        b.type_char('}');
        assert_eq!(b.cursor(), 2);
        assert_eq!(b.text(), "{}");
    }

    #[test]
    fn typing_quote_pairs_and_steps_over() {
        let mut b = Buffer::new("");
        b.type_char('"');
        assert_eq!(b.text(), "\"\"");
        b.type_char('a');
        b.type_char('"');
        assert_eq!(b.text(), "\"a\"");
        assert_eq!(b.cursor(), 3);
    }

    #[test]
    fn apostrophe_in_word_does_not_pair() {
        let mut b = Buffer::new("don");
        b.set_cursor(3, false);
        b.type_char('\'');
        assert_eq!(b.text(), "don'");
    }

    #[test]
    fn wrap_selection_in_brackets() {
        let mut b = Buffer::new("abc");
        b.select_all();
        b.type_char('(');
        assert_eq!(b.text(), "(abc)");
    }

    #[test]
    fn backspace_removes_empty_pair() {
        let mut b = Buffer::new("");
        b.insert_pair('(', ')');
        b.delete_backward();
        assert_eq!(b.text(), "");
    }

    #[test]
    fn newline_keeps_indent_and_expands_braces() {
        let mut b = Buffer::new("  fn x() {}");
        b.set_cursor(b.len_chars() - 1, false);
        b.newline();
        assert_eq!(b.text(), "  fn x() {\n    \n  }");
        assert_eq!(b.cursor_position(), (1, 4));
    }

    #[test]
    fn vertical_movement_remembers_column() {
        let mut b = Buffer::new("abcdef\nab\nabcdef");
        b.set_cursor(5, false);
        b.move_cursor(Move::Down, false);
        assert_eq!(b.cursor_position(), (1, 2));
        b.move_cursor(Move::Down, false);
        assert_eq!(b.cursor_position(), (2, 5));
    }

    #[test]
    fn selection_via_move_and_collapse() {
        let mut b = Buffer::new("hello world");
        b.move_cursor(Move::Right, true);
        b.move_cursor(Move::Right, true);
        assert_eq!(b.selected_text(), "he");
        b.move_cursor(Move::Right, false);
        assert!(b.selection().is_none());
        assert_eq!(b.cursor(), 2);
    }

    #[test]
    fn select_word_line_all() {
        let mut b = Buffer::new("foo bar\nbaz");
        b.set_cursor(5, false);
        b.select_word();
        assert_eq!(b.selected_text(), "bar");
        b.select_line();
        assert_eq!(b.selected_text(), "foo bar\n");
        b.select_all();
        assert_eq!(b.selected_text(), "foo bar\nbaz");
    }

    #[test]
    fn word_movement() {
        let mut b = Buffer::new("foo_bar baz");
        b.move_cursor(Move::WordRight, false);
        assert_eq!(b.cursor(), 7);
        b.move_cursor(Move::WordLeft, false);
        assert_eq!(b.cursor(), 0);
    }

    #[test]
    fn duplicate_line_works() {
        let mut b = Buffer::new("a\nb");
        b.duplicate_line();
        assert_eq!(b.text(), "a\na\nb");
        assert_eq!(b.cursor_position().0, 1);
    }

    #[test]
    fn toggle_comment_roundtrip() {
        let mut b = Buffer::new("  x = 1\ny = 2");
        b.select_all();
        b.toggle_comment("//");
        assert_eq!(b.text(), "  // x = 1\n// y = 2");
        b.select_all();
        b.toggle_comment("//");
        assert_eq!(b.text(), "  x = 1\ny = 2");
    }

    #[test]
    fn cut_and_unicode() {
        let mut b = Buffer::new("héllo مرحبا");
        b.select_all();
        assert_eq!(b.cut(), "héllo مرحبا");
        assert_eq!(b.text(), "");
    }

    #[test]
    fn find_and_replace() {
        let mut b = Buffer::new("Foo foo FOO");
        assert_eq!(b.find_all("foo", false).len(), 3);
        assert_eq!(b.find_all("foo", true).len(), 1);
        assert_eq!(b.replace_all("foo", "x", false), 3);
        assert_eq!(b.text(), "x x x");
        assert!(b.undo());
        assert_eq!(b.text(), "Foo foo FOO");
    }
}
