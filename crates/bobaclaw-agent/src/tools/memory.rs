use std::path::{Component, Path};

use bobaclaw_core::BobaPaths;
use bobaclaw_provider::{FunctionSpec, ToolCall, ToolSpec};
use serde::Deserialize;
use serde_json::json;

pub const MEMORY_MANAGE: &str = "memory_manage";

/// Max bytes appended in one call.
pub const MEMORY_APPEND_MAX_BYTES: usize = 4_096;
/// Max total file size after append.
pub const MEMORY_FILE_MAX_BYTES: usize = 65_536;

pub fn memory_tool_spec() -> ToolSpec {
    ToolSpec {
        kind: "function".into(),
        function: FunctionSpec {
            name: MEMORY_MANAGE.into(),
            description: "Append durable facts, preferences, or user context to workspace memory. \
                Use for things to remember across sessions — not for repeatable tool workflows (use skills for those). \
                action=append only."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["append"]
                    },
                    "path": {
                        "type": "string",
                        "description": "MEMORY.md or memory/<file> (.md or .txt)"
                    },
                    "content": {
                        "type": "string",
                        "description": "Text to append (newline added if missing)"
                    }
                },
                "required": ["action", "path", "content"]
            }),
        },
    }
}

pub fn is_memory_tool(name: &str) -> bool {
    name == MEMORY_MANAGE
}

#[derive(Debug, Deserialize)]
struct MemoryManageArgs {
    action: String,
    path: String,
    content: String,
}

pub fn handle_memory_tool(
    paths: &BobaPaths,
    agent_group: &str,
    call: &ToolCall,
) -> anyhow::Result<String> {
    let args: MemoryManageArgs = serde_json::from_str(&call.function.arguments)
        .map_err(|e| anyhow::anyhow!("invalid memory_manage arguments: {e}"))?;

    if args.action != "append" {
        anyhow::bail!("unknown memory_manage action: {}", args.action);
    }

    let workspace = paths.group_workspace(agent_group);
    let rel = validate_memory_path(&args.path)?;
    let target = workspace.join(&rel);
    let rel_display = rel.replace('\\', "/");

    if args.content.len() > MEMORY_APPEND_MAX_BYTES {
        anyhow::bail!(
            "content exceeds max append size ({} bytes)",
            MEMORY_APPEND_MAX_BYTES
        );
    }

    if let Some(parent) = target.parent() {
        if parent != workspace && parent.file_name().and_then(|n| n.to_str()) == Some("memory") {
            std::fs::create_dir_all(parent)?;
        }
    }

    let existing_len = if target.exists() {
        std::fs::metadata(&target)?.len() as usize
    } else {
        0
    };
    if existing_len + args.content.len() > MEMORY_FILE_MAX_BYTES {
        anyhow::bail!(
            "append would exceed max file size ({} bytes)",
            MEMORY_FILE_MAX_BYTES
        );
    }

    let mut chunk = args.content;
    if !chunk.ends_with('\n') {
        chunk.push('\n');
    }

    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&target)?;
    file.write_all(chunk.as_bytes())?;

    Ok(format!("Appended to '{rel_display}'."))
}

/// Resolve and validate a memory path relative to workspace group root.
pub fn validate_memory_path(path: &str) -> anyhow::Result<String> {
    let trimmed = path.trim().replace('\\', "/");
    if trimmed.is_empty() {
        anyhow::bail!("path must not be empty");
    }
    if trimmed.starts_with('/') {
        anyhow::bail!("path must be relative");
    }

    let rel = Path::new(&trimmed);
    for component in rel.components() {
        match component {
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                anyhow::bail!("path must not contain '..'");
            }
            _ => {}
        }
    }

    if rel == Path::new("MEMORY.md") {
        return Ok("MEMORY.md".to_string());
    }

    let rel_str = trimmed.as_str();
    if let Some(name) = rel_str.strip_prefix("memory/") {
        if name.is_empty() {
            anyhow::bail!("memory path must include a filename");
        }
        if name.contains('/') {
            anyhow::bail!("memory path must be memory/<file> only");
        }
        if name == "MEMORY.md" {
            anyhow::bail!("use MEMORY.md at workspace root, not under memory/");
        }
        let ext = Path::new(name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        if ext != "md" && ext != "txt" {
            anyhow::bail!("memory files must be .md or .txt");
        }
        return Ok(format!("memory/{name}"));
    }

    anyhow::bail!("path must be MEMORY.md or memory/<file>")
}

#[cfg(test)]
mod tests {
    use super::*;
    use bobaclaw_provider::ToolCall;

    fn memory_call(path: &str, content: &str) -> ToolCall {
        use bobaclaw_provider::FunctionCallPayload;
        ToolCall {
            id: "1".into(),
            kind: "function".into(),
            function: FunctionCallPayload {
                name: MEMORY_MANAGE.into(),
                arguments: serde_json::json!({
                    "action": "append",
                    "path": path,
                    "content": content
                })
                .to_string(),
            },
        }
    }

    #[test]
    fn validate_accepts_memory_md_and_memory_dir() {
        assert_eq!(validate_memory_path("MEMORY.md").unwrap(), "MEMORY.md");
        assert_eq!(
            validate_memory_path("memory/words.txt").unwrap(),
            "memory/words.txt"
        );
    }

    #[test]
    fn validate_rejects_traversal() {
        assert!(validate_memory_path("../MEMORY.md").is_err());
        assert!(validate_memory_path("memory/../MEMORY.md").is_err());
        assert!(validate_memory_path("/MEMORY.md").is_err());
    }

    #[test]
    fn append_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_path_buf();
        let ws = home.join("workspace").join("home");
        std::fs::create_dir_all(&ws).unwrap();
        let paths = BobaPaths {
            home: home.clone(),
            config: home.join("config.yaml"),
            state_db: home.join("state.db"),
            runs: home.join("runs"),
            workspace: home.join("workspace"),
        };
        let call = memory_call("memory/notes.txt", "hello");
        let body = handle_memory_tool(&paths, "home", &call).unwrap();
        assert!(body.contains("memory/notes.txt"));
        let content = std::fs::read_to_string(ws.join("memory/notes.txt")).unwrap();
        assert_eq!(content, "hello\n");
    }
}
