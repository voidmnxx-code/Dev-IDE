//! Syntax highlighting with tree-sitter. Whole-document, error tolerant.

use std::collections::HashMap;
use std::sync::OnceLock;
use tree_sitter_highlight::{Highlight, HighlightConfiguration, HighlightEvent, Highlighter};

/// Capture-name prefixes we colour. The index is the `TokenKind` discriminant.
const NAMES: &[&str] = &[
    "keyword",
    "string",
    "comment",
    "number",
    "function",
    "type",
    "constant",
    "variable.builtin",
    "operator",
    "punctuation",
    "property",
    "attribute",
    "tag",
    "escape",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TokenKind {
    Keyword,
    String,
    Comment,
    Number,
    Function,
    Type,
    Constant,
    Builtin,
    Operator,
    Punctuation,
    Property,
    Attribute,
    Tag,
    Escape,
}

const KINDS: [TokenKind; 14] = [
    TokenKind::Keyword,
    TokenKind::String,
    TokenKind::Comment,
    TokenKind::Number,
    TokenKind::Function,
    TokenKind::Type,
    TokenKind::Constant,
    TokenKind::Builtin,
    TokenKind::Operator,
    TokenKind::Punctuation,
    TokenKind::Property,
    TokenKind::Attribute,
    TokenKind::Tag,
    TokenKind::Escape,
];

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TokenSpan {
    /// Char offsets (Unicode scalar values) into the document, end exclusive.
    pub start: u32,
    pub end: u32,
    pub kind: TokenKind,
}

/// Map a file name to a language id, if we have a grammar for it.
pub fn language_for(file_name: &str) -> Option<&'static str> {
    let ext = file_name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())?;
    Some(match ext.as_str() {
        "js" | "mjs" | "cjs" | "jsx" => "javascript",
        "py" => "python",
        "rs" => "rust",
        "json" => "json",
        "kt" | "kts" => "kotlin",
        "html" | "htm" => "html",
        "css" => "css",
        "sh" | "bash" => "bash",
        "toml" => "toml",
        _ => return None,
    })
}

fn build(lang: tree_sitter::Language, name: &str, highlights: &str, injections: &str) -> Option<HighlightConfiguration> {
    let mut cfg = HighlightConfiguration::new(lang, name, highlights, injections, "").ok()?;
    cfg.configure(NAMES);
    Some(cfg)
}

fn configs() -> &'static HashMap<&'static str, HighlightConfiguration> {
    static CONFIGS: OnceLock<HashMap<&'static str, HighlightConfiguration>> = OnceLock::new();
    CONFIGS.get_or_init(|| {
        let mut m = HashMap::new();
        let mut add = |id: &'static str, cfg: Option<HighlightConfiguration>| {
            if let Some(c) = cfg {
                m.insert(id, c);
            }
        };
        add("javascript", build(tree_sitter_javascript::LANGUAGE.into(), "javascript", tree_sitter_javascript::HIGHLIGHT_QUERY, ""));
        add("python", build(tree_sitter_python::LANGUAGE.into(), "python", tree_sitter_python::HIGHLIGHTS_QUERY, ""));
        add("rust", build(tree_sitter_rust::LANGUAGE.into(), "rust", tree_sitter_rust::HIGHLIGHTS_QUERY, ""));
        add("json", build(tree_sitter_json::LANGUAGE.into(), "json", tree_sitter_json::HIGHLIGHTS_QUERY, ""));
        add("kotlin", build(tree_sitter_kotlin_ng::LANGUAGE.into(), "kotlin", include_str!("../queries/kotlin.scm"), ""));
        add("html", build(tree_sitter_html::LANGUAGE.into(), "html", tree_sitter_html::HIGHLIGHTS_QUERY, ""));
        add("css", build(tree_sitter_css::LANGUAGE.into(), "css", tree_sitter_css::HIGHLIGHTS_QUERY, ""));
        add("bash", build(tree_sitter_bash::LANGUAGE.into(), "bash", tree_sitter_bash::HIGHLIGHT_QUERY, ""));
        add("toml", build(tree_sitter_toml_ng::LANGUAGE.into(), "toml", tree_sitter_toml_ng::HIGHLIGHTS_QUERY, ""));
        m
    })
}

/// Highlight `text`; returns non-overlapping spans sorted by position.
pub fn highlight(lang: &str, text: &str) -> Vec<TokenSpan> {
    let Some(cfg) = configs().get(lang) else {
        return Vec::new();
    };
    // byte offset -> char offset
    let mut b2c = vec![0u32; text.len() + 1];
    let mut chars = 0u32;
    for (b, ch) in text.char_indices() {
        for k in 0..ch.len_utf8() {
            b2c[b + k] = chars;
        }
        chars += 1;
    }
    b2c[text.len()] = chars;

    let mut hl = Highlighter::new();
    let Ok(events) = hl.highlight(cfg, text.as_bytes(), None, None, |_| None) else {
        return Vec::new();
    };
    let mut out: Vec<TokenSpan> = Vec::new();
    let mut stack: Vec<Highlight> = Vec::new();
    for ev in events {
        match ev {
            Ok(HighlightEvent::HighlightStart(h)) => stack.push(h),
            Ok(HighlightEvent::HighlightEnd) => {
                stack.pop();
            }
            Ok(HighlightEvent::Source { start, end }) => {
                if let Some(top) = stack.last() {
                    let (s, e) = (b2c[start], b2c[end]);
                    if e > s {
                        let kind = KINDS[top.0];
                        // merge with the previous span when adjacent and same kind
                        match out.last_mut() {
                            Some(prev) if prev.end == s && prev.kind == kind => prev.end = e,
                            _ => out.push(TokenSpan { start: s, end: e, kind }),
                        }
                    }
                }
            }
            Err(_) => return out,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds_of(lang: &str, src: &str) -> Vec<(String, TokenKind)> {
        let chars: Vec<char> = src.chars().collect();
        highlight(lang, src)
            .into_iter()
            .map(|s| (chars[s.start as usize..s.end as usize].iter().collect(), s.kind))
            .collect()
    }

    #[test]
    fn detects_languages_by_extension() {
        assert_eq!(language_for("app.js"), Some("javascript"));
        assert_eq!(language_for("Main.KT"), Some("kotlin"));
        assert_eq!(language_for("README"), None);
        assert_eq!(language_for("a.unknown"), None);
    }

    #[test]
    fn every_language_has_a_working_grammar() {
        for lang in ["javascript", "python", "rust", "json", "kotlin", "html", "css", "bash", "toml"] {
            assert!(configs().contains_key(lang), "grammar failed to load: {lang}");
        }
    }

    #[test]
    fn javascript_tokens() {
        let t = kinds_of("javascript", "const a = \"hi\" // note\nlet n = 42");
        assert!(t.contains(&("const".into(), TokenKind::Keyword)), "{t:?}");
        assert!(t.iter().any(|(s, k)| s == "\"hi\"" && *k == TokenKind::String), "{t:?}");
        assert!(t.iter().any(|(s, k)| s.starts_with("// note") && *k == TokenKind::Comment), "{t:?}");
        assert!(t.iter().any(|(s, k)| s == "42" && *k == TokenKind::Number), "{t:?}");
    }

    #[test]
    fn rust_and_python_and_kotlin() {
        let r = kinds_of("rust", "fn main() { let x: u32 = 1; }");
        assert!(r.contains(&("fn".into(), TokenKind::Keyword)), "{r:?}");
        let p = kinds_of("python", "def f():\n    return 'x'");
        assert!(p.contains(&("def".into(), TokenKind::Keyword)), "{p:?}");
        let k = kinds_of("kotlin", "fun main() { val s = \"a\" /* c */ }");
        assert!(k.contains(&("fun".into(), TokenKind::Keyword)), "{k:?}");
        assert!(k.iter().any(|(s, kd)| s == "\"a\"" && *kd == TokenKind::String), "{k:?}");
        assert!(k.iter().any(|(_, kd)| *kd == TokenKind::Comment), "{k:?}");
    }

    #[test]
    fn offsets_are_in_chars_not_bytes() {
        // Arabic text before the token: byte and char offsets differ.
        let src = "// مرحبا\nconst x = 1";
        let chars: Vec<char> = src.chars().collect();
        let spans = highlight("javascript", src);
        let c = spans.iter().find(|s| s.kind == TokenKind::Keyword).unwrap();
        let word: String = chars[c.start as usize..c.end as usize].iter().collect();
        assert_eq!(word, "const");
    }

    #[test]
    fn broken_code_still_highlights_and_unknown_language_is_empty() {
        let t = kinds_of("javascript", "const a = (\n// still a comment");
        assert!(t.iter().any(|(_, k)| *k == TokenKind::Comment), "{t:?}");
        assert!(highlight("cobol", "x").is_empty());
    }
}
