//! Sandboxed file operations with path traversal protection.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Resolve a path and ensure it stays within the sandbox root.
/// Returns the canonical path or an error if traversal is detected.
pub fn resolve_safe_path(requested: &str, sandbox_root: &Path) -> Result<PathBuf> {
    let resolved = if Path::new(requested).is_absolute() {
        PathBuf::from(requested)
    } else {
        sandbox_root.join(requested)
    };

    // Canonicalize the sandbox root
    let canon_root = sandbox_root
        .canonicalize()
        .or_else(|_| {
            std::fs::create_dir_all(sandbox_root)?;
            sandbox_root.canonicalize()
        })
        .context("Failed to resolve sandbox root")?;

    // For new files that don't exist yet, check the parent
    let check_path = if resolved.exists() {
        resolved
            .canonicalize()
            .context("Failed to canonicalize path")?
    } else {
        let parent = resolved.parent().context("Path has no parent")?;
        if !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
        let canon_parent = parent.canonicalize()?;
        canon_parent.join(resolved.file_name().context("Path has no file name")?)
    };

    if !check_path.starts_with(&canon_root) {
        anyhow::bail!(
            "Path traversal detected: {} is outside sandbox {}",
            check_path.display(),
            canon_root.display()
        );
    }

    Ok(check_path)
}

/// Read a file within the sandbox.
pub fn read_file(path: &str, sandbox_root: &Path) -> Result<String> {
    let safe_path = resolve_safe_path(path, sandbox_root)?;
    std::fs::read_to_string(&safe_path)
        .with_context(|| format!("Failed to read {}", safe_path.display()))
}

/// Write a file within the sandbox.
pub fn write_file(path: &str, content: &str, sandbox_root: &Path) -> Result<()> {
    let safe_path = resolve_safe_path(path, sandbox_root)?;
    if let Some(parent) = safe_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&safe_path, content)
        .with_context(|| format!("Failed to write {}", safe_path.display()))
}

/// List files in a directory within the sandbox.
pub fn list_files(path: &str, sandbox_root: &Path) -> Result<Vec<String>> {
    let safe_path = resolve_safe_path(path, sandbox_root)?;
    let mut entries = Vec::new();

    for entry in std::fs::read_dir(&safe_path)
        .with_context(|| format!("Failed to list {}", safe_path.display()))?
    {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = entry.file_type()?.is_dir();
        entries.push(if is_dir { format!("{name}/") } else { name });
    }

    entries.sort();
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_path_traversal_blocked() {
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = tmp.path().join("sandbox");
        fs::create_dir_all(&sandbox).unwrap();

        // Normal path should work
        assert!(resolve_safe_path("test.txt", &sandbox).is_ok());

        // Traversal should fail
        assert!(resolve_safe_path("../../../etc/passwd", &sandbox).is_err());
    }
}
