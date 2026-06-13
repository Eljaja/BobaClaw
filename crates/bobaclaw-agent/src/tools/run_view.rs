use std::path::Path;

use bobaclaw_core::{head_tail_with_hint, BobaPaths};
use bobaclaw_provider::{FunctionSpec, ToolCall, ToolSpec};
use bobaclaw_state::RunLedger;
use serde::Deserialize;
use serde_json::json;
use sqlx::SqlitePool;

pub const RUN_VIEW: &str = "run_view";

const MAX_OUTPUT_CHARS: usize = 24_000;

pub fn run_view_tool_spec() -> ToolSpec {
    ToolSpec {
        kind: "function".into(),
        function: FunctionSpec {
            name: RUN_VIEW.into(),
            description: "Fetch full stdout/stderr for a past exec run by run_id. \
                Use when exec output was truncated."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "run_id": { "type": "string" },
                    "stream": {
                        "type": "string",
                        "enum": ["stdout", "stderr", "both"],
                        "description": "Which stream to return (default both)"
                    },
                    "grep": { "type": "string", "description": "Optional substring filter (case-sensitive)" }
                },
                "required": ["run_id"]
            }),
        },
    }
}

pub fn is_run_view_tool(name: &str) -> bool {
    name == RUN_VIEW
}

#[derive(Debug, Deserialize)]
struct RunViewArgs {
    run_id: String,
    #[serde(default = "default_stream")]
    stream: String,
    #[serde(default)]
    grep: Option<String>,
}

fn default_stream() -> String {
    "both".into()
}

pub async fn handle_run_view_tool(
    paths: &BobaPaths,
    pool: &SqlitePool,
    agent_group: &str,
    call: &ToolCall,
) -> anyhow::Result<String> {
    let args: RunViewArgs = serde_json::from_str(&call.function.arguments)
        .map_err(|e| anyhow::anyhow!("invalid run_view arguments: {e}"))?;

    let run_id = args.run_id.trim();
    if run_id.is_empty() {
        anyhow::bail!("run_id must not be empty");
    }

    let ledger = RunLedger::new(pool);
    let record = ledger
        .get_run(run_id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("run not found: {run_id}"))?;

    ledger
        .verify_run_agent_group(run_id, agent_group)
        .await?
        .ok_or_else(|| anyhow::anyhow!("run {run_id} is not accessible for this agent group"))?;

    let capsule_dir = if let Some(ref dir) = record.capsule_dir {
        let p = Path::new(dir);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            paths.run_dir(run_id)
        }
    } else {
        paths.run_dir(run_id)
    };

    let stdout_path = capsule_dir.join("stdout.log");
    let stderr_path = capsule_dir.join("stderr.log");

    let mut parts = Vec::new();
    match args.stream.as_str() {
        "stdout" => {
            if let Some(body) = read_stream(&stdout_path, args.grep.as_deref())? {
                parts.push(format!("--- stdout ---\n{body}"));
            }
        }
        "stderr" => {
            if let Some(body) = read_stream(&stderr_path, args.grep.as_deref())? {
                parts.push(format!("--- stderr ---\n{body}"));
            }
        }
        _ => {
            if let Some(body) = read_stream(&stdout_path, args.grep.as_deref())? {
                parts.push(format!("--- stdout ---\n{body}"));
            }
            if let Some(body) = read_stream(&stderr_path, args.grep.as_deref())? {
                parts.push(format!("--- stderr ---\n{body}"));
            }
        }
    }

    if parts.is_empty() {
        return Ok(format!(
            "run_id: {run_id}\nexit_code: {:?}\n(no matching output)",
            record.exit_code
        ));
    }

    let mut body = parts.join("\n\n");
    if body.chars().count() > MAX_OUTPUT_CHARS {
        let hint = format!("truncated run_view for {run_id} — narrow with grep or stream");
        body = head_tail_with_hint(&body, MAX_OUTPUT_CHARS, &hint);
    }

    Ok(format!(
        "run_id: {run_id}\nexit_code: {:?}\n\n{body}",
        record.exit_code
    ))
}

fn read_stream(path: &Path, grep: Option<&str>) -> anyhow::Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    let content = std::fs::read_to_string(path)?;
    let filtered = if let Some(g) = grep {
        if g.is_empty() {
            content
        } else {
            content
                .lines()
                .filter(|line| line.contains(g))
                .collect::<Vec<_>>()
                .join("\n")
        }
    } else {
        content
    };
    if filtered.is_empty() {
        Ok(None)
    } else {
        Ok(Some(filtered))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grep_filters_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("stdout.log");
        std::fs::write(&path, "alpha\nbeta\ngamma\n").unwrap();
        let out = read_stream(&path, Some("beta")).unwrap().unwrap();
        assert_eq!(out, "beta");
    }
}
