use std::path::PathBuf;
use std::time::Duration;

use bobaclaw_core::{head_tail_with_hint, BobaConfig, BobaPaths, TurnInterrupted};
use bobaclaw_executor::{ExecutorProfile, SandboxExecutor};
use bobaclaw_state::RunLedger;
use sqlx::SqlitePool;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::progress::{emit, AgentEvent, AgentProgress};
use crate::subagent::SubagentRunResult;

pub async fn run_claude_code(
    paths: &BobaPaths,
    config: &BobaConfig,
    pool: &SqlitePool,
    agent_group: &str,
    session_id: &str,
    task: &str,
    context: Option<&str>,
    cancel: &CancellationToken,
    progress: Option<&dyn AgentProgress>,
) -> anyhow::Result<SubagentRunResult> {
    let cfg = &config.subagents.backends.claude_code;
    let prompt = build_cli_prompt(task, context);
    let escaped = shell_escape(&prompt);
    let command = format!(
        "{} --bare -p {} --output-format json --max-turns {}",
        cfg.command, escaped, cfg.max_turns
    );
    run_external_command(
        paths,
        config,
        pool,
        agent_group,
        session_id,
        &command,
        &cfg.api_key_env,
        cfg.timeout_secs,
        cancel,
        progress,
        "claude-code",
    )
    .await
}

pub async fn run_codex(
    paths: &BobaPaths,
    config: &BobaConfig,
    pool: &SqlitePool,
    agent_group: &str,
    session_id: &str,
    task: &str,
    context: Option<&str>,
    cancel: &CancellationToken,
    progress: Option<&dyn AgentProgress>,
) -> anyhow::Result<SubagentRunResult> {
    let cfg = &config.subagents.backends.codex;
    let prompt = build_cli_prompt(task, context);
    let escaped = shell_escape(&prompt);
    let command = format!(
        "{} exec --sandbox {} --json {}",
        cfg.command, cfg.sandbox, escaped
    );
    run_external_command(
        paths,
        config,
        pool,
        agent_group,
        session_id,
        &command,
        &cfg.api_key_env,
        cfg.timeout_secs,
        cancel,
        progress,
        "codex",
    )
    .await
}

pub async fn run_cursor_local(
    paths: &BobaPaths,
    config: &BobaConfig,
    pool: &SqlitePool,
    agent_group: &str,
    session_id: &str,
    task: &str,
    context: Option<&str>,
    cancel: &CancellationToken,
    progress: Option<&dyn AgentProgress>,
) -> anyhow::Result<SubagentRunResult> {
    let cfg = &config.subagents.backends.cursor;
    let workspace = paths.group_workspace(agent_group);
    let wrapper = resolve_wrapper_script();
    let prompt = build_cli_prompt(task, context);
    let command = format!(
        "{} {} --workspace {} --model {} --task {}",
        cfg.wrapper_command,
        shell_escape(&wrapper.display().to_string()),
        shell_escape(&workspace.display().to_string()),
        shell_escape(&cfg.model),
        shell_escape(&prompt)
    );
    run_external_command(
        paths,
        config,
        pool,
        agent_group,
        session_id,
        &command,
        &cfg.api_key_env,
        cfg.timeout_secs,
        cancel,
        progress,
        "cursor",
    )
    .await
}

fn resolve_wrapper_script() -> PathBuf {
    if let Ok(manifest) = std::env::var("CARGO_MANIFEST_DIR") {
        let p = PathBuf::from(manifest).join("../../scripts/cursor-subagent-wrapper.py");
        if p.exists() {
            return p;
        }
    }
    PathBuf::from("scripts/cursor-subagent-wrapper.py")
}

fn build_cli_prompt(task: &str, context: Option<&str>) -> String {
    match context.filter(|s| !s.trim().is_empty()) {
        Some(ctx) => format!("{task}\n\nContext:\n{ctx}"),
        None => task.to_string(),
    }
}

fn shell_escape(s: &str) -> String {
    let mut out = String::from("'");
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

#[allow(clippy::too_many_arguments)]
async fn run_external_command(
    paths: &BobaPaths,
    config: &BobaConfig,
    pool: &SqlitePool,
    agent_group: &str,
    session_id: &str,
    command: &str,
    api_key_env: &str,
    timeout_secs: u64,
    cancel: &CancellationToken,
    progress: Option<&dyn AgentProgress>,
    label: &str,
) -> anyhow::Result<SubagentRunResult> {
    let workspace = paths.group_workspace(agent_group);
    std::fs::create_dir_all(&workspace)?;

    let run_id = format!("subagent_cli_{}", Uuid::new_v4());
    let run_dir = paths.run_dir(&run_id);
    let profile = ExecutorProfile::from_config_with_backend(
        config.executor.backend,
        config.executor.network,
        config.executor.sandbox_packages,
    );
    let ledger = RunLedger::new(pool);
    ledger
        .create_run(&run_id, Some(session_id), None, profile.id())
        .await?;
    ledger
        .set_capsule_dir(&run_id, &run_dir.display().to_string())
        .await?;
    ledger.mark_started(&run_id).await?;

    emit(
        progress,
        AgentEvent::ToolStart {
            name: format!("subagent-{label}"),
            label: truncate_label(command, 72),
        },
    );

    let api_key = std::env::var(api_key_env)
        .map_err(|_| anyhow::anyhow!("missing env {api_key_env} for subagent backend {label}"))?;
    let full_command = format!(
        "export {}={} && {}",
        api_key_env,
        shell_escape(&api_key),
        command
    );

    let executor_cfg = config.executor.clone();
    let profile_clone = profile.clone();
    let paths_workspace = paths.workspace.clone();
    let workspace_clone = workspace.clone();
    let run_dir_clone = run_dir.clone();
    let timeout = timeout_secs.max(1);

    let exec_fut = tokio::task::spawn_blocking(move || {
        SandboxExecutor::exec_command(
            &executor_cfg,
            &profile_clone,
            &paths_workspace,
            &workspace_clone,
            &run_dir_clone,
            &full_command,
        )
    });

    let result = tokio::select! {
        _ = cancel.cancelled() => {
            return Err(TurnInterrupted.into());
        }
        res = tokio::time::timeout(Duration::from_secs(timeout), exec_fut) => {
            match res {
                Ok(Ok(r)) => r.map_err(|e| anyhow::anyhow!("subagent exec: {e}"))?,
                Ok(Err(e)) => return Err(anyhow::anyhow!("subagent exec join: {e}")),
                Err(_) => return Err(anyhow::anyhow!("subagent backend timed out after {timeout}s")),
            }
        }
    };

    let exec = result;
    let preview = head_tail_with_hint(&exec.summary, 500, "subagent output truncated");
    emit(
        progress,
        AgentEvent::ToolEnd {
            name: format!("subagent-{label}"),
            exit_code: exec.exit_code,
            preview,
        },
    );
    let stdout = std::fs::read_to_string(&exec.stdout_path).unwrap_or_default();
    let stderr = std::fs::read_to_string(&exec.stderr_path).unwrap_or_default();
    let body = if stdout.trim().is_empty() {
        format!("exit_code={}\nstderr:\n{}", exec.exit_code, stderr.trim())
    } else {
        stdout
    };
    ledger
        .mark_completed(&run_id, exec.exit_code, &truncate_label(&body, 200))
        .await?;
    Ok(SubagentRunResult {
        body,
        exit_code: exec.exit_code,
        subagent_id: run_id,
    })
}

fn truncate_label(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}
