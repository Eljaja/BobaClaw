mod backends;
mod spawn_queue;

use std::sync::Arc;
use std::time::Duration;

use bobaclaw_core::{BobaConfig, BobaPaths, NormalizedRequest};
use bobaclaw_mcp::McpHub;
use bobaclaw_provider::ConversationMessage;
use bobaclaw_provider::ToolChatClient;
use bobaclaw_skills::SkillRegistry;
use bobaclaw_state::RunLedger;
use sqlx::SqlitePool;
use tokio::sync::{Mutex, Semaphore};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::progress::{emit, AgentEvent, AgentProgress};
use crate::prompt::{build_subagent_system_prompt, SUBAGENT_USER_TASK_PREFIX};
use crate::tool_loop::run_tool_loop;
use crate::tools::build_child_tool_specs;
use crate::turn_context::{TurnContext, TurnMode};

pub use spawn_queue::SpawnTaskRecord;

pub struct SubagentRunResult {
    pub body: String,
    pub exit_code: i32,
    pub subagent_id: String,
}

pub struct SubagentManager {
    paths: BobaPaths,
    config: BobaConfig,
    semaphore: Arc<Semaphore>,
    spawn_tasks: Arc<Mutex<Vec<SpawnTaskRecord>>>,
}

impl SubagentManager {
    pub fn new(paths: BobaPaths, config: BobaConfig) -> Self {
        let max = config.subagents.max_concurrent.max(1);
        Self {
            paths,
            config,
            semaphore: Arc::new(Semaphore::new(max)),
            spawn_tasks: Arc::new(Mutex::new(Vec::new())),
        }
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
        ledger.mark_started(&subagent_id).await?;

        let child_ctx = TurnContext::child(turn_ctx, label.map(str::to_string));
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

        let result = match outcome {
            Ok(mut r) => {
                if r.subagent_id.is_empty() {
                    r.subagent_id = subagent_id.clone();
                }
                r
            }
            Err(e) => {
                let msg = format!("subagent failed: {e}");
                ledger.mark_denied(&subagent_id, &msg).await.ok();
                emit(
                    progress,
                    AgentEvent::SubagentEnd {
                        id: subagent_id.clone(),
                        exit_code: 1,
                        preview: truncate_preview(&msg, 120),
                    },
                );
                return Ok(SubagentRunResult {
                    body: msg,
                    exit_code: 1,
                    subagent_id,
                });
            }
        };

        let summary = serde_json::json!({
            "label": label,
            "task": truncate_preview(task, 200),
            "backend": backend_name,
            "parent_run_id": turn_ctx.parent_run_id,
        })
        .to_string();
        ledger
            .mark_completed(&subagent_id, result.exit_code, &summary)
            .await?;

        emit(
            progress,
            AgentEvent::SubagentEnd {
                id: subagent_id.clone(),
                exit_code: result.exit_code,
                preview: truncate_preview(&result.body, 120),
            },
        );

        Ok(SubagentRunResult {
            body: result.body,
            exit_code: result.exit_code,
            subagent_id,
        })
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
        let system = build_subagent_system_prompt(&workspace, preset_cfg);
        let user_content = format_subagent_user_message(task, context, preset_cfg, &skills);

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
    ) -> anyhow::Result<String> {
        let task_id = format!("spawn_{}", Uuid::new_v4());
        let manager = self.clone_inner();
        let task_id_clone = task_id.clone();
        let session_for_delivery = session_id.clone();
        let label_for_reply = label.clone();

        {
            let mut tasks = self.spawn_tasks.lock().await;
            tasks.push(SpawnTaskRecord {
                id: task_id.clone(),
                label: label.clone(),
                status: "running".into(),
                result: None,
            });
        }

        tokio::spawn(async move {
            let cancel = CancellationToken::new();
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
                    &cancel,
                )
                .await;

            let (status, preview) = match &result {
                Ok(r) => ("completed".into(), truncate_preview(&r.body, 200)),
                Err(e) => ("failed".into(), truncate_preview(&e.to_string(), 200)),
            };

            {
                let mut tasks = manager.spawn_tasks.lock().await;
                if let Some(rec) = tasks.iter_mut().find(|t| t.id == task_id_clone) {
                    rec.status = status;
                    rec.result = Some(preview.clone());
                }
            }

            if let Ok(r) = result {
                let delivery = format!(
                    "[Subagent `{}` completed]\n\n{}",
                    label.as_deref().unwrap_or(&task_id_clone),
                    r.body
                );
                let sessions = bobaclaw_state::SessionStore::new(&pool);
                let _ = sessions
                    .append_message(&session_for_delivery, "assistant", &delivery)
                    .await;
            }
        });

        Ok(format!(
            "Spawned background subagent `{}` (id: {task_id}). Result will be appended to the session when complete.",
            label_for_reply.as_deref().unwrap_or("unnamed")
        ))
    }

    fn clone_inner(&self) -> Self {
        Self {
            paths: self.paths.clone(),
            config: self.config.clone(),
            semaphore: self.semaphore.clone(),
            spawn_tasks: self.spawn_tasks.clone(),
        }
    }
}

fn format_subagent_user_message(
    task: &str,
    context: Option<&str>,
    preset: Option<&bobaclaw_core::SubagentPreset>,
    skills: &SkillRegistry,
) -> String {
    let mut parts = vec![format!("{SUBAGENT_USER_TASK_PREFIX}\n{task}")];
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
