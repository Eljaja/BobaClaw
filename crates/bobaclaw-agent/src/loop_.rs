use bobaclaw_core::{BobaConfig, BobaPaths, NormalizedRequest};
use bobaclaw_skills::SkillRegistry;
use bobaclaw_state::{SessionStore, StateDb};

use crate::progress::AgentProgress;
use crate::turn::run_agent_turn;

#[derive(Debug, Clone)]
pub struct AgentResponse {
    pub text: String,
    pub session_id: String,
    pub run_id: Option<String>,
    pub executed: bool,
}

pub struct AgentLoop {
    paths: BobaPaths,
    config: BobaConfig,
    state: StateDb,
    skills: SkillRegistry,
}

impl AgentLoop {
    pub async fn new(paths: BobaPaths, config: BobaConfig) -> anyhow::Result<Self> {
        let state = StateDb::open(&paths.state_db).await?;
        let group = &config.default_agent_group;
        let skills = SkillRegistry::load(&paths.group_workspace(group))?;
        Ok(Self {
            paths,
            config,
            state,
            skills,
        })
    }

    pub async fn handle(&self, req: NormalizedRequest) -> anyhow::Result<AgentResponse> {
        self.handle_with_progress(req, None).await
    }

    pub async fn handle_with_progress(
        &self,
        req: NormalizedRequest,
        progress: Option<&dyn AgentProgress>,
    ) -> anyhow::Result<AgentResponse> {
        let pool = self.state.pool();
        let sessions = SessionStore::new(pool);
        let session_id = sessions.resolve_session(&req).await?;
        sessions
            .append_message(&session_id, "user", &req.user_text)
            .await?;

        let skills =
            SkillRegistry::load(&self.paths.group_workspace(&req.agent_group))?;

        let outcome = run_agent_turn(
            &self.paths,
            &self.config,
            pool,
            &skills,
            &session_id,
            &req,
            progress,
        )
        .await?;

        sessions
            .append_message(&session_id, "assistant", &outcome.text)
            .await?;

        Ok(AgentResponse {
            text: outcome.text,
            session_id,
            run_id: outcome.last_run_id,
            executed: outcome.executed,
        })
    }
}
