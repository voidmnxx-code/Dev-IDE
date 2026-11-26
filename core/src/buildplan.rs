//! Decide how to build/run a project, and turn tool output into diagnostics.

use regex::Regex;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ProjectKind {
    AndroidGradle,
    CargoGame,
    Cargo,
    Node,
    WebStatic,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub kind: ProjectKind,
    pub description: String,
    /// argv; empty when nothing needs building (static web).
    pub command: Vec<String>,
    /// Extra environment ("KEY=VALUE").
    pub env: Vec<String>,
    /// The result can be previewed in the browser/WebView.
    pub previewable: bool,
}

fn read(root: &Path, rel: &str) -> String {
    std::fs::read_to_string(root.join(rel)).unwrap_or_default()
}

fn is_android_app(root: &Path) -> bool {
    ["app/build.gradle.kts", "app/build.gradle", "build.gradle.kts", "build.gradle"]
        .iter()
        .any(|f| read(root, f).contains("com.android.application"))
}

/// `aapt2` is the path of a native aarch64 aapt2 (the official one is x86_64 only);
/// it is passed to Gradle through `android.aapt2FromMavenOverride`.
pub fn detect(root: &Path, aapt2: Option<&str>) -> Plan {
    let has = |f: &str| root.join(f).is_file();

    if (has("settings.gradle.kts") || has("settings.gradle") || has("build.gradle.kts") || has("build.gradle"))
        && is_android_app(root)
    {
        // The system Gradle is used on purpose: ./gradlew can't run from `noexec` storage
        // and downloads a distribution we may not be able to execute.
        let mut cmd: Vec<String> = ["gradle", "assembleDebug", "--console=plain"].map(String::from).to_vec();
        if let Some(a) = aapt2.filter(|a| !a.is_empty()) {
            cmd.push(format!("-Pandroid.aapt2FromMavenOverride={a}"));
        }
        return Plan {
            kind: ProjectKind::AndroidGradle,
            description: "Android app (Gradle assembleDebug)".into(),
            command: cmd,
            env: vec![],
            previewable: false,
        };
    }

    if has("Cargo.toml") {
        let manifest = read(root, "Cargo.toml");
        let game = manifest.contains("macroquad") || manifest.contains("bevy");
        if game {
            let cmd: Vec<String> = if has("build.sh") {
                vec!["sh".into(), "build.sh".into()]
            } else {
                ["cargo", "build", "--release", "--target", "wasm32-unknown-unknown"].map(String::from).to_vec()
            };
            return Plan {
                kind: ProjectKind::CargoGame,
                description: "Rust game (WebAssembly build)".into(),
                command: cmd,
                env: vec![],
                previewable: has("index.html") || has("web/index.html"),
            };
        }
        return Plan {
            kind: ProjectKind::Cargo,
            description: "Rust crate (cargo run)".into(),
            command: ["cargo", "run"].map(String::from).to_vec(),
            env: vec![],
            previewable: false,
        };
    }

    if has("package.json") {
        let pj = read(root, "package.json");
        let has_build = pj.contains("\"build\"");
        let cmd: Vec<String> = if has_build {
            ["npm", "run", "build"].map(String::from).to_vec()
        } else {
            ["npm", "start"].map(String::from).to_vec()
        };
        return Plan {
            kind: ProjectKind::Node,
            description: if has_build { "Node project (npm run build)".into() } else { "Node project (npm start)".into() },
            command: cmd,
            env: vec![],
            previewable: has("index.html") || has("dist/index.html"),
        };
    }

    if has("index.html") || has("public/index.html") {
        return Plan {
            kind: ProjectKind::WebStatic,
            description: "Static website".into(),
            command: vec![],
            env: vec![],
            previewable: true,
        };
    }

    Plan { kind: ProjectKind::Unknown, description: "No build recipe for this project".into(), command: vec![], env: vec![], previewable: false }
}

/// Built APKs below the project, newest-first is left to the caller.
pub fn find_apks(root: &Path) -> Vec<String> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<String>, depth: u32) {
        if depth > 8 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if p.is_dir() {
                if name != ".git" && name != "node_modules" {
                    walk(&p, root, out, depth + 1);
                }
            } else if name.ends_with(".apk") && p.to_string_lossy().contains("outputs/apk") {
                out.push(p.strip_prefix(root).unwrap_or(&p).to_string_lossy().into_owned());
            }
        }
    }
    let mut v = Vec::new();
    walk(root, root, &mut v, 0);
    v.sort();
    v
}

// ---- diagnostics ----------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Diagnostic {
    /// Relative to the project root when possible.
    pub path: String,
    pub line: u32,
    pub column: u32,
    pub severity: Severity,
    pub message: String,
}

fn rel(root: &str, p: &str) -> String {
    let p = p.trim_start_matches("file://");
    let root = root.trim_end_matches('/');
    match p.strip_prefix(root) {
        Some(r) => r.trim_start_matches('/').to_string(),
        None => p.to_string(),
    }
}

pub fn parse_diagnostics(output: &str, root: &str) -> Vec<Diagnostic> {
    let rustc_head = Regex::new(r"^(error|warning)(?:\[[A-Za-z0-9]+\])?: (.+)$").unwrap();
    let rustc_loc = Regex::new(r"^\s*--> (.+?):(\d+):(\d+)\s*$").unwrap();
    let kotlin = Regex::new(r"^([ew]): (?:file://)?(.+?):(\d+):(\d+)\s+(.+)$").unwrap();
    let gcc = Regex::new(r"^(.+?):(\d+)(?::(\d+))?: (error|warning): (.+)$").unwrap();
    let ansi = Regex::new("\x1b\\[[0-9;]*m").unwrap();

    let mut out = Vec::new();
    let mut pending: Option<(Severity, String)> = None;
    for raw in output.lines() {
        let line = ansi.replace_all(raw, "");
        let line = line.as_ref();
        let sev = |s: &str| if s.starts_with('e') { Severity::Error } else { Severity::Warning };

        if let Some(c) = rustc_head.captures(line) {
            pending = Some((sev(&c[1]), c[2].to_string()));
            continue;
        }
        if let (Some((severity, message)), Some(c)) = (pending.as_ref(), rustc_loc.captures(line)) {
            out.push(Diagnostic {
                path: rel(root, &c[1]),
                line: c[2].parse().unwrap_or(0),
                column: c[3].parse().unwrap_or(0),
                severity: *severity,
                message: message.clone(),
            });
            pending = None;
            continue;
        }
        if let Some(c) = kotlin.captures(line) {
            out.push(Diagnostic {
                path: rel(root, &c[2]),
                line: c[3].parse().unwrap_or(0),
                column: c[4].parse().unwrap_or(0),
                severity: sev(&c[1]),
                message: c[5].to_string(),
            });
            continue;
        }
        if let Some(c) = gcc.captures(line) {
            out.push(Diagnostic {
                path: rel(root, &c[1]),
                line: c[2].parse().unwrap_or(0),
                column: c.get(3).and_then(|m| m.as_str().parse().ok()).unwrap_or(0),
                severity: sev(&c[4]),
                message: c[5].to_string(),
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir_with(files: &[(&str, &str)]) -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        for (p, c) in files {
            let f = d.path().join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(f, c).unwrap();
        }
        d
    }

    #[test]
    fn android_project_uses_system_gradle_and_native_aapt2() {
        let d = dir_with(&[
            ("settings.gradle.kts", "include(\":app\")"),
            ("app/build.gradle.kts", "plugins { id(\"com.android.application\") }"),
        ]);
        let p = detect(d.path(), Some("/data/tools/aapt2"));
        assert_eq!(p.kind, ProjectKind::AndroidGradle);
        assert_eq!(p.command[0], "gradle");
        assert!(p.command.contains(&"assembleDebug".to_string()));
        assert!(p.command.contains(&"-Pandroid.aapt2FromMavenOverride=/data/tools/aapt2".to_string()));
        assert!(!p.command.iter().any(|c| c.contains("gradlew")));
        let without = detect(d.path(), None);
        assert!(!without.command.iter().any(|c| c.contains("aapt2")));
    }

    #[test]
    fn rust_games_and_crates() {
        let d = dir_with(&[("Cargo.toml", "[dependencies]\nmacroquad = \"0.4\""), ("index.html", "<html/>")]);
        let p = detect(d.path(), None);
        assert_eq!(p.kind, ProjectKind::CargoGame);
        assert!(p.command.contains(&"wasm32-unknown-unknown".to_string()));
        assert!(p.previewable);
        let d = dir_with(&[("Cargo.toml", "[dependencies]\nbevy = \"0.16\""), ("build.sh", "echo")]);
        assert_eq!(detect(d.path(), None).command, vec!["sh", "build.sh"]);
        let d = dir_with(&[("Cargo.toml", "[dependencies]\nserde = \"1\"")]);
        assert_eq!(detect(d.path(), None).kind, ProjectKind::Cargo);
    }

    #[test]
    fn web_and_node_and_unknown() {
        let d = dir_with(&[("index.html", "<html/>")]);
        let p = detect(d.path(), None);
        assert_eq!(p.kind, ProjectKind::WebStatic);
        assert!(p.command.is_empty() && p.previewable);
        let d = dir_with(&[("package.json", "{\"scripts\":{\"build\":\"vite build\"}}")]);
        assert_eq!(detect(d.path(), None).command, vec!["npm", "run", "build"]);
        assert_eq!(detect(tempfile::tempdir().unwrap().path(), None).kind, ProjectKind::Unknown);
    }

    #[test]
    fn finds_apks() {
        let d = dir_with(&[("app/build/outputs/apk/debug/app-debug.apk", ""), ("other/x.apk", "")]);
        assert_eq!(find_apks(d.path()), vec!["app/build/outputs/apk/debug/app-debug.apk"]);
    }

    #[test]
    fn parses_rustc_errors() {
        let out = "error[E0425]: cannot find value `x` in this scope\n  --> src/main.rs:5:9\n   |\n5  |     x\n\nwarning: unused variable: `y`\n --> /proj/src/lib.rs:2:5\n";
        let d = parse_diagnostics(out, "/proj");
        assert_eq!(d.len(), 2);
        assert_eq!(d[0], Diagnostic { path: "src/main.rs".into(), line: 5, column: 9, severity: Severity::Error, message: "cannot find value `x` in this scope".into() });
        assert_eq!(d[1].path, "src/lib.rs");
        assert_eq!(d[1].severity, Severity::Warning);
    }

    #[test]
    fn parses_kotlin_javac_and_gcc() {
        let out = "e: file:///proj/app/src/Main.kt:12:5 Unresolved reference 'foo'\nw: file:///proj/app/src/Main.kt:3:1 Unused import\n/proj/src/A.java:7: error: ';' expected\nmain.c:10:3: warning: unused variable 'z'\n";
        let d = parse_diagnostics(out, "/proj");
        assert_eq!(d.len(), 4);
        assert_eq!((d[0].path.as_str(), d[0].line, d[0].column, d[0].severity), ("app/src/Main.kt", 12, 5, Severity::Error));
        assert_eq!(d[1].severity, Severity::Warning);
        assert_eq!((d[2].path.as_str(), d[2].line, d[2].column), ("src/A.java", 7, 0));
        assert_eq!(d[3].message, "unused variable 'z'");
    }

    #[test]
    fn ignores_noise_and_ansi() {
        let out = "\u{1b}[1m\u{1b}[31merror\u{1b}[0m: aborting due to previous error\nBUILD FAILED in 3s\n";
        assert!(parse_diagnostics(out, "/p").is_empty(), "rustc summary has no location");
        let out = "\u{1b}[31mmain.c:1:1: error: bad\u{1b}[0m";
        assert_eq!(parse_diagnostics(out, "/p").len(), 1);
    }
}
