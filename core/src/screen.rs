//! A deliberately small terminal emulator: enough to render shell sessions
//! (prompts, line editing, `clear`, progress lines using `\r`) as plain text.
//! Colours and cursor-addressed full-screen apps are out of scope for now.

pub const MAX_LINES: usize = 3000;

pub struct Screen {
    lines: Vec<Vec<char>>,
    row: usize,
    col: usize,
    state: State,
    params: String,
    /// Bytes of an incomplete UTF-8 sequence carried between reads.
    pending: Vec<u8>,
    /// Terminal width in columns; 0 = unlimited. Text wraps when it is exceeded.
    cols: usize,
}

#[derive(PartialEq)]
enum State {
    Normal,
    Esc,
    Csi,
    /// OSC string (window title etc.), ends with BEL or ESC \
    Osc,
    OscEsc,
    /// ESC ( X  style two-byte sequences
    SkipOne,
}

impl Default for Screen {
    fn default() -> Self {
        Self::new()
    }
}

impl Screen {
    pub fn new() -> Self {
        Screen {
            lines: vec![Vec::new()],
            row: 0,
            col: 0,
            state: State::Normal,
            params: String::new(),
            pending: Vec::new(),
            cols: 0,
        }
    }

    /// Tell the screen how wide the terminal is, so long lines wrap like on a real terminal.
    pub fn set_cols(&mut self, cols: usize) {
        self.cols = cols;
    }

    pub fn lines(&self) -> Vec<String> {
        self.lines.iter().map(|l| l.iter().collect()).collect()
    }

    pub fn cursor(&self) -> (usize, usize) {
        (self.row, self.col)
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        let mut buf = std::mem::take(&mut self.pending);
        buf.extend_from_slice(bytes);
        let valid_up_to = match std::str::from_utf8(&buf) {
            Ok(_) => buf.len(),
            Err(e) => {
                if e.error_len().is_none() {
                    e.valid_up_to() // incomplete tail: keep for next time
                } else {
                    buf.len() // genuinely invalid bytes: lossy below
                }
            }
        };
        self.pending = buf.split_off(valid_up_to);
        for ch in String::from_utf8_lossy(&buf).chars() {
            self.put(ch);
        }
    }

    fn put(&mut self, c: char) {
        match self.state {
            State::Normal => self.normal(c),
            State::Esc => match c {
                '[' => {
                    self.state = State::Csi;
                    self.params.clear();
                }
                ']' => self.state = State::Osc,
                '(' | ')' | '*' | '+' | '#' => self.state = State::SkipOne,
                _ => self.state = State::Normal, // ESC = / > / 7 / 8 / c ...: ignored
            },
            State::SkipOne => self.state = State::Normal,
            State::Osc => match c {
                '\u{7}' => self.state = State::Normal,
                '\u{1b}' => self.state = State::OscEsc,
                _ => {}
            },
            State::OscEsc => self.state = State::Normal,
            State::Csi => {
                if c.is_ascii_digit() || matches!(c, ';' | '?' | '>' | '!' | ' ') {
                    self.params.push(c);
                } else {
                    let p = std::mem::take(&mut self.params);
                    self.state = State::Normal;
                    self.csi(c, &p);
                }
            }
        }
    }

    fn normal(&mut self, c: char) {
        match c {
            '\u{1b}' => self.state = State::Esc,
            '\n' => {
                self.row += 1;
                self.ensure_row();
            }
            '\r' => self.col = 0,
            '\u{8}' => self.col = self.col.saturating_sub(1),
            '\t' => {
                let next = (self.col / 8 + 1) * 8;
                while self.col < next {
                    self.write_char(' ');
                }
            }
            '\u{7}' | '\0' => {}
            c if (c as u32) < 0x20 => {}
            c => self.write_char(c),
        }
    }

    fn ensure_row(&mut self) {
        while self.lines.len() <= self.row {
            self.lines.push(Vec::new());
        }
        if self.lines.len() > MAX_LINES {
            let drop = self.lines.len() - MAX_LINES;
            self.lines.drain(..drop);
            self.row = self.row.saturating_sub(drop);
        }
    }

    fn write_char(&mut self, c: char) {
        // Deferred wrap: a full line only wraps when another character arrives.
        if self.cols > 0 && self.col >= self.cols {
            self.row += 1;
            self.col = 0;
        }
        self.ensure_row();
        let line = &mut self.lines[self.row];
        while line.len() < self.col {
            line.push(' ');
        }
        if self.col < line.len() {
            line[self.col] = c;
        } else {
            line.push(c);
        }
        self.col += 1;
    }

    fn csi(&mut self, final_byte: char, params: &str) {
        if params.starts_with('?') || params.starts_with('>') {
            return; // private modes (bracketed paste, cursor visibility...): ignore
        }
        let nums: Vec<usize> = params
            .split(';')
            .map(|s| s.parse::<usize>().unwrap_or(0))
            .collect();
        let n = |i: usize, default: usize| match nums.get(i) {
            Some(&v) if v > 0 => v,
            _ => default,
        };
        self.ensure_row();
        match final_byte {
            'A' => self.row = self.row.saturating_sub(n(0, 1)),
            'B' => {
                self.row += n(0, 1);
                self.ensure_row();
            }
            'C' => self.col += n(0, 1),
            'D' => self.col = self.col.saturating_sub(n(0, 1)),
            'G' => self.col = n(0, 1) - 1,
            'K' => {
                let line = &mut self.lines[self.row];
                match nums.first().copied().unwrap_or(0) {
                    0 => line.truncate(self.col),
                    1 => {
                        for i in 0..self.col.min(line.len()) {
                            line[i] = ' ';
                        }
                    }
                    _ => line.clear(),
                }
            }
            'P' => {
                let line = &mut self.lines[self.row];
                let end = (self.col + n(0, 1)).min(line.len());
                if self.col < line.len() {
                    line.drain(self.col..end);
                }
            }
            '@' => {
                let line = &mut self.lines[self.row];
                while line.len() < self.col {
                    line.push(' ');
                }
                for _ in 0..n(0, 1) {
                    line.insert(self.col, ' ');
                }
            }
            'X' => {
                let line = &mut self.lines[self.row];
                for i in self.col..(self.col + n(0, 1)).min(line.len()) {
                    line[i] = ' ';
                }
            }
            'J' => {
                // 2J / 3J (clear screen): wipe everything. 0J: erase below.
                match nums.first().copied().unwrap_or(0) {
                    2 | 3 => {
                        self.lines = vec![Vec::new()];
                        self.row = 0;
                        self.col = 0;
                    }
                    0 => {
                        self.lines.truncate(self.row + 1);
                        let c = self.col;
                        self.lines[self.row].truncate(c);
                    }
                    _ => {}
                }
            }
            'H' | 'f' => {
                // Home (e.g. after `clear`): only honour it when the screen is empty-ish.
                if nums.iter().all(|&v| v <= 1) {
                    self.col = 0;
                }
            }
            _ => {} // 'm' (colours) and everything else
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &Screen) -> String {
        s.lines().join("\n")
    }

    #[test]
    fn plain_text_and_newlines() {
        let mut s = Screen::new();
        s.feed(b"hello\r\nworld");
        assert_eq!(text(&s), "hello\nworld");
        assert_eq!(s.cursor(), (1, 5));
    }

    #[test]
    fn carriage_return_overwrites() {
        let mut s = Screen::new();
        s.feed(b"10%\r50%\r100%");
        assert_eq!(text(&s), "100%");
    }

    #[test]
    fn backspace_and_erase_to_eol() {
        let mut s = Screen::new();
        s.feed(b"abcd\x08\x08\x1b[K");
        assert_eq!(text(&s), "ab");
    }

    #[test]
    fn colours_and_titles_are_stripped() {
        let mut s = Screen::new();
        s.feed(b"\x1b]0;title\x07\x1b[1;32mok\x1b[0m done\x1b(B");
        assert_eq!(text(&s), "ok done");
    }

    #[test]
    fn utf8_split_across_reads() {
        let mut s = Screen::new();
        let bytes = "مرحبا".as_bytes();
        s.feed(&bytes[..3]);
        s.feed(&bytes[3..]);
        assert_eq!(text(&s), "مرحبا");
    }

    #[test]
    fn clear_screen_resets() {
        let mut s = Screen::new();
        s.feed(b"junk\r\nmore\x1b[H\x1b[2Jfresh");
        assert_eq!(text(&s), "fresh");
    }

    #[test]
    fn cursor_left_and_delete_char() {
        let mut s = Screen::new();
        s.feed(b"abc\x1b[2D\x1b[P");
        assert_eq!(text(&s), "ac");
    }

    #[test]
    fn long_lines_wrap_at_the_terminal_width() {
        let mut s = Screen::new();
        s.set_cols(5);
        s.feed(b"abcdefgh");
        assert_eq!(s.lines(), vec!["abcde", "fgh"]);
        assert_eq!(s.cursor(), (1, 3));
    }

    #[test]
    fn exactly_full_line_followed_by_crlf_does_not_add_a_blank_line() {
        let mut s = Screen::new();
        s.set_cols(5);
        s.feed(b"abcde\r\nxy");
        assert_eq!(s.lines(), vec!["abcde", "xy"]);
    }

    #[test]
    fn scrollback_is_bounded() {
        let mut s = Screen::new();
        for i in 0..(MAX_LINES + 50) {
            s.feed(format!("line{i}\r\n").as_bytes());
        }
        assert!(s.lines().len() <= MAX_LINES);
        assert!(s.lines().iter().any(|l| l == &format!("line{}", MAX_LINES + 49)));
    }
}
