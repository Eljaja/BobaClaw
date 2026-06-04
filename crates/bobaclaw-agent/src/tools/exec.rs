use crate::progress::{emit, AgentEvent, AgentProgress};
use bobaclaw_core::{head_tail_with_hint, BobaConfig, BobaPaths, CommandCapsuleManifest};
use bobaclaw_executor::{BwrapExecutor, ExecutorProfile};
use bobaclaw_provider::{FunctionSpec, ToolCall, ToolSpec};
use bobaclaw_state::RunLedger;
use serde::Deserialize;
use serde_json::json;
use sqlx::SqlitePool;
use std::path::PathBuf;
use uuid::Uuid;

const TOOL_NAME: &str = "exec";
/// Above this, API gets head+tail; full stdout/stderr stay in the run capsule on disk.
const MAX_TOOL_BODY_CHARS: usize = 24_000;

pub fn exec_tool_spec() -> ToolSpec {
    ToolSpec {
        kind: "function".into(),
        function: FunctionSpec {
            name: TOOL_NAME.into(),
            description: "Run a shell command in the agent workspace (bubblewrap sandbox). \
                Use for file inspection, builds, git, scripts, and system checks. \
                Call this tool instead of telling the user to run commands. \
                Only report stdout/stderr/exit_code from this tool's result."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "Shell command to run (bash -lc)."
                    },
                    "workdir": {
                        "type": "string",
                        "description": "Optional path relative to the agent workspace (default: workspace root)."
                    }
                },
                "required": ["command"]
            }),
        },
    }
}

#[derive(Debug, Deserialize)]
struct ExecArgs {
    command: String,
    #[serde(default)]
    workdir: Option<String>,
}

pub struct ExecToolResult {
    pub body: String,
    pub run_id: String,
    pub exit_code: i32,
}

pub async fn handle_exec_tool(
    paths: &BobaPaths,
    config: &BobaConfig,
    agent_group: &str,
    pool: &SqlitePool,
    session_id: &str,
    request_id: &str,
    call: &ToolCall,
    progress: Option<&dyn AgentProgress>,
) -> anyhow::Result<ExecToolResult> {
    if call.function.name != TOOL_NAME {
        anyhow::bail!("unknown tool: {}", call.function.name);
    }

    let args: ExecArgs = serde_json::from_str(&call.function.arguments)
        .map_err(|e| anyhow::anyhow!("invalid exec arguments: {e}"))?;

    let command = args.command.trim();
    if command.is_empty() {
        anyhow::bail!("exec: command is empty");
    }

    let workspace = paths.group_workspace(agent_group);
    std::fs::create_dir_all(&workspace)?;

    let workdir = args
        .workdir
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(".");

    let full_command = if workdir == "." {
        command.to_string()
    } else {
        let wd = sanitize_workdir(workdir)?;
        format!("cd {wd} && {command}")
    };

    let run_id = format!("run_{}", Uuid::new_v4());
    let run_dir = paths.run_dir(&run_id);
    let profile = ExecutorProfile::from_config(
        config.executor.network,
        config.executor.sandbox_packages,
    );
    let ledger = RunLedger::new(pool);

    ledger
        .create_run(
            &run_id,
            Some(session_id),
            Some(request_id),
            profile.id(),
        )
        .await?;
    ledger
        .set_capsule_dir(&run_id, &run_dir.display().to_string())
        .await?;
    ledger.mark_started(&run_id).await?;

    emit(
        progress,
        AgentEvent::ToolStart {
            name: TOOL_NAME.into(),
            label: truncate_label(&full_command, 72),
        },
    );

    let manifest = CommandCapsuleManifest {
        language: "bash".into(),
        argv: vec!["/bin/bash".into(), "-lc".into(), full_command.clone()],
        executor_profile: profile.id().into(),
        timeout_secs: 120,
        network: config.executor.network,
    };
    let _ = manifest;

    let result = BwrapExecutor::exec_command(&profile, &workspace, &run_dir, &full_command);

    match result {
        Ok(exec) => {
            ledger
                .mark_completed(&run_id, exec.exit_code, &exec.summary)
                .await?;
            let body = format_tool_result(
                &full_command,
                &workspace,
                &run_dir,
                exec.exit_code,
                &exec.summary,
            );
            emit(
                progress,
                AgentEvent::ToolEnd {
                    name: TOOL_NAME.into(),
                    exit_code: exec.exit_code,
                    preview: preview_output(&exec.summary),
                },
            );
            Ok(ExecToolResult {
                body,
                run_id,
                exit_code: exec.exit_code,
            })
        }
        Err(e) => {
            ledger.mark_denied(&run_id, &e.to_string()).await?;
            emit(
                progress,
                AgentEvent::ToolEnd {
                    name: TOOL_NAME.into(),
                    exit_code: 1,
                    preview: truncate_label(&e.to_string(), 60),
                },
            );
            Ok(ExecToolResult {
                body: format!("exec failed: {e}"),
                run_id,
                exit_code: 1,
            })
        }
    }
}

fn truncate_label(s: &str, max: usize) -> String {
    let one_line = s.replace('\n', " ");
    if one_line.chars().count() <= max {
        one_line
    } else {
        let mut t: String = one_line.chars().take(max.saturating_sub(1)).collect();
        t.push('…');
        t
    }
}

fn preview_output(summary: &str) -> String {
    summary
        .lines()
        .find(|l| !l.trim().is_empty())
        .map(|l| truncate_label(l.trim(), 56))
        .unwrap_or_else(|| "ok".into())
}

fn sanitize_workdir(workdir: &str) -> anyhow::Result<String> {
    let p = PathBuf::from(workdir);
    if p.is_absolute() {
        anyhow::bail!("workdir must be relative to the workspace");
    }
    for comp in p.components() {
        use std::path::Component;
        match comp {
            Component::ParentDir => anyhow::bail!("workdir cannot contain .."),
            Component::RootDir | Component::Prefix(_) => {
                anyhow::bail!("workdir must be relative")
            }
            _ => {}
        }
    }
    Ok(workdir.to_string())
}

fn format_tool_result(
    command: &str,
    workspace: &std::path::Path,
    run_dir: &std::path::Path,
    exit_code: i32,
    summary: &str,
) -> String {
    let artifact = format!(
        "full output in {}",
        run_dir.join("result.json").display()
    );
    let output = if summary.chars().count() > MAX_TOOL_BODY_CHARS {
        head_tail_with_hint(summary, MAX_TOOL_BODY_CHARS, &artifact)
    } else {
        summary.to_string()
    };
    format!(
        "command: {command}\nworkspace: {}\nrun_dir: {}\nexit_code: {exit_code}\n\n{output}",
        workspace.display(),
        run_dir.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_workdir_rejects_parent() {
        assert!(sanitize_workdir("../etc").is_err());
    }

    #[test]
    fn sanitize_workdir_allows_relative() {
        assert_eq!(sanitize_workdir("src").unwrap(), "src");
    }

    #[test]
    fn format_tool_result_includes_exit_code() {
        let dir = tempfile::tempdir().unwrap();
        let out = format_tool_result("echo hi", dir.path(), dir.path(), 0, "hi\n");
        assert!(out.contains("exit_code: 0"));
        assert!(out.contains("run_dir:"));
    }
}
