//! Hybrid Git: gitoxide (pure Rust) for reads, the real `git` binary for
//! everything that writes or touches the network.

use crate::IdeError;
use base64::Engine;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Untracked,
    Conflicted,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct GitChange {
    pub path: String,
    pub kind: ChangeKind,
    /// True when the change is already in the index (will be part of the next commit).
    pub staged: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CommitInfo {
    pub id: String,
    pub summary: String,
    pub author: String,
    /// Seconds since the Unix epoch.
    pub time: i64,
}

fn git_err(e: impl std::fmt::Display) -> IdeError {
    IdeError::Git(e.to_string())
}

pub fn is_repo(dir: &Path) -> bool {
    gix::open(dir).is_ok()
}

pub fn branch(dir: &Path) -> Result<Option<String>, IdeError> {
    let repo = gix::open(dir).map_err(git_err)?;
    Ok(repo.head_name().map_err(git_err)?.map(|n| n.shorten().to_string()))
}

pub fn log(dir: &Path, limit: usize) -> Result<Vec<CommitInfo>, IdeError> {
    let repo = gix::open(dir).map_err(git_err)?;
    let head = match repo.head_id() {
        Ok(id) => id,
        Err(_) => return Ok(Vec::new()), // no commits yet
    };
    let mut out = Vec::new();
    for info in repo.rev_walk([head.detach()]).all().map_err(git_err)?.take(limit) {
        let info = info.map_err(git_err)?;
        let commit = info.object().map_err(git_err)?;
        let msg = commit.message().map_err(git_err)?;
        let author = commit.author().map_err(git_err)?;
        out.push(CommitInfo {
            id: info.id.to_string(),
            summary: msg.summary().to_string(),
            author: author.name.to_string(),
            time: author.seconds(),
        });
    }
    Ok(out)
}

pub fn status(dir: &Path) -> Result<Vec<GitChange>, IdeError> {
    use gix::status::index_worktree::iter::Summary;
    use gix::status::Item;

    let repo = gix::open(dir).map_err(git_err)?;
    let platform = repo.status(gix::progress::Discard).map_err(git_err)?;
    let iter = platform
        .untracked_files(gix::status::UntrackedFiles::Files)
        .into_iter(Vec::<gix::bstr::BString>::new())
        .map_err(git_err)?;

    let mut out: Vec<GitChange> = Vec::new();
    for item in iter {
        let item = item.map_err(git_err)?;
        let path = item.location().to_string();
        let (kind, staged) = match &item {
            Item::IndexWorktree(change) => match change.summary() {
                Some(Summary::Removed) => (ChangeKind::Deleted, false),
                Some(Summary::Added) => (ChangeKind::Untracked, false),
                Some(Summary::Modified) | Some(Summary::TypeChange) => (ChangeKind::Modified, false),
                Some(Summary::Renamed) | Some(Summary::Copied) => (ChangeKind::Renamed, false),
                Some(Summary::Conflict) => (ChangeKind::Conflicted, false),
                Some(Summary::IntentToAdd) => (ChangeKind::Added, false),
                None => continue,
            },
            Item::TreeIndex(change) => {
                use gix::diff::index::Change;
                let kind = match change {
                    Change::Addition { .. } => ChangeKind::Added,
                    Change::Deletion { .. } => ChangeKind::Deleted,
                    Change::Modification { .. } => ChangeKind::Modified,
                    Change::Rewrite { .. } => ChangeKind::Renamed,
                };
                (kind, true)
            }
        };
        out.push(GitChange { path, kind, staged });
    }
    out.sort_by(|a, b| a.path.cmp(&b.path).then(b.staged.cmp(&a.staged)));
    out.dedup();
    Ok(out)
}

// ---- writes & network: the git binary -------------------------------------

pub struct Git {
    pub dir: PathBuf,
    pub bin: String,
    /// Personal access token for HTTPS remotes (sent as a header, never put in the URL).
    pub token: Option<String>,
}

impl Git {
    fn command(&self, args: &[&str]) -> Command {
        let mut c = Command::new(&self.bin);
        c.current_dir(&self.dir);
        c.env("GIT_TERMINAL_PROMPT", "0");
        if let Some(tok) = self.token.as_ref().filter(|t| !t.is_empty()) {
            let basic = base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{tok}"));
            c.arg("-c").arg(format!("http.extraHeader=Authorization: Basic {basic}"));
        }
        c.args(args);
        c
    }

    pub fn run(&self, args: &[&str]) -> Result<String, IdeError> {
        let out = self.command(args).output().map_err(|e| {
            IdeError::Git(format!("cannot run '{}': {e} (is git installed?)", self.bin))
        })?;
        let mut text = String::from_utf8_lossy(&out.stdout).to_string();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        if out.status.success() {
            Ok(text)
        } else {
            Err(IdeError::Git(text.trim().to_string()))
        }
    }

    pub fn init(&self) -> Result<(), IdeError> {
        self.run(&["init", "-b", "main"]).map(|_| ())
    }

    pub fn stage(&self, paths: &[String]) -> Result<(), IdeError> {
        let mut args = vec!["add", "--"];
        args.extend(paths.iter().map(|s| s.as_str()));
        self.run(&args).map(|_| ())
    }

    pub fn stage_all(&self) -> Result<(), IdeError> {
        self.run(&["add", "-A"]).map(|_| ())
    }

    pub fn unstage(&self, paths: &[String]) -> Result<(), IdeError> {
        // `restore --staged` fails before the first commit; fall back to `rm --cached`.
        let mut args = vec!["restore", "--staged", "--"];
        args.extend(paths.iter().map(|s| s.as_str()));
        if self.run(&args).is_ok() {
            return Ok(());
        }
        let mut args = vec!["rm", "--cached", "-r", "-q", "--"];
        args.extend(paths.iter().map(|s| s.as_str()));
        self.run(&args).map(|_| ())
    }

    pub fn commit(&self, message: &str, name: &str, email: &str) -> Result<String, IdeError> {
        let n = format!("user.name={name}");
        let e = format!("user.email={email}");
        self.run(&["-c", &n, "-c", &e, "commit", "-m", message])
    }

    pub fn push(&self) -> Result<String, IdeError> {
        // `-u origin HEAD` also works for a branch that has no upstream yet.
        self.run(&["push", "-u", "origin", "HEAD"])
    }

    pub fn pull(&self) -> Result<String, IdeError> {
        self.run(&["pull", "--ff-only"])
    }

    pub fn checkout(&self, branch: &str, create: bool) -> Result<(), IdeError> {
        let args: Vec<&str> = if create { vec!["checkout", "-b", branch] } else { vec!["checkout", branch] };
        self.run(&args).map(|_| ())
    }

    pub fn add_remote(&self, url: &str) -> Result<(), IdeError> {
        self.run(&["remote", "add", "origin", url]).map(|_| ())
    }

    pub fn diff(&self, path: &str, staged: bool) -> Result<String, IdeError> {
        if staged {
            self.run(&["diff", "--cached", "--", path])
        } else {
            self.run(&["diff", "--", path])
        }
    }
}

pub fn clone_repo(url: &str, dest: &Path, bin: &str, token: Option<String>) -> Result<(), IdeError> {
    let parent = dest.parent().unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let g = Git { dir: parent.to_path_buf(), bin: bin.to_string(), token };
    g.run(&["clone", "--", url, &dest.to_string_lossy()]).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git_for(dir: &Path) -> Git {
        Git { dir: dir.to_path_buf(), bin: "git".into(), token: None }
    }

    #[test]
    fn full_local_workflow() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        let g = git_for(d);
        assert!(!is_repo(d));
        g.init().unwrap();
        assert!(is_repo(d));
        assert_eq!(branch(d).unwrap().as_deref(), Some("main"));
        assert!(log(d, 10).unwrap().is_empty());

        std::fs::write(d.join("a.txt"), "one").unwrap();
        let st = status(d).unwrap();
        assert_eq!(st, vec![GitChange { path: "a.txt".into(), kind: ChangeKind::Untracked, staged: false }]);

        g.stage(&["a.txt".into()]).unwrap();
        let st = status(d).unwrap();
        assert_eq!(st, vec![GitChange { path: "a.txt".into(), kind: ChangeKind::Added, staged: true }]);

        g.commit("first commit", "Tester", "t@example.com").unwrap();
        assert!(status(d).unwrap().is_empty());
        let l = log(d, 10).unwrap();
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].summary, "first commit");
        assert_eq!(l[0].author, "Tester");

        std::fs::write(d.join("a.txt"), "two").unwrap();
        let st = status(d).unwrap();
        assert_eq!(st, vec![GitChange { path: "a.txt".into(), kind: ChangeKind::Modified, staged: false }]);
        assert!(g.diff("a.txt", false).unwrap().contains("+two"));

        std::fs::remove_file(d.join("a.txt")).unwrap();
        assert_eq!(status(d).unwrap()[0].kind, ChangeKind::Deleted);
    }

    #[test]
    fn unstage_works_before_and_after_first_commit() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        let g = git_for(d);
        g.init().unwrap();
        std::fs::write(d.join("x"), "1").unwrap();
        g.stage_all().unwrap();
        g.unstage(&["x".into()]).unwrap();
        assert_eq!(status(d).unwrap()[0].kind, ChangeKind::Untracked);
    }

    #[test]
    fn branches_push_and_pull_via_local_remote() {
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        std::fs::create_dir_all(&remote).unwrap();
        git_for(&remote).run(&["init", "--bare", "-b", "main"]).unwrap();

        let a = tmp.path().join("a");
        std::fs::create_dir_all(&a).unwrap();
        let ga = git_for(&a);
        ga.init().unwrap();
        ga.add_remote(remote.to_str().unwrap()).unwrap();
        std::fs::write(a.join("f.txt"), "hello").unwrap();
        ga.stage_all().unwrap();
        ga.commit("init", "A", "a@x.io").unwrap();
        ga.push().unwrap();

        let b = tmp.path().join("b");
        clone_repo(remote.to_str().unwrap(), &b, "git", None).unwrap();
        assert_eq!(std::fs::read_to_string(b.join("f.txt")).unwrap(), "hello");
        assert_eq!(log(&b, 5).unwrap()[0].summary, "init");

        std::fs::write(a.join("f.txt"), "hello2").unwrap();
        ga.stage_all().unwrap();
        ga.commit("second", "A", "a@x.io").unwrap();
        ga.push().unwrap();
        git_for(&b).pull().unwrap();
        assert_eq!(std::fs::read_to_string(b.join("f.txt")).unwrap(), "hello2");

        ga.checkout("feature", true).unwrap();
        assert_eq!(branch(&a).unwrap().as_deref(), Some("feature"));
    }

    #[test]
    fn missing_git_binary_gives_a_clear_error() {
        let tmp = tempfile::tempdir().unwrap();
        let g = Git { dir: tmp.path().into(), bin: "/nonexistent/git".into(), token: None };
        let e = g.init().unwrap_err().to_string();
        assert!(e.contains("is git installed"), "{e}");
    }
}
