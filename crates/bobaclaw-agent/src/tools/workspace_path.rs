use std::path::{Component, Path, PathBuf};

/// Validate a workspace-relative path (no `..`, no absolute roots).
pub fn validate_relative_path(path: &str) -> anyhow::Result<String> {
    let trimmed = path.trim().replace('\\', "/");
    if trimmed.is_empty() {
        anyhow::bail!("path must not be empty");
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        anyhow::bail!(
            "looks like a URL — use web_fetch for web pages, or exec (e.g. yt-dlp) for video; file_read is workspace files only"
        );
    }
    if trimmed.starts_with('/') {
        anyhow::bail!(
            "path must be workspace-relative (not an absolute or URL path); for http(s) links use web_fetch"
        );
    }

    let rel = Path::new(&trimmed);
    for component in rel.components() {
        match component {
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                anyhow::bail!("path must not contain '..' or absolute segments");
            }
            _ => {}
        }
    }
    Ok(trimmed)
}

/// Resolve `rel` under `workspace` and ensure the result stays inside the workspace (symlink-safe).
pub fn resolve_in_workspace(workspace: &Path, rel: &str) -> anyhow::Result<PathBuf> {
    let rel = validate_relative_path(rel)?;
    let target = workspace.join(&rel);
    let canonical_ws = std::fs::canonicalize(workspace)
        .map_err(|e| anyhow::anyhow!("workspace unavailable: {e}"))?;

    let resolved = if target.exists() {
        std::fs::canonicalize(&target)
            .map_err(|e| anyhow::anyhow!("cannot resolve path '{rel}': {e}"))?
    } else {
        let parent = target
            .parent()
            .filter(|p| p.exists())
            .map(std::fs::canonicalize)
            .transpose()
            .map_err(|e| anyhow::anyhow!("cannot resolve parent of '{rel}': {e}"))?
            .unwrap_or_else(|| canonical_ws.clone());
        let file_name = target
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("invalid path '{rel}'"))?;
        parent.join(file_name)
    };

    if !resolved.starts_with(&canonical_ws) {
        anyhow::bail!("path escapes workspace: {rel}");
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_traversal() {
        assert!(validate_relative_path("../etc/passwd").is_err());
        assert!(validate_relative_path("/etc/passwd").is_err());
    }

    #[test]
    fn rejects_urls() {
        let err = validate_relative_path("https://www.youtube.com/watch?v=abc").unwrap_err();
        assert!(err.to_string().contains("web_fetch"));
    }

    #[test]
    fn resolve_blocks_symlink_escape() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        let outside = dir.path().join("outside.txt");
        std::fs::write(&outside, "secret").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&outside, ws.join("link.txt")).unwrap();
            assert!(resolve_in_workspace(&ws, "link.txt").is_err());
        }
    }
}
