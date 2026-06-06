use crate::progress::{emit, sanitize_status_text, AgentEvent, AgentProgress};
use bobaclaw_core::{head_tail_with_hint, BobaConfig, BobaPaths, CommandCapsuleManifest};
use bobaclaw_executor::{ExecutorProfile, SandboxExecutor};
use bobaclaw_provider::{FunctionSpec, ToolCall, ToolSpec};
use bobaclaw_state::RunLedger;
use serde::Deserialize;
use serde_json::json;
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use uuid::Uuid;

const TOOL_NAME: &str = "exec";
/// Above this, API gets head+tail; full stdout/stderr stay in the run capsule on disk.
const MAX_TOOL_BODY_CHARS: usize = 24_000;

pub fn exec_tool_spec() -> ToolSpec {
    ToolSpec {
        kind: "function".into(),
        function: FunctionSpec {
            name: TOOL_NAME.into(),
            description: "Run a shell command in the agent workspace (sandboxed executor). \
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
                        "description": "Optional subdirectory relative to the workspace (e.g. \"src\"). \
                            Use \".\" or omit for workspace root — never pass absolute paths like /workspace."
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

    let args: ExecArgs = match serde_json::from_str(&call.function.arguments) {
        Ok(a) => a,
        Err(e) => {
            return Ok(validation_error(
                progress,
                &format!("invalid exec arguments: {e}"),
            ));
        }
    };

    let command = args.command.trim();
    if command.is_empty() {
        return Ok(validation_error(progress, "exec: command is empty"));
    }

    let workspace = paths.group_workspace(agent_group);
    std::fs::create_dir_all(&workspace)?;

    let workdir = args
        .workdir
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(".");

    let wd = match normalize_workdir(workdir, &workspace) {
        Ok(w) => w,
        Err(e) => return Ok(validation_error(progress, &e.to_string())),
    };

    let full_command = if wd == "." {
        command.to_string()
    } else {
        format!("cd {wd} && {command}")
    };

    let run_id = format!("run_{}", Uuid::new_v4());
    let run_dir = paths.run_dir(&run_id);
    let profile = ExecutorProfile::from_config_with_backend(
        config.executor.backend,
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

    let result = SandboxExecutor::exec_command(
        &config.executor,
        &profile,
        &paths.workspace,
        &workspace,
        &run_dir,
        &full_command,
    );

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
    let lines: Vec<&str> = summary
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .take(6)
        .collect();
    if lines.is_empty() {
        return "ok".into();
    }
    let joined = lines
        .iter()
        .map(|line| sanitize_status_text(line, 120))
        .collect::<Vec<_>>()
        .join("\n");
    truncate_preview(&joined, 280)
}

fn truncate_preview(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

fn validation_error(progress: Option<&dyn AgentProgress>, message: &str) -> ExecToolResult {
    emit(
        progress,
        AgentEvent::ToolEnd {
            name: TOOL_NAME.into(),
            exit_code: 1,
            preview: truncate_label(message, 60),
        },
    );
    ExecToolResult {
        body: format!("exec rejected: {message}"),
        run_id: String::new(),
        exit_code: 1,
    }
}

/// Accept relative paths; normalize common agent mistakes (`/workspace`, host absolute under workspace).
fn normalize_workdir(workdir: &str, workspace: &Path) -> anyhow::Result<String> {
    let w = workdir.trim();
    if w.is_empty() || w == "." {
        return Ok(".".to_string());
    }

    if w == "/workspace" || w.starts_with("/workspace/") {
        let rel = w.strip_prefix("/workspace").unwrap_or(w).trim_start_matches('/');
        return if rel.is_empty() {
            Ok(".".to_string())
        } else {
            sanitize_relative_workdir(rel)
        };
    }

    let p = PathBuf::from(w);
    if p.is_absolute() {
        if let (Ok(canon_ws), Ok(canon_p)) = (workspace.canonicalize(), p.canonicalize()) {
            if let Ok(rel) = canon_p.strip_prefix(&canon_ws) {
                let rel = rel.to_str().unwrap_or(".").trim_start_matches('/');
                return if rel.is_empty() {
                    Ok(".".to_string())
                } else {
                    sanitize_relative_workdir(rel)
                };
            }
        }
        anyhow::bail!(
            "workdir must be relative to the workspace (got {w:?}); use \".\" for workspace root"
        );
    }

    sanitize_relative_workdir(w)
}

fn sanitize_relative_workdir(workdir: &str) -> anyhow::Result<String> {
    let p = PathBuf::from(workdir);
    for comp in p.components() {
        use std::path::Component;
        match comp {
            Component::ParentDir => anyhow::bail!("workdir cannot contain .."),
            Component::RootDir | Component::Prefix(_) => {
                anyhow::bail!("workdir must be relative to the workspace")
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
        assert!(sanitize_relative_workdir("../etc").is_err());
    }

    #[test]
    fn sanitize_workdir_allows_relative() {
        assert_eq!(sanitize_relative_workdir("src").unwrap(), "src");
    }

    #[test]
    fn normalize_workdir_maps_sandbox_root() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(normalize_workdir("/workspace", dir.path()).unwrap(), ".");
        assert_eq!(
            normalize_workdir("/workspace/src", dir.path()).unwrap(),
            "src"
        );
    }

    #[test]
    fn normalize_workdir_maps_host_absolute() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("pkg");
        std::fs::create_dir_all(&sub).unwrap();
        let abs = sub.canonicalize().unwrap();
        assert_eq!(
            normalize_workdir(abs.to_str().unwrap(), dir.path()).unwrap(),
            "pkg"
        );
    }

    #[test]
    fn format_tool_result_includes_exit_code() {
        let dir = tempfile::tempdir().unwrap();
        let out = format_tool_result("echo hi", dir.path(), dir.path(), 0, "hi\n");
        assert!(out.contains("exit_code: 0"));
        assert!(out.contains("run_dir:"));
    }
}
