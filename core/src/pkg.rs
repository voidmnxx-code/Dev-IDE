//! Offline-capable package manager for language toolchains.
//!
//! Flow: a JSON *catalog* lists packages (url + sha256). The platform layer
//! downloads an archive into `cache/`; this module verifies the checksum,
//! extracts it atomically into `pkgs/<id>/` and records it in `installed/`.
//! Cached archives make reinstalling possible without a network.

use crate::IdeError;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub url: String,
    pub sha256: String,
    /// "tar.gz" (default) or "zip"
    #[serde(default = "default_format")]
    pub format: String,
    /// Leading path components to drop while extracting (like `tar --strip-components`).
    #[serde(default)]
    pub strip: u32,
    /// Directories (relative to the install dir) to put on PATH.
    #[serde(default)]
    pub bin: Vec<String>,
    /// Extra environment variables; `{dir}` expands to the install directory.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub description: String,
    /// Catalog ids that must be installed first (resolved by [`Store::plan`]).
    #[serde(default)]
    pub deps: Vec<String>,
    /// Library packages that are only installed as dependencies; the UI hides them.
    #[serde(default)]
    pub hidden: bool,
    /// Extract into the shared prefix (Termux-style layout) instead of a private folder.
    #[serde(default)]
    pub prefix: bool,
}

fn default_format() -> String {
    "tar.gz".into()
}

#[derive(Deserialize)]
struct Catalog {
    packages: Vec<Entry>,
}

pub fn parse_catalog(json: &str) -> Result<Vec<Entry>, IdeError> {
    let c: Catalog = serde_json::from_str(json).map_err(|e| IdeError::Package(format!("bad catalog: {e}")))?;
    for e in &c.packages {
        validate_id(&e.id)?;
        if !matches!(e.format.as_str(), "tar.gz" | "zip") {
            return Err(IdeError::Package(format!("{}: unsupported format '{}'", e.id, e.format)));
        }
        if e.sha256.len() != 64 || !e.sha256.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(IdeError::Package(format!("{}: sha256 must be 64 hex characters", e.id)));
        }
    }
    Ok(c.packages)
}

fn validate_id(id: &str) -> Result<(), IdeError> {
    let ok = !id.is_empty()
        && id.len() <= 64
        && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        && !id.starts_with('.');
    if ok {
        Ok(())
    } else {
        Err(IdeError::Package(format!("invalid package id '{id}'")))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Installed {
    pub id: String,
    pub version: String,
    #[serde(default)]
    pub prefix: bool,
    pub bin: Vec<String>,
    pub env: BTreeMap<String, String>,
}

pub struct Store {
    root: PathBuf,
    /// Shared install prefix for `prefix: true` packages (binaries are built for this exact path).
    prefix: PathBuf,
}

impl Store {
    pub fn new(root: &Path) -> Result<Store, IdeError> {
        Self::with_prefix(root, &root.join("usr"))
    }

    pub fn with_prefix(root: &Path, prefix: &Path) -> Result<Store, IdeError> {
        for d in ["cache", "pkgs", "installed", "manifests"] {
            fs::create_dir_all(root.join(d))?;
        }
        fs::create_dir_all(prefix)?;
        Ok(Store { root: root.to_path_buf(), prefix: prefix.to_path_buf() })
    }

    pub fn prefix_dir(&self) -> &Path {
        &self.prefix
    }

    /// Install order for `id`: not-yet-installed dependencies first, then `id` itself.
    pub fn plan(&self, id: &str) -> Result<Vec<String>, IdeError> {
        let catalog = self.load_catalog()?;
        let installed: Vec<String> = self.installed().into_iter().map(|i| i.id).collect();
        let mut order = Vec::new();
        let mut visiting = Vec::new();
        fn visit(
            id: &str,
            catalog: &[Entry],
            installed: &[String],
            order: &mut Vec<String>,
            visiting: &mut Vec<String>,
        ) -> Result<(), IdeError> {
            if order.iter().any(|o| o == id) || installed.iter().any(|i| i == id) {
                return Ok(());
            }
            if visiting.iter().any(|v| v == id) {
                return Err(IdeError::Package(format!("dependency cycle at '{id}'")));
            }
            let e = catalog
                .iter()
                .find(|e| e.id == id)
                .ok_or_else(|| IdeError::Package(format!("'{id}' is not in the catalog")))?;
            visiting.push(id.to_string());
            for d in &e.deps {
                visit(d, catalog, installed, order, visiting)?;
            }
            visiting.pop();
            order.push(id.to_string());
            Ok(())
        }
        visit(id, &catalog, &installed, &mut order, &mut visiting)?;
        // The requested package is reinstalled even if it is already present.
        if !order.iter().any(|o| o == id) {
            order.push(id.to_string());
        }
        Ok(order)
    }

    fn dir_of(&self, p: &Installed) -> PathBuf {
        if p.prefix { self.prefix.clone() } else { self.install_dir(&p.id) }
    }

    pub fn catalog_path(&self) -> PathBuf {
        self.root.join("catalog.json")
    }

    pub fn load_catalog(&self) -> Result<Vec<Entry>, IdeError> {
        match fs::read_to_string(self.catalog_path()) {
            Ok(s) => parse_catalog(&s),
            Err(_) => Ok(Vec::new()),
        }
    }

    pub fn save_catalog(&self, json: &str) -> Result<usize, IdeError> {
        let n = parse_catalog(json)?.len(); // validate before replacing the old one
        fs::write(self.catalog_path(), json)?;
        Ok(n)
    }

    pub fn archive_path(&self, e: &Entry) -> PathBuf {
        let ext = if e.format == "zip" { "zip" } else { "tar.gz" };
        self.root.join("cache").join(format!("{}-{}.{}", e.id, e.version, ext))
    }

    pub fn install_dir(&self, id: &str) -> PathBuf {
        self.root.join("pkgs").join(id)
    }

    pub fn installed(&self) -> Vec<Installed> {
        let mut v: Vec<Installed> = fs::read_dir(self.root.join("installed"))
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|f| serde_json::from_str(&fs::read_to_string(f.path()).ok()?).ok())
            .collect();
        v.sort_by(|a, b| a.id.cmp(&b.id));
        v
    }

    pub fn find(&self, id: &str) -> Result<Entry, IdeError> {
        self.load_catalog()?
            .into_iter()
            .find(|e| e.id == id)
            .ok_or_else(|| IdeError::Package(format!("'{id}' is not in the catalog")))
    }

    /// Verify + extract the cached archive of `id`, replacing any previous version.
    pub fn install(&self, id: &str) -> Result<(), IdeError> {
        let e = self.find(id)?;
        let archive = self.archive_path(&e);
        if !archive.exists() {
            return Err(IdeError::Package(format!("{id}: archive not downloaded yet")));
        }
        let actual = sha256_file(&archive)?;
        if !actual.eq_ignore_ascii_case(&e.sha256) {
            let _ = fs::remove_file(&archive); // never keep a corrupt/tampered download
            return Err(IdeError::Package(format!(
                "{id}: checksum mismatch (expected {}, got {actual})",
                e.sha256
            )));
        }

        let tmp = self.root.join("pkgs").join(format!(".tmp-{id}"));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp)?;
        let result = match e.format.as_str() {
            "zip" => extract_zip(&archive, &tmp, e.strip as usize),
            _ => extract_tar_gz(&archive, &tmp, e.strip as usize),
        };
        if let Err(err) = result {
            let _ = fs::remove_dir_all(&tmp);
            return Err(err);
        }

        if e.prefix {
            self.remove_manifest_files(id);
            let mut files = Vec::new();
            merge_into(&tmp, &self.prefix, Path::new(""), &mut files)?;
            let _ = fs::remove_dir_all(&tmp);
            fs::write(
                self.root.join("manifests").join(format!("{id}.txt")),
                files.iter().map(|f| f.to_string_lossy().into_owned()).collect::<Vec<_>>().join("\n"),
            )?;
        } else {
            let dest = self.install_dir(id);
            let _ = fs::remove_dir_all(&dest);
            fs::rename(&tmp, &dest)?;
        }
        let rec = Installed {
            id: e.id.clone(),
            version: e.version.clone(),
            prefix: e.prefix,
            bin: e.bin.clone(),
            env: e.env.clone(),
        };
        fs::write(
            self.root.join("installed").join(format!("{id}.json")),
            serde_json::to_string_pretty(&rec).unwrap(),
        )?;
        Ok(())
    }

    pub fn uninstall(&self, id: &str) -> Result<(), IdeError> {
        validate_id(id)?;
        self.remove_manifest_files(id);
        let _ = fs::remove_dir_all(self.install_dir(id));
        let _ = fs::remove_file(self.root.join("installed").join(format!("{id}.json")));
        Ok(())
    }

    /// Delete the files a prefix package put into the shared prefix.
    fn remove_manifest_files(&self, id: &str) {
        let mf = self.root.join("manifests").join(format!("{id}.txt"));
        if let Ok(text) = fs::read_to_string(&mf) {
            let mut dirs = Vec::new();
            for rel in text.lines().filter(|l| !l.is_empty()) {
                let Ok(Some(rel)) = safe_member_path(Path::new(rel), 0) else { continue };
                let p = self.prefix.join(&rel);
                let _ = fs::remove_file(&p);
                if let Some(d) = rel.parent() {
                    dirs.push(d.to_path_buf());
                }
            }
            // Remove directories that became empty, deepest first.
            dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
            dirs.dedup();
            for d in dirs {
                let mut cur = d;
                while !cur.as_os_str().is_empty() && fs::remove_dir(self.prefix.join(&cur)).is_ok() {
                    cur = cur.parent().map(Path::to_path_buf).unwrap_or_default();
                }
            }
            let _ = fs::remove_file(mf);
        }
    }

    pub fn remove_cache(&self, id: &str) -> Result<(), IdeError> {
        for e in self.load_catalog()?.into_iter().filter(|e| e.id == id) {
            let _ = fs::remove_file(self.archive_path(&e));
        }
        Ok(())
    }

    /// Absolute bin directories of all installed packages, for PATH.
    pub fn path_dirs(&self) -> Vec<String> {
        self.installed()
            .iter()
            .flat_map(|p| {
                let base = self.dir_of(p);
                p.bin.iter().map(move |b| base.join(b).to_string_lossy().into_owned())
            })
            .collect()
    }

    /// "KEY=VALUE" entries from installed packages (with `{dir}` expanded).
    pub fn extra_env(&self) -> Vec<String> {
        self.installed()
            .iter()
            .flat_map(|p| {
                let dir = self.dir_of(p).to_string_lossy().into_owned();
                p.env.iter().map(move |(k, v)| format!("{k}={}", v.replace("{dir}", &dir)))
            })
            .collect()
    }
}

/// Move everything under `src` into `dst`, merging directories and replacing files.
/// Records each moved file/symlink (relative to the prefix) in `out`.
fn merge_into(src: &Path, dst: &Path, rel: &Path, out: &mut Vec<PathBuf>) -> Result<(), IdeError> {
    fs::create_dir_all(dst.join(rel))?;
    for entry in fs::read_dir(src.join(rel))? {
        let entry = entry?;
        let r = rel.join(entry.file_name());
        let meta = fs::symlink_metadata(entry.path())?;
        if meta.is_dir() {
            merge_into(src, dst, &r, out)?;
        } else {
            let to = dst.join(&r);
            let _ = fs::remove_file(&to);
            if fs::rename(entry.path(), &to).is_err() {
                // Different filesystem: copy (symlinks recreated).
                #[cfg(unix)]
                if meta.file_type().is_symlink() {
                    std::os::unix::fs::symlink(fs::read_link(entry.path())?, &to)?;
                    out.push(r);
                    continue;
                }
                fs::copy(entry.path(), &to)?;
            }
            out.push(r);
        }
    }
    Ok(())
}

pub fn sha256_file(path: &Path) -> Result<String, IdeError> {
    let mut f = File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Turn an archive member path into a safe path under the destination:
/// drops `strip` leading components and rejects absolute paths and `..`.
/// Returns None when nothing is left (e.g. the stripped top-level directory).
pub fn safe_member_path(raw: &Path, strip: usize) -> Result<Option<PathBuf>, IdeError> {
    let mut parts = Vec::new();
    for c in raw.components() {
        match c {
            Component::Normal(p) => parts.push(p.to_os_string()),
            Component::CurDir => {}
            _ => return Err(IdeError::Package(format!("unsafe path in archive: {}", raw.display()))),
        }
    }
    if parts.len() <= strip {
        return Ok(None);
    }
    Ok(Some(parts[strip..].iter().collect()))
}

fn extract_tar_gz(archive: &Path, dest: &Path, strip: usize) -> Result<(), IdeError> {
    let gz = flate2::read::GzDecoder::new(File::open(archive)?);
    let mut tar = tar::Archive::new(gz);
    for entry in tar.entries().map_err(|e| IdeError::Package(e.to_string()))? {
        let mut entry = entry.map_err(|e| IdeError::Package(e.to_string()))?;
        let raw = entry.path().map_err(|e| IdeError::Package(e.to_string()))?.into_owned();
        let Some(rel) = safe_member_path(&raw, strip)? else { continue };
        let out = dest.join(&rel);
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent)?;
        }
        let kind = entry.header().entry_type();
        if kind.is_dir() {
            fs::create_dir_all(&out)?;
        } else if kind.is_file() {
            let mode = entry.header().mode().unwrap_or(0o644);
            let mut f = File::create(&out)?;
            std::io::copy(&mut entry, &mut f)?;
            f.flush()?;
            set_mode(&out, mode)?;
        } else if kind.is_symlink() {
            let target = entry
                .link_name()
                .map_err(|e| IdeError::Package(e.to_string()))?
                .ok_or_else(|| IdeError::Package("symlink without target".into()))?
                .into_owned();
            // A link must stay inside the package: resolve it relative to its own folder.
            let base = rel.parent().unwrap_or(Path::new("")).to_path_buf();
            if target.is_absolute() || !stays_inside(&base, &target) {
                return Err(IdeError::Package(format!("symlink escapes package: {}", raw.display())));
            }
            #[cfg(unix)]
            {
                let _ = fs::remove_file(&out);
                std::os::unix::fs::symlink(&target, &out)?;
            }
        }
        // hard links, devices, fifos: ignored
    }
    Ok(())
}

fn stays_inside(base: &Path, target: &Path) -> bool {
    let mut depth: i64 = base.components().count() as i64;
    for c in target.components() {
        match c {
            Component::ParentDir => depth -= 1,
            Component::Normal(_) => depth += 1,
            _ => {}
        }
        if depth < 0 {
            return false;
        }
    }
    true
}

fn extract_zip(archive: &Path, dest: &Path, strip: usize) -> Result<(), IdeError> {
    let mut zip = zip::ZipArchive::new(File::open(archive)?).map_err(|e| IdeError::Package(e.to_string()))?;
    for i in 0..zip.len() {
        let mut f = zip.by_index(i).map_err(|e| IdeError::Package(e.to_string()))?;
        let name = f.enclosed_name().ok_or_else(|| IdeError::Package(format!("unsafe path in archive: {}", f.name())))?;
        let Some(rel) = safe_member_path(&name, strip)? else { continue };
        let out = dest.join(rel);
        if f.is_dir() {
            fs::create_dir_all(&out)?;
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent)?;
        }
        let mode = f.unix_mode();
        let mut o = File::create(&out)?;
        std::io::copy(&mut f, &mut o)?;
        drop(o);
        if let Some(m) = mode {
            set_mode(&out, m)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn set_mode(p: &Path, mode: u32) -> Result<(), IdeError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(p, fs::Permissions::from_mode(mode & 0o777))?;
    Ok(())
}
#[cfg(not(unix))]
fn set_mode(_: &Path, _: u32) -> Result<(), IdeError> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a tar.gz with one top-level folder: `pkg-1.0/bin/hello` (executable) and `pkg-1.0/README`.
    fn make_tar_gz(path: &Path) {
        let gz = flate2::write::GzEncoder::new(File::create(path).unwrap(), flate2::Compression::fast());
        let mut b = tar::Builder::new(gz);
        let script = b"#!/bin/sh\necho hello-from-pkg\n";
        let mut h = tar::Header::new_gnu();
        h.set_size(script.len() as u64);
        h.set_mode(0o755);
        h.set_cksum();
        b.append_data(&mut h, "pkg-1.0/bin/hello", &script[..]).unwrap();
        let readme = b"readme";
        let mut h = tar::Header::new_gnu();
        h.set_size(readme.len() as u64);
        h.set_mode(0o644);
        h.set_cksum();
        b.append_data(&mut h, "pkg-1.0/README", &readme[..]).unwrap();
        b.into_inner().unwrap().finish().unwrap();
    }

    fn catalog(sha: &str, format: &str) -> String {
        format!(
            r#"{{"version":1,"packages":[{{"id":"demo","name":"Demo","version":"1.0","url":"https://example.invalid/demo.tar.gz",
            "sha256":"{sha}","format":"{format}","strip":1,"bin":["bin"],"env":{{"DEMO_HOME":"{{dir}}"}}}}]}}"#
        )
    }

    fn setup(format: &str) -> (tempfile::TempDir, Store, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(&tmp.path().join("packages")).unwrap();
        let src = tmp.path().join("src.tar.gz");
        make_tar_gz(&src);
        let sha = sha256_file(&src).unwrap();
        store.save_catalog(&catalog(&sha, format)).unwrap();
        (tmp, store, src)
    }

    #[test]
    fn install_verifies_extracts_strips_and_keeps_exec_bit() {
        let (_t, store, src) = setup("tar.gz");
        let e = store.find("demo").unwrap();
        assert!(store.install("demo").is_err(), "must refuse before download");
        fs::copy(&src, store.archive_path(&e)).unwrap();
        store.install("demo").unwrap();

        let hello = store.install_dir("demo").join("bin/hello");
        assert!(hello.exists());
        assert!(store.install_dir("demo").join("README").exists(), "strip=1 removes pkg-1.0/");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&hello).unwrap().permissions().mode() & 0o111, 0o111);
        }
        let out = std::process::Command::new(&hello).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "hello-from-pkg");

        let inst = store.installed();
        assert_eq!(inst.len(), 1);
        assert_eq!(inst[0].version, "1.0");
        assert_eq!(store.path_dirs(), vec![store.install_dir("demo").join("bin").to_string_lossy().to_string()]);
        let env = store.extra_env();
        assert_eq!(env, vec![format!("DEMO_HOME={}", store.install_dir("demo").display())]);

        store.uninstall("demo").unwrap();
        assert!(store.installed().is_empty());
        assert!(!store.install_dir("demo").exists());
        assert!(store.archive_path(&e).exists(), "uninstall keeps the cached archive for offline reinstall");
        store.install("demo").unwrap(); // offline reinstall from cache
        assert_eq!(store.installed().len(), 1);
    }

    fn tgz(path: &Path, files: &[(&str, &str, u32)]) -> String {
        let gz = flate2::write::GzEncoder::new(File::create(path).unwrap(), flate2::Compression::fast());
        let mut b = tar::Builder::new(gz);
        for (name, body, mode) in files {
            let mut h = tar::Header::new_gnu();
            h.set_size(body.len() as u64);
            h.set_mode(*mode);
            h.set_cksum();
            b.append_data(&mut h, name, body.as_bytes()).unwrap();
        }
        b.into_inner().unwrap().finish().unwrap();
        sha256_file(path).unwrap()
    }

    #[test]
    fn prefix_packages_share_a_prefix_resolve_deps_and_uninstall_cleanly() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path().join("u");
        let store = Store::with_prefix(&tmp.path().join("packages"), &prefix).unwrap();
        let (a, b) = (tmp.path().join("libfoo.tgz"), tmp.path().join("tool.tgz"));
        let sha_a = tgz(&a, &[("lib/libfoo.so", "lib", 0o644), ("share/foo/data", "d", 0o644)]);
        let sha_b = tgz(&b, &[("bin/tool", "#!/bin/sh\necho tool-ok\n", 0o755)]);
        let cat = format!(
            r#"{{"version":1,"packages":[
            {{"id":"libfoo","name":"libfoo","version":"1","url":"u","sha256":"{sha_a}","prefix":true,"hidden":true}},
            {{"id":"tool","name":"Tool","version":"1","url":"u","sha256":"{sha_b}","prefix":true,
              "deps":["libfoo"],"bin":["bin"],"env":{{"TOOL_HOME":"{{dir}}"}}}}]}}"#
        );
        store.save_catalog(&cat).unwrap();
        assert_eq!(store.plan("tool").unwrap(), vec!["libfoo", "tool"]);
        for (id, src) in [("libfoo", &a), ("tool", &b)] {
            fs::copy(src, store.archive_path(&store.find(id).unwrap())).unwrap();
            store.install(id).unwrap();
        }
        assert!(prefix.join("lib/libfoo.so").exists());
        let out = std::process::Command::new(prefix.join("bin/tool")).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "tool-ok");
        assert_eq!(store.path_dirs(), vec![prefix.join("bin").to_string_lossy().to_string()]);
        assert_eq!(store.extra_env(), vec![format!("TOOL_HOME={}", prefix.display())]);
        assert_eq!(store.plan("tool").unwrap(), vec!["tool"], "installed deps are skipped");

        store.uninstall("tool").unwrap();
        assert!(!prefix.join("bin/tool").exists());
        assert!(!prefix.join("bin").exists(), "empty directories are cleaned up");
        assert!(prefix.join("lib/libfoo.so").exists(), "other packages are untouched");
        store.uninstall("libfoo").unwrap();
        assert!(!prefix.join("share").exists());
    }

    #[test]
    fn dependency_cycles_and_unknown_deps_are_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(&tmp.path().join("p")).unwrap();
        let sha = "a".repeat(64);
        let cat = format!(
            r#"{{"packages":[{{"id":"a","name":"A","version":"1","url":"u","sha256":"{sha}","deps":["b"]}},
                             {{"id":"b","name":"B","version":"1","url":"u","sha256":"{sha}","deps":["a"]}},
                             {{"id":"c","name":"C","version":"1","url":"u","sha256":"{sha}","deps":["zzz"]}}]}}"#
        );
        store.save_catalog(&cat).unwrap();
        assert!(store.plan("a").unwrap_err().to_string().contains("cycle"));
        assert!(store.plan("c").unwrap_err().to_string().contains("not in the catalog"));
    }

    #[test]
    fn corrupt_download_is_rejected_and_deleted() {
        let (_t, store, _src) = setup("tar.gz");
        let e = store.find("demo").unwrap();
        fs::write(store.archive_path(&e), b"not the real archive").unwrap();
        let err = store.install("demo").unwrap_err().to_string();
        assert!(err.contains("checksum mismatch"), "{err}");
        assert!(!store.archive_path(&e).exists());
        assert!(store.installed().is_empty());
    }

    #[test]
    fn path_traversal_is_rejected() {
        assert!(safe_member_path(Path::new("../evil"), 0).is_err());
        assert!(safe_member_path(Path::new("/etc/passwd"), 0).is_err());
        assert!(safe_member_path(Path::new("a/../../b"), 0).is_err());
        assert_eq!(safe_member_path(Path::new("top/a/b"), 1).unwrap(), Some(PathBuf::from("a/b")));
        assert_eq!(safe_member_path(Path::new("top"), 1).unwrap(), None);
        assert!(stays_inside(Path::new("bin"), Path::new("../lib/x")));
        assert!(!stays_inside(Path::new("bin"), Path::new("../../x")));
    }

    #[test]
    fn catalog_validation() {
        assert!(parse_catalog("{}").is_err());
        assert!(parse_catalog(&catalog("zz", "tar.gz")).is_err());
        assert!(parse_catalog(&catalog(&"a".repeat(64), "rar")).is_err());
        let bad_id = catalog(&"a".repeat(64), "zip").replace("\"demo\"", "\"../x\"");
        assert!(parse_catalog(&bad_id).is_err());
        assert_eq!(parse_catalog(&catalog(&"a".repeat(64), "zip")).unwrap().len(), 1);
        let tmp = tempfile::tempdir().unwrap();
        let s = Store::new(tmp.path()).unwrap();
        assert!(s.save_catalog("garbage").is_err());
        assert!(s.load_catalog().unwrap().is_empty());
    }

    #[test]
    fn zip_install_works() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(&tmp.path().join("p")).unwrap();
        let zpath = tmp.path().join("a.zip");
        {
            let mut z = zip::ZipWriter::new(File::create(&zpath).unwrap());
            let opts = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
            z.start_file("top/bin/tool", opts).unwrap();
            z.write_all(b"#!/bin/sh\necho zip-ok\n").unwrap();
            z.finish().unwrap();
        }
        let sha = sha256_file(&zpath).unwrap();
        store.save_catalog(&catalog(&sha, "zip")).unwrap();
        let e = store.find("demo").unwrap();
        fs::copy(&zpath, store.archive_path(&e)).unwrap();
        store.install("demo").unwrap();
        let out = std::process::Command::new(store.install_dir("demo").join("bin/tool")).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "zip-ok");
    }
}
