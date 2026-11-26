//! Dev-IDE core. Everything the Kotlin UI needs is exposed through the
//! `Workspace` object below via UniFFI (no hand-written JNI).

mod buildplan;
mod editor;
mod git;
mod highlight;
mod lang;
mod pkg;
mod project;
mod screen;
mod templates;
mod terminal;
mod webserver;

use editor::{Buffer, Move};
pub use buildplan::{Diagnostic, ProjectKind, Severity};
pub use git::{ChangeKind, CommitInfo, GitChange};
pub use highlight::{TokenKind, TokenSpan};
pub use lang::Language;
use std::path::PathBuf;
use std::sync::Mutex;

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum IdeError {
    #[error("io error: {0}")]
    Io(String),
    #[error("path escapes the project root: {0}")]
    OutsideRoot(String),
    #[error("no such tab: {0}")]
    NoSuchTab(u32),
    #[error("already exists: {0}")]
    AlreadyExists(String),
    #[error("terminal error: {0}")]
    Terminal(String),
    #[error("git: {0}")]
    Git(String),
    #[error("package: {0}")]
    Package(String),
    #[error("{0}")]
    Language(String),
}

impl From<std::io::Error> for IdeError {
    fn from(e: std::io::Error) -> Self {
        IdeError::Io(e.to_string())
    }
}

#[derive(uniffi::Record, Clone, Debug)]
pub struct FileEntry {
    pub name: String,
    /// Path relative to the project root, using '/' separators.
    pub path: String,
    pub is_dir: bool,
}

#[derive(uniffi::Record, Clone, Debug)]
pub struct TabInfo {
    pub path: String,
    pub name: String,
    pub dirty: bool,
}

#[derive(uniffi::Record, Clone, Debug)]
pub struct CursorInfo {
    pub line: u32,
    pub column: u32,
    /// Char offsets; equal when there is no selection.
    pub sel_start: u32,
    pub sel_end: u32,
    pub offset: u32,
}

#[derive(uniffi::Enum, Clone, Copy, Debug)]
pub enum CursorMove {
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

impl From<CursorMove> for Move {
    fn from(m: CursorMove) -> Self {
        match m {
            CursorMove::Left => Move::Left,
            CursorMove::Right => Move::Right,
            CursorMove::Up => Move::Up,
            CursorMove::Down => Move::Down,
            CursorMove::WordLeft => Move::WordLeft,
            CursorMove::WordRight => Move::WordRight,
            CursorMove::LineStart => Move::LineStart,
            CursorMove::LineEnd => Move::LineEnd,
            CursorMove::DocStart => Move::DocStart,
            CursorMove::DocEnd => Move::DocEnd,
        }
    }
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectUnit {
    Word,
    Line,
    Document,
}

struct Tab {
    path: String,
    buffer: Buffer,
}

struct State {
    root: PathBuf,
    tabs: Vec<Tab>,
}

#[derive(uniffi::Object)]
pub struct Workspace {
    state: Mutex<State>,
}

impl Workspace {
    fn with_tab<R>(&self, tab: u32, f: impl FnOnce(&mut Buffer) -> R) -> Result<R, IdeError> {
        let mut st = self.state.lock().unwrap();
        let t = st
            .tabs
            .get_mut(tab as usize)
            .ok_or(IdeError::NoSuchTab(tab))?;
        Ok(f(&mut t.buffer))
    }
}

#[uniffi::export]
impl Workspace {
    /// `root` is the project directory, which must live in the app's private
    /// storage (filesDir) so build tools and git can execute there.
    #[uniffi::constructor]
    pub fn new(root: String) -> Result<Self, IdeError> {
        let root = PathBuf::from(root);
        std::fs::create_dir_all(&root)?;
        Ok(Workspace {
            state: Mutex::new(State { root, tabs: Vec::new() }),
        })
    }

    // ---- files ---------------------------------------------------------

    pub fn list_dir(&self, rel: String) -> Result<Vec<FileEntry>, IdeError> {
        let root = self.state.lock().unwrap().root.clone();
        project::list_dir(&root, &rel)
    }

    pub fn create_file(&self, rel: String) -> Result<(), IdeError> {
        let root = self.state.lock().unwrap().root.clone();
        project::create_file(&root, &rel)
    }

    pub fn create_dir(&self, rel: String) -> Result<(), IdeError> {
        let root = self.state.lock().unwrap().root.clone();
        project::create_dir(&root, &rel)
    }

    pub fn delete_path(&self, rel: String) -> Result<(), IdeError> {
        let root = self.state.lock().unwrap().root.clone();
        project::delete_path(&root, &rel)
    }

    pub fn rename_path(&self, from: String, to: String) -> Result<(), IdeError> {
        let root = self.state.lock().unwrap().root.clone();
        project::rename_path(&root, &from, &to)
    }

    // ---- tabs ----------------------------------------------------------

    /// Opens a file (or focuses it if already open) and returns its tab index.
    pub fn open_file(&self, rel: String) -> Result<u32, IdeError> {
        let mut st = self.state.lock().unwrap();
        if let Some(i) = st.tabs.iter().position(|t| t.path == rel) {
            return Ok(i as u32);
        }
        let text = project::read_file(&st.root, &rel)?;
        st.tabs.push(Tab { path: rel, buffer: Buffer::new(&text) });
        Ok((st.tabs.len() - 1) as u32)
    }

    pub fn close_tab(&self, tab: u32) -> Result<(), IdeError> {
        let mut st = self.state.lock().unwrap();
        if (tab as usize) >= st.tabs.len() {
            return Err(IdeError::NoSuchTab(tab));
        }
        st.tabs.remove(tab as usize);
        Ok(())
    }

    pub fn tabs(&self) -> Vec<TabInfo> {
        let st = self.state.lock().unwrap();
        st.tabs
            .iter()
            .map(|t| TabInfo {
                path: t.path.clone(),
                name: t.path.rsplit('/').next().unwrap_or(&t.path).to_string(),
                dirty: t.buffer.is_dirty(),
            })
            .collect()
    }

    /// Autosave: writes every dirty tab. Returns how many files were written.
    pub fn save_all(&self) -> Result<u32, IdeError> {
        let mut st = self.state.lock().unwrap();
        let root = st.root.clone();
        let mut n = 0;
        for t in st.tabs.iter_mut().filter(|t| t.buffer.is_dirty()) {
            project::write_file(&root, &t.path, &t.buffer.text())?;
            t.buffer.mark_saved();
            n += 1;
        }
        Ok(n)
    }

    pub fn save_tab(&self, tab: u32) -> Result<(), IdeError> {
        let mut st = self.state.lock().unwrap();
        let root = st.root.clone();
        let t = st.tabs.get_mut(tab as usize).ok_or(IdeError::NoSuchTab(tab))?;
        project::write_file(&root, &t.path, &t.buffer.text())?;
        t.buffer.mark_saved();
        Ok(())
    }

    // ---- reading a buffer ---------------------------------------------

    pub fn text(&self, tab: u32) -> Result<String, IdeError> {
        self.with_tab(tab, |b| b.text())
    }

    pub fn line_count(&self, tab: u32) -> Result<u32, IdeError> {
        self.with_tab(tab, |b| b.line_count() as u32)
    }

    pub fn line(&self, tab: u32, index: u32) -> Result<String, IdeError> {
        self.with_tab(tab, |b| b.line(index as usize))
    }

    pub fn cursor(&self, tab: u32) -> Result<CursorInfo, IdeError> {
        self.with_tab(tab, |b| {
            let (line, column) = b.cursor_position();
            let (s, e) = b.selection().unwrap_or((b.cursor(), b.cursor()));
            CursorInfo {
                line: line as u32,
                column: column as u32,
                sel_start: s as u32,
                sel_end: e as u32,
                offset: b.cursor() as u32,
            }
        })
    }

    pub fn selected_text(&self, tab: u32) -> Result<String, IdeError> {
        self.with_tab(tab, |b| b.selected_text())
    }

    pub fn can_undo(&self, tab: u32) -> Result<bool, IdeError> {
        self.with_tab(tab, |b| b.can_undo())
    }

    // ---- editing -------------------------------------------------------

    pub fn insert(&self, tab: u32, text: String) -> Result<(), IdeError> {
        self.with_tab(tab, |b| b.insert(&text))
    }

    /// Single typed character with auto-pairing / step-over behaviour.
    pub fn type_char(&self, tab: u32, ch: String) -> Result<(), IdeError> {
        self.with_tab(tab, |b| {
            for c in ch.chars() {
                b.type_char(c)
            }
        })
    }

    /// Extra-keys row: "{ }" inserts both and parks the cursor between.
    pub fn insert_pair(&self, tab: u32, open: String, close: String) -> Result<(), IdeError> {
        let o = open.chars().next().unwrap_or('(');
        let c = close.chars().next().unwrap_or(')');
        self.with_tab(tab, |b| b.insert_pair(o, c))
    }

    pub fn newline(&self, tab: u32) -> Result<(), IdeError> {
        self.with_tab(tab, |b| b.newline())
    }

    pub fn backspace(&self, tab: u32) -> Result<(), IdeError> {
        self.with_tab(tab, |b| b.delete_backward())
    }

    pub fn delete_forward(&self, tab: u32) -> Result<(), IdeError> {
        self.with_tab(tab, |b| b.delete_forward())
    }

    pub fn undo(&self, tab: u32) -> Result<bool, IdeError> {
        self.with_tab(tab, |b| b.undo())
    }

    pub fn redo(&self, tab: u32) -> Result<bool, IdeError> {
        self.with_tab(tab, |b| b.redo())
    }

    pub fn duplicate_line(&self, tab: u32) -> Result<(), IdeError> {
        self.with_tab(tab, |b| b.duplicate_line())
    }

    pub fn toggle_comment(&self, tab: u32, prefix: String) -> Result<(), IdeError> {
        self.with_tab(tab, |b| b.toggle_comment(&prefix))
    }

    pub fn cut(&self, tab: u32) -> Result<String, IdeError> {
        self.with_tab(tab, |b| b.cut())
    }

    // ---- cursor / selection (cursor editor + trackpad) ----------------

    pub fn set_cursor(&self, tab: u32, offset: u32, extend: bool) -> Result<(), IdeError> {
        self.with_tab(tab, |b| b.set_cursor(offset as usize, extend))
    }

    pub fn move_cursor(&self, tab: u32, to: CursorMove, extend: bool) -> Result<(), IdeError> {
        self.with_tab(tab, |b| b.move_cursor(to.into(), extend))
    }

    /// Trackpad drag: moves by whole characters/lines, optionally selecting.
    pub fn nudge(&self, tab: u32, dx: i32, dy: i32, extend: bool) -> Result<(), IdeError> {
        self.with_tab(tab, |b| {
            for _ in 0..dy.unsigned_abs() {
                b.move_cursor(if dy < 0 { Move::Up } else { Move::Down }, extend);
            }
            for _ in 0..dx.unsigned_abs() {
                b.move_cursor(if dx < 0 { Move::Left } else { Move::Right }, extend);
            }
        })
    }

    pub fn select(&self, tab: u32, unit: SelectUnit) -> Result<(), IdeError> {
        self.with_tab(tab, |b| match unit {
            SelectUnit::Word => b.select_word(),
            SelectUnit::Line => b.select_line(),
            SelectUnit::Document => b.select_all(),
        })
    }

    // ---- highlighting --------------------------------------------------

    /// Syntax spans for the tab (empty when the file type has no grammar).
    pub fn highlight(&self, tab: u32) -> Result<Vec<TokenSpan>, IdeError> {
        let mut st = self.state.lock().unwrap();
        let t = st.tabs.get_mut(tab as usize).ok_or(IdeError::NoSuchTab(tab))?;
        let name = t.path.rsplit('/').next().unwrap_or(&t.path).to_string();
        Ok(match highlight::language_for(&name) {
            Some(lang) => highlight::highlight(lang, &t.buffer.text()),
            None => Vec::new(),
        })
    }

    // ---- search --------------------------------------------------------

    /// Returns match ranges as flat [start0, end0, start1, end1, ...] char offsets.
    pub fn find_all(&self, tab: u32, needle: String, case_sensitive: bool) -> Result<Vec<u32>, IdeError> {
        self.with_tab(tab, |b| {
            b.find_all(&needle, case_sensitive)
                .into_iter()
                .flat_map(|(s, e)| [s as u32, e as u32])
                .collect()
        })
    }

    pub fn replace_all(
        &self,
        tab: u32,
        needle: String,
        with: String,
        case_sensitive: bool,
    ) -> Result<u32, IdeError> {
        self.with_tab(tab, |b| b.replace_all(&needle, &with, case_sensitive) as u32)
    }
}

// ---- build plans, diagnostics, templates --------------------------------------

#[derive(uniffi::Record, Clone, Debug)]
pub struct BuildPlan {
    pub kind: ProjectKind,
    pub description: String,
    /// Empty when there is nothing to build (static web).
    pub command: Vec<String>,
    pub env: Vec<String>,
    pub previewable: bool,
}

/// How to build `root`. `aapt2` is the path of a native aarch64 aapt2 ("" if unknown).
#[uniffi::export]
pub fn detect_build(root: String, aapt2: String) -> BuildPlan {
    let p = buildplan::detect(std::path::Path::new(&root), Some(aapt2.as_str()));
    BuildPlan { kind: p.kind, description: p.description, command: p.command, env: p.env, previewable: p.previewable }
}

#[uniffi::export]
pub fn find_apks(root: String) -> Vec<String> {
    buildplan::find_apks(std::path::Path::new(&root))
}

/// Compiler/build output -> structured errors and warnings (rustc, kotlinc, javac, gcc/clang).
#[uniffi::export]
pub fn parse_diagnostics(output: String, root: String) -> Vec<Diagnostic> {
    buildplan::parse_diagnostics(&output, &root)
}

#[derive(uniffi::Record, Clone, Debug)]
pub struct TemplateInfo {
    pub id: String,
    pub name: String,
    pub description: String,
}

#[uniffi::export]
pub fn project_templates() -> Vec<TemplateInfo> {
    templates::ALL
        .iter()
        .map(|t| TemplateInfo { id: t.id.into(), name: t.name.into(), description: t.description.into() })
        .collect()
}

/// Creates `<root>/<name>` from a template; returns the project folder relative to `root`.
#[uniffi::export]
pub fn create_from_template(root: String, template_id: String, name: String) -> Result<String, IdeError> {
    templates::create(std::path::Path::new(&root), &template_id, &name)
}

// ---- web preview ------------------------------------------------------------

#[derive(uniffi::Record, Clone, Debug)]
pub struct PreviewPlan {
    /// Entry page relative to the project root; empty when the project has no HTML page.
    pub entry: String,
    /// Needs cross-origin isolation: open in Chrome Custom Tabs, not WebView.
    pub isolated: bool,
    pub reason: String,
}

/// Decide how a web project must be previewed.
#[uniffi::export]
pub fn analyze_preview(root: String) -> PreviewPlan {
    let p = webserver::analyze(std::path::Path::new(&root));
    PreviewPlan { entry: p.entry, isolated: p.isolated, reason: p.reason }
}

/// Loopback-only static server with live reload. Hosted by the foreground service.
#[derive(uniffi::Object)]
pub struct WebServer {
    inner: Mutex<Option<webserver::Server>>,
}

#[uniffi::export]
impl WebServer {
    #[uniffi::constructor]
    pub fn new() -> Self {
        WebServer { inner: Mutex::new(None) }
    }

    /// Starts (or restarts) serving `root`. `port` 0 picks a free port. Returns the port.
    pub fn start(&self, root: String, port: u32, isolated: bool, live_reload: bool) -> Result<u32, IdeError> {
        let mut g = self.inner.lock().unwrap();
        if let Some(mut old) = g.take() {
            old.stop();
        }
        let s = webserver::Server::start(std::path::Path::new(&root), port as u16, isolated, live_reload)
            .map_err(|e| IdeError::Io(e.to_string()))?;
        let p = s.port() as u32;
        *g = Some(s);
        Ok(p)
    }

    pub fn stop(&self) {
        if let Some(mut s) = self.inner.lock().unwrap().take() {
            s.stop();
        }
    }

    /// 0 when not running.
    pub fn port(&self) -> u32 {
        self.inner.lock().unwrap().as_ref().map(|s| s.port() as u32).unwrap_or(0)
    }
}

// ---- packages -------------------------------------------------------------

#[derive(uniffi::Record, Clone, Debug)]
pub struct PackageInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub url: String,
    pub size: u64,
    pub installed: bool,
    pub installed_version: Option<String>,
    /// The archive is already on disk, so (re)installing works offline.
    pub cached: bool,
}

/// Language/tool packages stored under `root` (app-private storage).
#[derive(uniffi::Object)]
pub struct PackageManager {
    store: pkg::Store,
}

#[uniffi::export]
impl PackageManager {
    #[uniffi::constructor]
    pub fn new(root: String) -> Result<Self, IdeError> {
        Ok(PackageManager { store: pkg::Store::new(std::path::Path::new(&root))? })
    }

    /// Replace the catalog (validated first). Returns the number of packages.
    pub fn set_catalog(&self, json: String) -> Result<u32, IdeError> {
        Ok(self.store.save_catalog(&json)? as u32)
    }

    pub fn packages(&self) -> Result<Vec<PackageInfo>, IdeError> {
        let installed = self.store.installed();
        Ok(self
            .store
            .load_catalog()?
            .into_iter()
            .map(|e| {
                let inst = installed.iter().find(|i| i.id == e.id);
                PackageInfo {
                    cached: self.store.archive_path(&e).exists(),
                    installed: inst.is_some(),
                    installed_version: inst.map(|i| i.version.clone()),
                    id: e.id,
                    name: e.name,
                    version: e.version,
                    description: e.description,
                    url: e.url,
                    size: e.size,
                }
            })
            .collect())
    }

    pub fn download_url(&self, id: String) -> Result<String, IdeError> {
        Ok(self.store.find(&id)?.url)
    }

    /// Where the platform layer should save the downloaded archive.
    pub fn archive_path(&self, id: String) -> Result<String, IdeError> {
        Ok(self.store.archive_path(&self.store.find(&id)?).to_string_lossy().into_owned())
    }

    pub fn install(&self, id: String) -> Result<(), IdeError> {
        self.store.install(&id)
    }

    pub fn uninstall(&self, id: String) -> Result<(), IdeError> {
        self.store.uninstall(&id)
    }

    pub fn remove_cache(&self, id: String) -> Result<(), IdeError> {
        self.store.remove_cache(&id)
    }

    /// Directories to prepend to PATH for every installed package.
    pub fn path_dirs(&self) -> Vec<String> {
        self.store.path_dirs()
    }

    /// Extra "KEY=VALUE" environment entries from installed packages.
    pub fn extra_env(&self) -> Vec<String> {
        self.store.extra_env()
    }
}

// ---- git ------------------------------------------------------------------

/// Git for one project directory. `git_bin` is the path of the git executable
/// (installed through the package manager); reads use gitoxide and do not need it.
#[derive(uniffi::Object)]
pub struct Repo {
    dir: PathBuf,
    git_bin: String,
}

impl Repo {
    fn cli(&self, token: Option<String>) -> git::Git {
        git::Git { dir: self.dir.clone(), bin: self.git_bin.clone(), token }
    }
}

#[uniffi::export]
impl Repo {
    #[uniffi::constructor]
    pub fn new(dir: String, git_bin: String) -> Self {
        Repo { dir: PathBuf::from(dir), git_bin }
    }

    pub fn is_repo(&self) -> bool {
        git::is_repo(&self.dir)
    }
    pub fn init(&self) -> Result<(), IdeError> {
        self.cli(None).init()
    }
    pub fn branch(&self) -> Result<Option<String>, IdeError> {
        git::branch(&self.dir)
    }
    pub fn status(&self) -> Result<Vec<GitChange>, IdeError> {
        git::status(&self.dir)
    }
    pub fn log(&self, limit: u32) -> Result<Vec<CommitInfo>, IdeError> {
        git::log(&self.dir, limit as usize)
    }
    pub fn stage(&self, paths: Vec<String>) -> Result<(), IdeError> {
        self.cli(None).stage(&paths)
    }
    pub fn stage_all(&self) -> Result<(), IdeError> {
        self.cli(None).stage_all()
    }
    pub fn unstage(&self, paths: Vec<String>) -> Result<(), IdeError> {
        self.cli(None).unstage(&paths)
    }
    pub fn commit(&self, message: String, name: String, email: String) -> Result<String, IdeError> {
        self.cli(None).commit(&message, &name, &email)
    }
    pub fn push(&self, token: String) -> Result<String, IdeError> {
        self.cli(Some(token)).push()
    }
    pub fn pull(&self, token: String) -> Result<String, IdeError> {
        self.cli(Some(token)).pull()
    }
    pub fn checkout(&self, branch: String, create: bool) -> Result<(), IdeError> {
        self.cli(None).checkout(&branch, create)
    }
    pub fn add_remote(&self, url: String) -> Result<(), IdeError> {
        self.cli(None).add_remote(&url)
    }
    pub fn diff(&self, path: String, staged: bool) -> Result<String, IdeError> {
        self.cli(None).diff(&path, staged)
    }
}

/// Clone `url` into `dest` (created). Used for "Clone repository".
#[uniffi::export]
pub fn clone_repository(url: String, dest: String, git_bin: String, token: String) -> Result<(), IdeError> {
    git::clone_repo(&url, std::path::Path::new(&dest), &git_bin, Some(token))
}

// ---- terminal -----------------------------------------------------------

#[derive(uniffi::Record, Clone, Debug)]
pub struct TermCursor {
    pub line: u32,
    pub col: u32,
}

/// A real pseudo-terminal running a shell. `env` entries are "KEY=VALUE".
#[derive(uniffi::Object)]
pub struct Terminal {
    session: terminal::Session,
}

#[uniffi::export]
impl Terminal {
    #[uniffi::constructor]
    pub fn new(
        program: String,
        args: Vec<String>,
        cwd: String,
        env: Vec<String>,
        cols: u32,
        rows: u32,
    ) -> Result<Self, IdeError> {
        let env: Vec<(String, String)> = env
            .iter()
            .filter_map(|e| e.split_once('=').map(|(k, v)| (k.to_string(), v.to_string())))
            .collect();
        let session = terminal::Session::spawn(&program, &args, &cwd, &env, cols as u16, rows as u16)
            .map_err(IdeError::Terminal)?;
        Ok(Terminal { session })
    }

    /// Raw input, e.g. "ls\r", "\u{3}" (Ctrl+C), "\u{1b}[A" (Up).
    pub fn send(&self, data: String) -> Result<(), IdeError> {
        self.session.write(data.as_bytes()).map_err(IdeError::Terminal)
    }

    pub fn resize(&self, cols: u32, rows: u32) -> Result<(), IdeError> {
        self.session.resize(cols as u16, rows as u16).map_err(IdeError::Terminal)
    }

    /// Increases whenever output changes; poll this cheaply before calling `lines`.
    pub fn version(&self) -> u64 {
        self.session.version()
    }

    pub fn lines(&self) -> Vec<String> {
        self.session.lines()
    }

    pub fn cursor(&self) -> TermCursor {
        let (l, c) = self.session.cursor();
        TermCursor { line: l as u32, col: c as u32 }
    }

    pub fn is_alive(&self) -> bool {
        self.session.is_alive()
    }

    pub fn kill(&self) {
        self.session.kill()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws() -> (tempfile::TempDir, Workspace) {
        let dir = tempfile::tempdir().unwrap();
        let w = Workspace::new(dir.path().to_string_lossy().into()).unwrap();
        (dir, w)
    }

    #[test]
    fn open_edit_autosave_roundtrip() {
        let (dir, w) = ws();
        w.create_file("app.js".into()).unwrap();
        let t = w.open_file("app.js".into()).unwrap();
        w.insert(t, "const a = 1".into()).unwrap();
        assert!(w.tabs()[0].dirty);
        assert_eq!(w.save_all().unwrap(), 1);
        assert!(!w.tabs()[0].dirty);
        assert_eq!(std::fs::read_to_string(dir.path().join("app.js")).unwrap(), "const a = 1");
        assert_eq!(w.save_all().unwrap(), 0);
    }

    #[test]
    fn opening_same_file_reuses_tab() {
        let (_d, w) = ws();
        w.create_file("a.txt".into()).unwrap();
        let a = w.open_file("a.txt".into()).unwrap();
        let b = w.open_file("a.txt".into()).unwrap();
        assert_eq!(a, b);
        assert_eq!(w.tabs().len(), 1);
    }

    #[test]
    fn bad_tab_is_an_error() {
        let (_d, w) = ws();
        assert!(matches!(w.text(3), Err(IdeError::NoSuchTab(3))));
    }

    #[test]
    fn selection_and_trackpad() {
        let (_d, w) = ws();
        w.create_file("a.txt".into()).unwrap();
        let t = w.open_file("a.txt".into()).unwrap();
        w.insert(t, "hello world".into()).unwrap();
        w.set_cursor(t, 0, false).unwrap();
        w.nudge(t, 5, 0, true).unwrap();
        assert_eq!(w.selected_text(t).unwrap(), "hello");
        let c = w.cursor(t).unwrap();
        assert_eq!((c.sel_start, c.sel_end), (0, 5));
    }

    #[test]
    fn project_paths_cannot_escape_root() {
        let (_d, w) = ws();
        assert!(matches!(w.create_file("../evil".into()), Err(IdeError::OutsideRoot(_))));
        assert!(matches!(w.open_file("/etc/passwd".into()), Err(IdeError::OutsideRoot(_))));
    }

    #[test]
    fn list_dir_sorts_dirs_first() {
        let (_d, w) = ws();
        w.create_file("z.txt".into()).unwrap();
        w.create_dir("src".into()).unwrap();
        w.create_file("src/main.rs".into()).unwrap();
        let top = w.list_dir("".into()).unwrap();
        assert_eq!(top[0].name, "src");
        assert!(top[0].is_dir);
        assert_eq!(top[1].name, "z.txt");
        let sub = w.list_dir("src".into()).unwrap();
        assert_eq!(sub[0].path, "src/main.rs");
    }
}

// ---- languages ------------------------------------------------------------

/// Built-in languages merged with the user's JSON array (see `lang.rs`).
#[uniffi::export]
pub fn languages(user_json: String) -> Result<Vec<Language>, IdeError> {
    lang::merged(&user_json)
}

/// Validate a user language list before saving it; returns how many entries it holds.
#[uniffi::export]
pub fn validate_languages(user_json: String) -> Result<u32, IdeError> {
    Ok(lang::parse_user(&user_json)?.len() as u32)
}

#[uniffi::export]
pub fn language_for_file(user_json: String, path: String) -> Result<Option<Language>, IdeError> {
    let all = lang::merged(&user_json)?;
    Ok(lang::for_file(&all, &path).cloned())
}

/// Shell command line for running `path` with `language` (placeholders substituted and quoted).
#[uniffi::export]
pub fn render_run(language: Language, path: String) -> String {
    lang::render(&language.run, &path)
}

#[uniffi::export]
pub fn missing_packages(language: Language, installed: Vec<String>) -> Vec<String> {
    lang::missing(&language, &installed)
}
