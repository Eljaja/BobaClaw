use std::path::{Component, Path};

use bobaclaw_core::{head_tail_with_hint, BobaPaths};
use bobaclaw_provider::{FunctionSpec, ToolCall, ToolSpec};
use bobaclaw_state::SessionStore;
use serde::Deserialize;
use serde_json::json;
use sqlx::SqlitePool;

pub const MEMORY_MANAGE: &str = "memory_manage";
pub const MEMORY_SEARCH: &str = "memory_search";
pub const MEMORY_READ: &str = "memory_read";

/// Max bytes appended in one call.
pub const MEMORY_APPEND_MAX_BYTES: usize = 4_096;
/// Max total file size after append.
pub const MEMORY_FILE_MAX_BYTES: usize = 65_536;
const MAX_READ_CHARS: usize = 24_000;
const DEFAULT_SEARCH_LIMIT: i64 = 10;

pub fn memory_tool_specs() -> Vec<ToolSpec> {
    vec![
        memory_manage_spec(),
        memory_search_spec(),
        memory_read_spec(),
    ]
}

pub fn memory_tool_spec() -> ToolSpec {
    memory_manage_spec()
}

fn memory_manage_spec() -> ToolSpec {
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

fn memory_search_spec() -> ToolSpec {
    ToolSpec {
        kind: "function".into(),
        function: FunctionSpec {
            name: MEMORY_SEARCH.into(),
            description: "Search past session messages (FTS) and workspace memory files for a query. \
                When you use results in the answer, cite the memory file path or session in Sources."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "limit": { "type": "integer", "description": "Max hits per source (default 10, max 50)" }
                },
                "required": ["query"]
            }),
        },
    }
}

fn memory_read_spec() -> ToolSpec {
    ToolSpec {
        kind: "function".into(),
        function: FunctionSpec {
            name: MEMORY_READ.into(),
            description: "Read MEMORY.md or memory/<file> beyond prompt injection caps. \
                Cite the file path in Sources when using retrieved facts."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string" },
                    "offset": { "type": "integer", "description": "1-based start line (optional)" },
                    "limit": { "type": "integer", "description": "Max lines (optional)" }
                },
                "required": ["path"]
            }),
        },
    }
}

pub fn is_memory_tool(name: &str) -> bool {
    matches!(name, MEMORY_MANAGE | MEMORY_SEARCH | MEMORY_READ)
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
    match call.function.name.as_str() {
        MEMORY_MANAGE => handle_memory_manage(paths, agent_group, call),
        MEMORY_READ => handle_memory_read(paths, agent_group, call),
        other => anyhow::bail!("sync memory tool not supported: {other}"),
    }
}

pub async fn handle_memory_search_tool(
    paths: &BobaPaths,
    pool: &SqlitePool,
    agent_group: &str,
    call: &ToolCall,
) -> anyhow::Result<String> {
    let args: MemorySearchArgs = serde_json::from_str(&call.function.arguments)
        .map_err(|e| anyhow::anyhow!("invalid memory_search arguments: {e}"))?;

    let limit = args.limit.unwrap_or(DEFAULT_SEARCH_LIMIT);
    let store = SessionStore::new(pool);
    let hits = store
        .search_messages(agent_group, &args.query, limit)
        .await?;

    let workspace = paths.group_workspace(agent_group);
    let file_hits = search_memory_files(&workspace, &args.query, limit as usize);

    let mut lines = Vec::new();
    if !hits.is_empty() {
        lines.push("## Session messages".into());
        for h in hits {
            lines.push(format!(
                "- session={} role={} ts={:.0}: {}",
                h.session_id, h.role, h.timestamp, h.snippet
            ));
        }
    }
    if !file_hits.is_empty() {
        lines.push("## Memory files".into());
        for h in file_hits {
            lines.push(format!("- {}: {}", h.path, h.snippet));
        }
    }
    if lines.is_empty() {
        Ok(format!("No matches for '{}'.", args.query.trim()))
    } else {
        Ok(lines.join("\n"))
    }
}

fn handle_memory_manage(
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

#[derive(Debug, Deserialize)]
struct MemorySearchArgs {
    query: String,
    #[serde(default)]
    limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct MemoryReadArgs {
    path: String,
    #[serde(default)]
    offset: Option<u32>,
    #[serde(default)]
    limit: Option<u32>,
}

struct MemoryFileHit {
    path: String,
    snippet: String,
}

fn search_memory_files(workspace: &Path, query: &str, limit: usize) -> Vec<MemoryFileHit> {
    let q = query.to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    let candidates = memory_file_candidates(workspace);
    for path in candidates {
        let Ok(content) = std::fs::read_to_string(&path) else {
            continue;
        };
        if !content.to_lowercase().contains(&q) {
            continue;
        }
        let rel = path
            .strip_prefix(workspace)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| path.display().to_string());
        let snippet = extract_snippet(&content, &q, 120);
        hits.push(MemoryFileHit { path: rel, snippet });
        if hits.len() >= limit {
            break;
        }
    }
    hits
}

fn memory_file_candidates(workspace: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let root = workspace.join("MEMORY.md");
    if root.is_file() {
        out.push(root);
    }
    let mem_dir = workspace.join("memory");
    if mem_dir.is_dir() {
        if let Ok(read) = std::fs::read_dir(&mem_dir) {
            for entry in read.flatten() {
                let p = entry.path();
                if p.is_file() {
                    out.push(p);
                }
            }
        }
    }
    out
}

fn extract_snippet(content: &str, query: &str, max_chars: usize) -> String {
    let lower = content.to_lowercase();
    let Some(idx) = lower.find(query) else {
        return content.chars().take(max_chars).collect();
    };
    let start = idx.saturating_sub(40);
    let slice: String = content.chars().skip(start).take(max_chars).collect();
    if start > 0 {
        format!("…{slice}")
    } else {
        slice
    }
}

fn handle_memory_read(
    paths: &BobaPaths,
    agent_group: &str,
    call: &ToolCall,
) -> anyhow::Result<String> {
    let args: MemoryReadArgs = serde_json::from_str(&call.function.arguments)
        .map_err(|e| anyhow::anyhow!("invalid memory_read arguments: {e}"))?;

    let workspace = paths.group_workspace(agent_group);
    let rel = validate_memory_path(&args.path)?;
    let target = workspace.join(&rel);
    if !target.is_file() {
        anyhow::bail!("memory file not found: {rel}");
    }

    let content = std::fs::read_to_string(&target)?;
    let output = match (args.offset, args.limit) {
        (None, None) => content,
        (offset, limit) => {
            let start = offset.unwrap_or(1).max(1) as usize;
            let lines: Vec<&str> = content.lines().collect();
            let idx = start.saturating_sub(1);
            if idx >= lines.len() {
                String::new()
            } else {
                let end = limit.map(|l| idx + l as usize).unwrap_or(lines.len());
                lines[idx..end.min(lines.len())].join("\n")
            }
        }
    };

    if output.chars().count() > MAX_READ_CHARS {
        let hint = format!("truncated '{rel}' — use offset/limit");
        Ok(head_tail_with_hint(&output, MAX_READ_CHARS, &hint))
    } else {
        Ok(output)
    }
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
