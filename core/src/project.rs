//! Project file operations, sandboxed to the project root.

use crate::{FileEntry, IdeError};
use std::path::{Component, Path, PathBuf};

/// Resolve a project-relative path, refusing absolute paths and `..` escapes.
pub fn resolve(root: &Path, rel: &str) -> Result<PathBuf, IdeError> {
    let mut out = root.to_path_buf();
    for c in Path::new(rel).components() {
        match c {
            Component::Normal(p) => out.push(p),
            Component::CurDir => {}
            _ => return Err(IdeError::OutsideRoot(rel.to_string())),
        }
    }
    Ok(out)
}

pub fn list_dir(root: &Path, rel: &str) -> Result<Vec<FileEntry>, IdeError> {
    let dir = resolve(root, rel)?;
    let mut items = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == ".git" {
            continue;
        }
        let path = if rel.is_empty() || rel == "." {
            name.clone()
        } else {
            format!("{}/{}", rel.trim_end_matches('/'), name)
        };
        items.push(FileEntry { name, path, is_dir: entry.file_type()?.is_dir() });
    }
    items.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(items)
}

pub fn read_file(root: &Path, rel: &str) -> Result<String, IdeError> {
    Ok(std::fs::read_to_string(resolve(root, rel)?)?)
}

pub fn write_file(root: &Path, rel: &str, text: &str) -> Result<(), IdeError> {
    let p = resolve(root, rel)?;
    // Write to a temp file then rename so a crash never leaves a half-written file.
    let tmp = p.with_extension("ide-tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, &p)?;
    Ok(())
}

pub fn create_file(root: &Path, rel: &str) -> Result<(), IdeError> {
    let p = resolve(root, rel)?;
    if p.exists() {
        return Err(IdeError::AlreadyExists(rel.to_string()));
    }
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(p, "")?;
    Ok(())
}

pub fn create_dir(root: &Path, rel: &str) -> Result<(), IdeError> {
    let p = resolve(root, rel)?;
    if p.exists() {
        return Err(IdeError::AlreadyExists(rel.to_string()));
    }
    std::fs::create_dir_all(p)?;
    Ok(())
}

pub fn delete_path(root: &Path, rel: &str) -> Result<(), IdeError> {
    let p = resolve(root, rel)?;
    if p == root {
        return Err(IdeError::OutsideRoot(rel.to_string()));
    }
    if p.is_dir() {
        std::fs::remove_dir_all(p)?;
    } else {
        std::fs::remove_file(p)?;
    }
    Ok(())
}

pub fn rename_path(root: &Path, from: &str, to: &str) -> Result<(), IdeError> {
    let a = resolve(root, from)?;
    let b = resolve(root, to)?;
    if b.exists() {
        return Err(IdeError::AlreadyExists(to.to_string()));
    }
    std::fs::rename(a, b)?;
    Ok(())
}
