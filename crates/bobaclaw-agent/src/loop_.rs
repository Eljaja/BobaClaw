use std::sync::Arc;

use bobaclaw_core::{BobaConfig, BobaPaths, NormalizedRequest};
use tokio_util::sync::CancellationToken;
use bobaclaw_mcp::McpHub;
use bobaclaw_skills::SkillRegistry;
use bobaclaw_state::{SessionStore, StateDb};

use crate::progress::AgentProgress;
use crate::review::{maybe_post_turn_skill_save, SkillSaveSource, TurnSkillMetrics};
use crate::turn::run_agent_turn;

#[derive(Debug, Clone)]
pub struct AgentResponse {
    pub text: String,
    pub session_id: String,
    pub run_id: Option<String>,
    pub executed: bool,
    /// Turn was cancelled (Ctrl+C, `/stop`, or a new message for the same scope).
    pub interrupted: bool,
    /// Skill auto-saved after a tool-heavy turn (background review or forge fallback).
    pub auto_saved_skill: Option<String>,
}

pub struct AgentLoop {
    paths: BobaPaths,
    config: BobaConfig,
    state: StateDb,
    skills: SkillRegistry,
    mcp: Arc<McpHub>,
}

impl AgentLoop {
    pub async fn new(paths: BobaPaths, config: BobaConfig) -> anyhow::Result<Self> {
        let state = StateDb::open(&paths.state_db).await?;
        let group = &config.default_agent_group;
        let skills = SkillRegistry::load_enabled(&paths.group_workspace(group))?;
        let mcp = Arc::new(McpHub::connect(&config.mcp_servers).await);
        Ok(Self {
            paths,
            config,
            state,
            skills,
            mcp,
        })
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

        let skills =
            SkillRegistry::load_enabled(&self.paths.group_workspace(&req.agent_group))?;

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
        )
        .await?;

        let mut reply_text = outcome.text.clone();
        let auto_saved_skill = if outcome.interrupted {
            None
        } else {
            maybe_post_turn_skill_save(
            &self.paths,
            &self.config,
            &self.state,
            &req.agent_group,
            &TurnSkillMetrics {
                tool_call_count: outcome.tool_call_count,
                skill_manage_used: outcome.skill_manage_used,
            },
            &outcome.review_snapshot,
            outcome.last_run_id.as_deref(),
        )
        .await
        .map(|saved| {
            let note = match saved.source {
                SkillSaveSource::BackgroundReview => {
                    format!(
                        "\n\n💾 Saved skill `{}` for reuse (background review).",
                        saved.skill_name
                    )
                }
                SkillSaveSource::ForgeAutoPromote => {
                    format!(
                        "\n\n💾 Saved skill `{}` for reuse (from successful run).",
                        saved.skill_name
                    )
                }
            };
            reply_text.push_str(&note);
            saved.skill_name
        })
        };

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
        })
    }
}
