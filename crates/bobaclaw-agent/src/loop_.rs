use std::sync::Arc;

use bobaclaw_core::{BobaConfig, BobaPaths, NormalizedRequest};
use bobaclaw_mcp::McpHub;
use bobaclaw_skills::SkillRegistry;
use bobaclaw_state::{SessionStore, SpawnJobRecord, SpawnJobStore, StateDb};
use tokio_util::sync::CancellationToken;

use crate::progress::AgentProgress;
use crate::review::{maybe_post_turn_review, PostTurnSave, TurnReviewMetrics};
use crate::spawn_completer::SpawnCompleter;
use crate::subagent::SubagentManager;
use crate::turn::run_agent_turn;

#[derive(Debug, Clone)]
pub struct AgentResponse {
    pub text: String,
    pub session_id: String,
    pub run_id: Option<String>,
    pub executed: bool,
    /// Turn was cancelled (Ctrl+C, `/stop`, or a new message for the same scope).
    pub interrupted: bool,
    /// Skill auto-saved after a tool-heavy turn (background review).
    pub auto_saved_skill: Option<String>,
    /// Memory path appended after background memory review.
    pub auto_saved_memory: Option<String>,
}

pub struct AgentLoop {
    paths: BobaPaths,
    config: BobaConfig,
    state: StateDb,
    #[allow(dead_code)]
    skills: SkillRegistry,
    mcp: Arc<McpHub>,
    subagent: Arc<SubagentManager>,
}

impl AgentLoop {
    pub async fn new(paths: BobaPaths, config: BobaConfig) -> anyhow::Result<Self> {
        let state = StateDb::open(&paths.state_db).await?;
        let group = &config.default_agent_group;
        let skills = SkillRegistry::load_enabled(&paths.group_workspace(group))?;
        let mcp = Arc::new(McpHub::connect(&config.mcp_servers).await);
        let subagent = Arc::new(SubagentManager::new(paths.clone(), config.clone()));
        Ok(Self {
            paths,
            config,
            state,
            skills,
            mcp,
            subagent,
        })
    }

    pub fn pool(&self) -> &sqlx::SqlitePool {
        self.state.pool()
    }

    pub fn subagent(&self) -> &Arc<SubagentManager> {
        &self.subagent
    }

    pub async fn set_spawn_completer(&self, completer: Arc<SpawnCompleter>) {
        self.subagent.set_completer(completer).await;
    }

    pub async fn list_spawn_jobs(&self, session_id: &str) -> Vec<SpawnJobRecord> {
        SpawnJobStore::new(self.state.pool())
            .list_by_session(session_id)
            .await
            .unwrap_or_default()
    }

    pub async fn handle(&self, req: NormalizedRequest) -> anyhow::Result<AgentResponse> {
        self.handle_with_progress(req, None, CancellationToken::new())
            .await
    }

    pub async fn handle_with_progress(
        &self,
        req: NormalizedRequest,
        progress: Option<&dyn AgentProgress>,
        cancel: CancellationToken,
    ) -> anyhow::Result<AgentResponse> {
        let pool = self.state.pool();
        let sessions = SessionStore::new(pool);
        let session_id = sessions.resolve_session(&req).await?;
        let workspace = self.paths.group_workspace(&req.agent_group);
        let user_content = req.format_user_content(&workspace);
        sessions
            .append_message(&session_id, "user", &user_content)
            .await?;

        let user_message_count = sessions.count_user_messages(&session_id).await?;

        let skills = SkillRegistry::load_enabled(&self.paths.group_workspace(&req.agent_group))?;

        let outcome = run_agent_turn(
            &self.paths,
            &self.config,
            pool,
            &skills,
            Some(&self.mcp),
            &session_id,
            &req,
            progress,
            &cancel,
            Some(self.subagent.as_ref()),
        )
        .await?;

        let mut reply_text = outcome.text.clone();
        let mut auto_saved_skill = None;
        let mut auto_saved_memory = None;

        if !outcome.interrupted {
            let review = maybe_post_turn_review(
                &self.paths,
                &self.config,
                &req.agent_group,
                &TurnReviewMetrics {
                    tool_call_count: outcome.tool_call_count,
                    skill_manage_used: outcome.skill_manage_used,
                    memory_manage_used: outcome.memory_manage_used,
                    user_message_count,
                },
                &outcome.review_snapshot,
            )
            .await;

            if let Some(PostTurnSave::Memory { path }) = review.memory {
                reply_text.push_str(&format!(
                    "\n\nSaved to memory `{}` (background review).",
                    path
                ));
                auto_saved_memory = Some(path);
            }
            if let Some(PostTurnSave::Skill { name }) = review.skill {
                reply_text.push_str(&format!(
                    "\n\nSaved skill `{}` for reuse (background review).",
                    name
                ));
                auto_saved_skill = Some(name);
            }
        }

        sessions
            .append_message(&session_id, "assistant", &outcome.persisted_assistant)
            .await?;

        Ok(AgentResponse {
            text: reply_text,
            session_id,
            run_id: outcome.last_run_id,
            executed: outcome.executed,
            interrupted: outcome.interrupted,
            auto_saved_skill,
            auto_saved_memory,
        })
    }
}
