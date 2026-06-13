use bobaclaw_core::{head_tail_with_hint, BobaPaths};
use bobaclaw_provider::{FunctionSpec, ToolCall, ToolSpec};
use serde::Deserialize;
use serde_json::json;

use super::workspace_path::{resolve_in_workspace, validate_relative_path};

const FILE_READ: &str = "file_read";
const FILE_WRITE: &str = "file_write";
const FILE_EDIT: &str = "file_edit";

const MAX_READ_CHARS: usize = 24_000;

pub fn file_tool_specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec {
            kind: "function".into(),
            function: FunctionSpec {
                name: FILE_READ.into(),
                description: "Read a file inside the agent workspace. Prefer over `exec cat`. \
                    Supports optional line offset and limit."
                    .into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Workspace-relative path" },
                        "offset": { "type": "integer", "description": "1-based start line (optional)" },
                        "limit": { "type": "integer", "description": "Max lines to read (optional)" }
                    },
                    "required": ["path"]
                }),
            },
        },
        ToolSpec {
            kind: "function".into(),
            function: FunctionSpec {
                name: FILE_WRITE.into(),
                description: "Create or overwrite a file inside the workspace. Prefer over heredoc via exec."
                    .into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "contents": { "type": "string" }
                    },
                    "required": ["path", "contents"]
                }),
            },
        },
        ToolSpec {
            kind: "function".into(),
            function: FunctionSpec {
                name: FILE_EDIT.into(),
                description: "Replace exact text in a workspace file. Fails if old_string is missing or ambiguous \
                    unless replace_all is true."
                    .into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string" },
                        "old_string": { "type": "string" },
                        "new_string": { "type": "string" },
                        "replace_all": { "type": "boolean", "description": "Replace every occurrence (default false)" }
                    },
                    "required": ["path", "old_string", "new_string"]
                }),
            },
        },
    ]
}

pub fn is_file_tool(name: &str) -> bool {
    matches!(name, FILE_READ | FILE_WRITE | FILE_EDIT)
}

#[derive(Debug, Deserialize)]
struct FileReadArgs {
    path: String,
    #[serde(default)]
    offset: Option<u32>,
    #[serde(default)]
    limit: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct FileWriteArgs {
    path: String,
    contents: String,
}

#[derive(Debug, Deserialize)]
struct FileEditArgs {
    path: String,
    old_string: String,
    new_string: String,
    #[serde(default)]
    replace_all: bool,
}

pub fn handle_file_tool(
    paths: &BobaPaths,
    agent_group: &str,
    call: &ToolCall,
) -> anyhow::Result<String> {
    match call.function.name.as_str() {
        FILE_READ => handle_file_read(paths, agent_group, call),
        FILE_WRITE => handle_file_write(paths, agent_group, call),
        FILE_EDIT => handle_file_edit(paths, agent_group, call),
        other => anyhow::bail!("unknown file tool: {other}"),
    }
}

fn handle_file_read(
    paths: &BobaPaths,
    agent_group: &str,
    call: &ToolCall,
) -> anyhow::Result<String> {
    let args: FileReadArgs = serde_json::from_str(&call.function.arguments)
        .map_err(|e| anyhow::anyhow!("invalid file_read arguments: {e}"))?;
    let workspace = paths.group_workspace(agent_group);
    let target = resolve_in_workspace(&workspace, &args.path)?;
    if !target.is_file() {
        anyhow::bail!("not a file: {}", args.path);
    }

    let content = std::fs::read_to_string(&target)?;
    let rel = validate_relative_path(&args.path)?;

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
        let hint = format!("truncated '{rel}' — use offset/limit for more");
        Ok(head_tail_with_hint(&output, MAX_READ_CHARS, &hint))
    } else {
        Ok(output)
    }
}

fn handle_file_write(
    paths: &BobaPaths,
    agent_group: &str,
    call: &ToolCall,
) -> anyhow::Result<String> {
    let args: FileWriteArgs = serde_json::from_str(&call.function.arguments)
        .map_err(|e| anyhow::anyhow!("invalid file_write arguments: {e}"))?;
    let workspace = paths.group_workspace(agent_group);
    let rel = validate_relative_path(&args.path)?;
    let target = resolve_in_workspace(&workspace, &rel)?;
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&target, &args.contents)?;
    Ok(format!("Wrote {} bytes to '{rel}'.", args.contents.len()))
}

fn handle_file_edit(
    paths: &BobaPaths,
    agent_group: &str,
    call: &ToolCall,
) -> anyhow::Result<String> {
    let args: FileEditArgs = serde_json::from_str(&call.function.arguments)
        .map_err(|e| anyhow::anyhow!("invalid file_edit arguments: {e}"))?;
    if args.old_string.is_empty() {
        anyhow::bail!("old_string must not be empty");
    }
    let workspace = paths.group_workspace(agent_group);
    let rel = validate_relative_path(&args.path)?;
    let target = resolve_in_workspace(&workspace, &rel)?;
    if !target.is_file() {
        anyhow::bail!("not a file: {rel}");
    }

    let content = std::fs::read_to_string(&target)?;
    let count = content.matches(&args.old_string).count();
    if count == 0 {
        anyhow::bail!("old_string not found in '{rel}'");
    }
    if count > 1 && !args.replace_all {
        anyhow::bail!(
            "old_string matches {count} times in '{rel}' — set replace_all=true or use a unique old_string"
        );
    }

    let updated = if args.replace_all {
        content.replace(&args.old_string, &args.new_string)
    } else {
        content.replacen(&args.old_string, &args.new_string, 1)
    };
    std::fs::write(&target, &updated)?;
    let replaced = if args.replace_all { count } else { 1 };
    Ok(format!("Replaced {replaced} occurrence(s) in '{rel}'."))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bobaclaw_provider::FunctionCallPayload;

    fn file_call(name: &str, args: serde_json::Value) -> ToolCall {
        ToolCall {
            id: "1".into(),
            kind: "function".into(),
            function: FunctionCallPayload {
                name: name.into(),
                arguments: args.to_string(),
            },
        }
    }

    fn test_paths(dir: &std::path::Path) -> BobaPaths {
        let home = dir.to_path_buf();
        BobaPaths {
            home: home.clone(),
            config: home.join("config.yaml"),
            state_db: home.join("state.db"),
            runs: home.join("runs"),
            workspace: home.join("workspace"),
        }
    }

    #[test]
    fn read_write_edit_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("workspace").join("home");
        std::fs::create_dir_all(&ws).unwrap();
        let paths = test_paths(dir.path());

        let write = file_call(
            FILE_WRITE,
            json!({"path": "notes.txt", "contents": "hello world"}),
        );
        handle_file_tool(&paths, "home", &write).unwrap();

        let read = file_call(FILE_READ, json!({"path": "notes.txt"}));
        assert_eq!(
            handle_file_tool(&paths, "home", &read).unwrap(),
            "hello world"
        );

        let edit = file_call(
            FILE_EDIT,
            json!({"path": "notes.txt", "old_string": "world", "new_string": "BobaClaw"}),
        );
        handle_file_tool(&paths, "home", &edit).unwrap();
        assert!(handle_file_tool(&paths, "home", &read)
            .unwrap()
            .contains("BobaClaw"));
    }
}
