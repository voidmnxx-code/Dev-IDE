//! Language registry: built-in languages plus user-defined ones.
//!
//! A language says which file extensions it owns, which toolchain packages it
//! needs and how to run a file. Run templates are executed with `sh -c`, so they
//! may chain commands. Placeholders (already shell-quoted when substituted):
//! `{file}` project-relative path, `{stem}` file name without extension,
//! `{name}` file name, `{dir}` directory of the file.
//! The special run value `@preview` opens the web preview instead.

use crate::IdeError;
use serde::{Deserialize, Serialize};

#[derive(uniffi::Record, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Language {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub extensions: Vec<String>,
    /// Catalog package ids that must be installed to run this language.
    #[serde(default)]
    pub packages: Vec<String>,
    #[serde(default)]
    pub run: String,
    /// Language-server command (used by the editor once LSP lands); may be empty.
    #[serde(default)]
    pub lsp: String,
    #[serde(default)]
    pub comment: String,
    #[serde(default, skip_serializing)]
    pub builtin: bool,
}

fn b(id: &str, name: &str, ext: &[&str], pk: &[&str], run: &str, lsp: &str, comment: &str) -> Language {
    Language {
        id: id.into(),
        name: name.into(),
        extensions: ext.iter().map(|s| s.to_string()).collect(),
        packages: pk.iter().map(|s| s.to_string()).collect(),
        run: run.into(),
        lsp: lsp.into(),
        comment: comment.into(),
        builtin: true,
    }
}

pub fn builtin() -> Vec<Language> {
    const OUT: &str = ".build";
    vec![
        b("c", "C", &["c", "h"], &["clang"],
          &format!("mkdir -p {OUT} && clang -g {{file}} -o {OUT}/{{stem}} && {OUT}/{{stem}}"), "clangd", "//"),
        b("cpp", "C++", &["cpp", "cc", "cxx", "hpp", "hh"], &["clang"],
          &format!("mkdir -p {OUT} && clang++ -g -std=c++20 {{file}} -o {OUT}/{{stem}} && {OUT}/{{stem}}"), "clangd", "//"),
        b("rust", "Rust", &["rs"], &["rust"],
          &format!("if [ -f Cargo.toml ]; then cargo run; else mkdir -p {OUT} && rustc {{file}} -o {OUT}/{{stem}} && {OUT}/{{stem}}; fi"),
          "rust-analyzer", "//"),
        b("java", "Java", &["java"], &["jdk"],
          &format!("mkdir -p {OUT} && javac -d {OUT} {{file}} && java -cp {OUT} {{stem}}"), "jdtls", "//"),
        b("csharp", "C#", &["cs", "csproj"], &["dotnet"], "dotnet run", "omnisharp", "//"),
        b("sql", "SQL", &["sql"], &["sqlite"],
          &format!("mkdir -p {OUT} && sqlite3 -header -column {OUT}/data.db < {{file}}"), "", "--"),
        // T-SQL: highlighting only. SQL Server has no Android build; running needs a remote server.
        b("tsql", "T-SQL", &["tsql"], &[], "", "", "--"),
        b("html", "HTML", &["html", "htm"], &[], "@preview", "", ""),
        b("css", "CSS", &["css"], &[], "@preview", "", ""),
        b("javascript", "JavaScript", &["js", "mjs", "cjs"], &["nodejs"], "node {file}", "", "//"),
        b("typescript", "TypeScript", &["ts"], &["nodejs"], "node --experimental-strip-types {file}", "", "//"),
        b("python", "Python", &["py"], &["python"], "python3 {file}", "", "#"),
        b("shell", "Shell", &["sh"], &[], "sh {file}", "", "#"),
    ]
}

/// Built-ins overridden/extended by a user JSON array (may be empty).
pub fn merged(user_json: &str) -> Result<Vec<Language>, IdeError> {
    let mut all = builtin();
    for mut u in parse_user(user_json)? {
        u.builtin = false;
        match all.iter_mut().find(|l| l.id == u.id) {
            Some(slot) => *slot = u,
            None => all.push(u),
        }
    }
    Ok(all)
}

pub fn parse_user(json: &str) -> Result<Vec<Language>, IdeError> {
    if json.trim().is_empty() {
        return Ok(vec![]);
    }
    let list: Vec<Language> =
        serde_json::from_str(json).map_err(|e| IdeError::Language(format!("languages: {e}")))?;
    for l in &list {
        if l.id.is_empty()
            || !l.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            return Err(IdeError::Language(format!("language id '{}' must be letters, digits, - or _", l.id)));
        }
        if l.name.trim().is_empty() {
            return Err(IdeError::Language(format!("language '{}' needs a name", l.id)));
        }
    }
    Ok(list)
}

pub fn for_file<'a>(langs: &'a [Language], path: &str) -> Option<&'a Language> {
    let name = path.rsplit('/').next().unwrap_or(path);
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    // Later entries (user languages) win over built-ins.
    langs.iter().rev().find(|l| l.extensions.iter().any(|e| e.eq_ignore_ascii_case(&ext)))
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

pub fn render(template: &str, path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    let stem = name.rsplit_once('.').map(|x| x.0).unwrap_or(name);
    let dir = path.rsplit_once('/').map(|x| x.0).unwrap_or(".");
    template
        .replace("{file}", &quote(path))
        .replace("{stem}", &quote(stem))
        .replace("{name}", &quote(name))
        .replace("{dir}", &quote(dir))
}

pub fn missing(lang: &Language, installed: &[String]) -> Vec<String> {
    lang.packages.iter().filter(|p| !installed.contains(p)).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_ids_unique_and_cover_requested_languages() {
        let l = builtin();
        let mut ids: Vec<_> = l.iter().map(|x| x.id.clone()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), l.len());
        for want in ["c", "cpp", "rust", "csharp", "sql", "tsql", "css", "html", "java"] {
            assert!(l.iter().any(|x| x.id == want), "{want}");
        }
    }

    #[test]
    fn finds_language_by_extension() {
        let l = builtin();
        assert_eq!(for_file(&l, "src/Main.JAVA").unwrap().id, "java");
        assert_eq!(for_file(&l, "a/b.hpp").unwrap().id, "cpp");
        assert!(for_file(&l, "Makefile").is_none());
    }

    #[test]
    fn user_language_extends_and_overrides() {
        let json = r#"[{"id":"zig","name":"Zig","extensions":["zig"],"packages":["zig"],"run":"zig run {file}"},
                       {"id":"python","name":"Python","extensions":["py"],"run":"python {file}"}]"#;
        let l = merged(json).unwrap();
        assert_eq!(for_file(&l, "x.zig").unwrap().id, "zig");
        let py = l.iter().find(|x| x.id == "python").unwrap();
        assert_eq!(py.run, "python {file}");
        assert!(!py.builtin);
    }

    #[test]
    fn rejects_bad_user_ids_and_json() {
        assert!(parse_user(r#"[{"id":"a b","name":"X"}]"#).is_err());
        assert!(parse_user(r#"[{"id":"x","name":" "}]"#).is_err());
        assert!(parse_user("{").is_err());
        assert!(parse_user("").unwrap().is_empty());
    }

    #[test]
    fn render_quotes_values() {
        let r = render("cc {file} -o {stem}", "my dir/it's.c");
        assert_eq!(r, "cc 'my dir/it'\\''s.c' -o 'it'\\''s'");
        assert_eq!(render("ls {dir}", "a/b/c.c"), "ls 'a/b'");
    }

    #[test]
    fn reports_missing_packages() {
        let l = builtin();
        let c = l.iter().find(|x| x.id == "c").unwrap();
        assert_eq!(missing(c, &[]), vec!["clang"]);
        assert!(missing(c, &["clang".into()]).is_empty());
    }
}
