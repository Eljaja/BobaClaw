use std::sync::Arc;

use async_trait::async_trait;
use bobaclaw_agent::{AgentDispatcher, AgentProgress, AgentResponse};
use bobaclaw_core::NormalizedRequest;
use sqlx::SqlitePool;

use crate::auth::WebAuth;

/// The slice of [`AgentDispatcher`] the web channel needs. A trait so router tests can
/// run without an LLM provider.
#[async_trait]
pub trait TurnRunner: Send + Sync + 'static {
    async fn run_turn(
        &self,
        req: NormalizedRequest,
        progress: &dyn AgentProgress,
    ) -> anyhow::Result<AgentResponse>;

    async fn interrupt_session(&self, session_id: &str) -> bool;
}

#[async_trait]
impl TurnRunner for AgentDispatcher {
    async fn run_turn(
        &self,
        req: NormalizedRequest,
        progress: &dyn AgentProgress,
    ) -> anyhow::Result<AgentResponse> {
        self.handle_with_progress(req, Some(progress)).await
    }

    async fn interrupt_session(&self, session_id: &str) -> bool {
        AgentDispatcher::interrupt_session(self, session_id).await
    }
}

/// Shared state for the web routes.
#[derive(Clone)]
pub struct WebState {
    pub(crate) runner: Arc<dyn TurnRunner>,
    pub(crate) pool: SqlitePool,
    pub(crate) agent_group: String,
    pub(crate) title: String,
    pub(crate) auth: Arc<WebAuth>,
}

impl WebState {
    pub fn new(
        runner: Arc<dyn TurnRunner>,
        pool: SqlitePool,
        agent_group: impl Into<String>,
        title: impl Into<String>,
        auth: WebAuth,
    ) -> Self {
        Self {
            runner,
            pool,
            agent_group: agent_group.into(),
            title: title.into(),
            auth: Arc::new(auth),
        }
    }
}
