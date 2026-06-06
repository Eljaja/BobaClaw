use std::collections::HashMap;
use std::sync::Arc;

use bobaclaw_core::{BobaConfig, BobaPaths, NormalizedRequest};
use tokio::sync::{Mutex, Semaphore};

use crate::loop_::{AgentLoop, AgentResponse};
use crate::progress::AgentProgress;

/// Routes agent turns: parallel across sessions, serialized within one scope.
#[derive(Clone)]
pub struct AgentDispatcher {
    agent: Arc<AgentLoop>,
    permits: Arc<Semaphore>,
    scope_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

impl AgentDispatcher {
    pub async fn new(paths: BobaPaths, config: BobaConfig) -> anyhow::Result<Self> {
        let max = config.gateway.max_parallel_turns.max(1);
        Ok(Self {
            agent: Arc::new(AgentLoop::new(paths, config).await?),
            permits: Arc::new(Semaphore::new(max)),
            scope_locks: Arc::new(Mutex::new(HashMap::new())),
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
        let scope = req.dispatch_scope();
        let _permit = self
            .permits
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| anyhow::anyhow!("agent dispatcher shut down"))?;

        let scope_mutex = self.scope_lock(scope).await;
        let _scope_guard = scope_mutex.lock().await;

        self.agent.handle_with_progress(req, progress).await
    }

    async fn scope_lock(&self, scope: String) -> Arc<Mutex<()>> {
        let mut map = self.scope_locks.lock().await;
        map.entry(scope)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }
}
