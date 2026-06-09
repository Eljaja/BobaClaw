mod backends;
mod spawn_queue;

use std::sync::Arc;
use std::time::Duration;

use bobaclaw_core::{BobaConfig, BobaPaths, NormalizedRequest};
use bobaclaw_mcp::McpHub;
use bobaclaw_provider::ConversationMessage;
use bobaclaw_provider::ToolChatClient;
use bobaclaw_skills::SkillRegistry;
use bobaclaw_state::{RunLedger, SpawnJobStore};
use sqlx::SqlitePool;
use tokio::sync::{RwLock, Semaphore};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::progress::{emit, AgentEvent, AgentProgress};
use crate::prompt::{build_subagent_system_prompt, load_subagent_task_prefix};
use crate::spawn_completer::SpawnCompleter;
use crate::tool_loop::run_tool_loop;
use crate::tools::build_child_tool_specs;
use crate::turn_context::{TurnContext, TurnMode};

pub use spawn_queue::format_spawn_task_list;

pub struct SubagentRunResult {
    pub body: String,
    pub exit_code: i32,
    pub subagent_id: String,
}

pub struct SubagentManager {
    paths: BobaPaths,
    config: BobaConfig,
    semaphore: Arc<Semaphore>,
    completer: Arc<RwLock<Option<Arc<SpawnCompleter>>>>,
}

impl SubagentManager {
    pub fn new(paths: BobaPaths, config: BobaConfig) -> Self {
        let max = config.subagents.max_concurrent.max(1);
        Self {
            paths,
            config,
            semaphore: Arc::new(Semaphore::new(max)),
            completer: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn set_completer(&self, completer: Arc<SpawnCompleter>) {
        *self.completer.write().await = Some(completer);
    }

    pub async fn run_sync(
        &self,
        pool: &SqlitePool,
        mcp: Option<&Arc<McpHub>>,
        session_id: &str,
        req: &NormalizedRequest,
        turn_ctx: &TurnContext,
        task: &str,
        label: Option<&str>,
        context: Option<&str>,
        preset: Option<&str>,
        backend: Option<&str>,
        progress: Option<&dyn AgentProgress>,
        cancel: &CancellationToken,
        spawn_job_id: Option<&str>,
    ) -> anyhow::Result<SubagentRunResult> {
        if !self.config.subagents.enabled {
            anyhow::bail!("subagents are disabled in config (subagents.enabled: false)");
        }
        if turn_ctx.delegation_depth >= self.config.subagents.max_depth {
            anyhow::bail!(
                "nested subagents are not allowed (max_depth={})",
                self.config.subagents.max_depth
            );
        }

        let backend_name = backend
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(self.config.subagents.default_backend.as_str());

        let _permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| anyhow::anyhow!("subagent manager shut down"))?;

        let subagent_id = format!("subagent_{}", Uuid::new_v4());
        emit(
            progress,
            AgentEvent::SubagentStart {
                id: subagent_id.clone(),
                label: label.unwrap_or("subagent").to_string(),
            },
        );

        let ledger = RunLedger::new(pool);
        ledger
            .create_run(
                &subagent_id,
                Some(session_id),
                Some(&req.request_id.to_string()),
                &format!("subagent-{backend_name}"),
            )
            .await?;
        if let Err(e) = ledger.mark_started(&subagent_id).await {
            ledger
                .mark_denied(&subagent_id, &format!("mark_started failed: {e}"))
                .await
                .ok();
            return Err(e);
        }

        if let Some(job_id) = spawn_job_id {
            SpawnJobStore::new(pool)
                .link_subagent(job_id, &subagent_id)
                .await
                .ok();
        }

        let mut child_ctx = TurnContext::child(turn_ctx, label.map(str::to_string));
        child_ctx.run_id = Some(subagent_id.clone());
        let parent_run_id = child_ctx.parent_run_id.clone();

        let outcome = match backend_name {
            "native" => {
                self.run_native(
                    pool, mcp, session_id, req, &child_ctx, task, context, preset, progress, cancel,
                )
                .await
            }
            "claude-code" | "claude_code" => {
                if !self.config.subagents.backends.claude_code.enabled {
                    anyhow::bail!("claude-code subagent backend is disabled in config");
                }
                backends::run_claude_code(
                    &self.paths,
                    &self.config,
                    pool,
                    &req.agent_group,
                    session_id,
                    task,
                    context,
                    cancel,
                    progress,
                )
                .await
            }
            "codex" => {
                if !self.config.subagents.backends.codex.enabled {
                    anyhow::bail!("codex subagent backend is disabled in config");
                }
                backends::run_codex(
                    &self.paths,
                    &self.config,
                    pool,
                    &req.agent_group,
                    session_id,
                    task,
                    context,
                    cancel,
                    progress,
                )
                .await
            }
            "cursor" => {
                if !self.config.subagents.backends.cursor.enabled {
                    anyhow::bail!("cursor subagent backend is disabled in config");
                }
                backends::run_cursor_local(
                    &self.paths,
                    &self.config,
                    pool,
                    &req.agent_group,
                    session_id,
                    task,
                    context,
                    cancel,
                    progress,
                )
                .await
            }
            other => anyhow::bail!("unknown subagent backend: {other}"),
        };

        finalize_subagent_ledger(
            &ledger,
            &subagent_id,
            label,
            task,
            backend_name,
            parent_run_id.as_deref(),
            outcome,
            progress,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn run_native(
        &self,
        pool: &SqlitePool,
        mcp: Option<&Arc<McpHub>>,
        session_id: &str,
        req: &NormalizedRequest,
        child_ctx: &TurnContext,
        task: &str,
        context: Option<&str>,
        preset: Option<&str>,
        progress: Option<&dyn AgentProgress>,
        cancel: &CancellationToken,
    ) -> anyhow::Result<SubagentRunResult> {
        let skills = SkillRegistry::load_enabled(&self.paths.group_workspace(&req.agent_group))?;
        let preset_cfg = preset.and_then(|id| self.config.subagents.preset(id));
        let workspace = self.paths.group_workspace(&req.agent_group);
        let system = build_subagent_system_prompt(&self.paths, &workspace, preset_cfg);
        let task_prefix = load_subagent_task_prefix(&self.paths);
        let user_content =
            format_subagent_user_message(&task_prefix, task, context, preset_cfg, &skills);

        let mut messages = vec![
            ConversationMessage::system(system),
            ConversationMessage::user(user_content),
        ];

        let tools = build_child_tool_specs(mcp, preset_cfg);
        let api_key = self.config.resolve_api_key()?;
        let client = ToolChatClient::from_provider(&self.config.provider, api_key)?;

        let model_override =
            preset_cfg
                .and_then(|p| p.model.as_deref())
                .or(self.config.subagents.model.as_deref());

        let timeout = Duration::from_secs(self.config.subagents.child_timeout_seconds.max(30));
        let loop_fut = run_tool_loop(
            &self.paths,
            &self.config,
            pool,
            mcp,
            session_id,
            req,
            child_ctx,
            TurnMode::Child,
            &client,
            &tools,
            &mut messages,
            model_override,
            false,
            self.config.subagents.max_tool_iterations,
            progress,
            cancel,
            Some(self),
        );

        let outcome = tokio::time::timeout(timeout, loop_fut)
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "subagent timed out after {}s",
                    self.config.subagents.child_timeout_seconds
                )
            })??;

        if outcome.interrupted {
            anyhow::bail!("subagent interrupted");
        }

        let body = truncate_body(&outcome.final_text, self.config.subagents.result_max_chars);
        Ok(SubagentRunResult {
            body,
            exit_code: if outcome.executed || !outcome.final_text.trim().is_empty() {
                0
            } else {
                1
            },
            subagent_id: String::new(),
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn spawn_async(
        &self,
        pool: Arc<SqlitePool>,
        mcp: Arc<McpHub>,
        session_id: String,
        req: NormalizedRequest,
        turn_ctx: TurnContext,
        task: String,
        label: Option<String>,
        context: Option<String>,
        preset: Option<String>,
        backend: Option<String>,
        wake_parent: bool,
        cancel: CancellationToken,
    ) -> anyhow::Result<String> {
        let deliver_channel = req.spawn_deliver_channel().to_string();
        let (deliver_peer, deliver_thread_id) = req
            .channel_peer
            .as_ref()
            .map(|p| (Some(p.peer.clone()), p.thread_id.clone()))
            .unwrap_or((None, None));

        let job = SpawnJobStore::new(&pool)
            .insert_running(
                &session_id,
                &req.agent_group,
                req.ingress.as_str(),
                Some(&deliver_channel),
                deliver_peer.as_deref(),
                deliver_thread_id.as_deref(),
                label.as_deref(),
                truncate_preview(&task, 200).as_str(),
                backend.as_deref(),
                Some(&req.request_id.to_string()),
                wake_parent,
            )
            .await?;

        let task_id = job.id.clone();
        let manager = self.clone_inner();
        let label_for_reply = label.clone();
        let job_id = task_id.clone();
        let completer = self.completer.read().await.clone();

        let child_cancel = cancel.child_token();
        tokio::spawn(async move {
            let result = manager
                .run_sync(
                    &pool,
                    Some(&mcp),
                    &session_id,
                    &req,
                    &turn_ctx,
                    &task,
                    label.as_deref(),
                    context.as_deref(),
                    preset.as_deref(),
                    backend.as_deref(),
                    None,
                    &child_cancel,
                    Some(&job_id),
                )
                .await;

            let cancelled = child_cancel.is_cancelled();
            if let Some(completer) = completer {
                let _ = completer.on_complete(&job_id, &result, cancelled).await;
            } else if !cancelled {
                let store = SpawnJobStore::new(&pool);
                let (status, exit_code, preview) = match &result {
                    Ok(r) if r.exit_code == 0 => (
                        "completed",
                        Some(r.exit_code),
                        truncate_preview(&r.body, 200),
                    ),
                    Ok(r) => ("failed", Some(r.exit_code), truncate_preview(&r.body, 200)),
                    Err(e) => ("failed", None, truncate_preview(&e.to_string(), 200)),
                };
                let _ = store
                    .finalize(&job_id, status, exit_code, Some(&preview), None)
                    .await;
            }
        });

        Ok(format!(
            "Spawned background subagent `{}` (id: {task_id}). Use spawn_status to check progress; result will be delivered when complete.",
            label_for_reply.as_deref().unwrap_or("unnamed")
        ))
    }

    fn clone_inner(&self) -> Self {
        Self {
            paths: self.paths.clone(),
            config: self.config.clone(),
            semaphore: self.semaphore.clone(),
            completer: self.completer.clone(),
        }
    }
}

async fn finalize_subagent_ledger(
    ledger: &RunLedger<'_>,
    subagent_id: &str,
    label: Option<&str>,
    task: &str,
    backend_name: &str,
    parent_run_id: Option<&str>,
    outcome: anyhow::Result<SubagentRunResult>,
    progress: Option<&dyn AgentProgress>,
) -> anyhow::Result<SubagentRunResult> {
    match outcome {
        Ok(mut result) => {
            if result.subagent_id.is_empty() {
                result.subagent_id = subagent_id.to_string();
            }
            let summary = serde_json::json!({
                "label": label,
                "task": truncate_preview(task, 200),
                "backend": backend_name,
                "parent_run_id": parent_run_id,
            })
            .to_string();
            if let Err(e) = ledger
                .mark_completed(subagent_id, result.exit_code, &summary)
                .await
            {
                let msg = format!("mark_completed failed: {e}");
                ledger.mark_denied(subagent_id, &msg).await.ok();
                return Err(e);
            }
            emit(
                progress,
                AgentEvent::SubagentEnd {
                    id: subagent_id.to_string(),
                    exit_code: result.exit_code,
                    preview: truncate_preview(&result.body, 120),
                },
            );
            Ok(result)
        }
        Err(e) => {
            let msg = format!("subagent failed: {e}");
            ledger.mark_denied(subagent_id, &msg).await.ok();
            emit(
                progress,
                AgentEvent::SubagentEnd {
                    id: subagent_id.to_string(),
                    exit_code: 1,
                    preview: truncate_preview(&msg, 120),
                },
            );
            Ok(SubagentRunResult {
                body: msg,
                exit_code: 1,
                subagent_id: subagent_id.to_string(),
            })
        }
    }
}

fn format_subagent_user_message(
    task_prefix: &str,
    task: &str,
    context: Option<&str>,
    preset: Option<&bobaclaw_core::SubagentPreset>,
    skills: &SkillRegistry,
) -> String {
    let mut parts = vec![format!("{task_prefix}\n{task}")];
    if let Some(ctx) = context.filter(|s| !s.trim().is_empty()) {
        parts.push(format!("\nAdditional context from parent:\n{ctx}"));
    }
    if let Some(preset) = preset {
        if let Some(extra) = preset.system_extra.as_deref().filter(|s| !s.is_empty()) {
            parts.push(format!("\nPreset instructions:\n{extra}"));
        }
        for skill_name in &preset.skills {
            if let Some(skill) = skills.get(skill_name) {
                parts.push(format!("\nSkill `{skill_name}`:\n{}", skill.body));
            }
        }
    }
    parts.join("\n")
}

fn truncate_body(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max.saturating_sub(80)).collect();
    format!("{head}\n… (subagent result truncated; full output in run ledger capsule)")
}

fn truncate_preview(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}
