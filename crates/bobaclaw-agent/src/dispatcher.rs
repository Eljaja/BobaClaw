use std::collections::HashMap;
use std::sync::Arc;

use bobaclaw_core::{BobaConfig, BobaPaths, NormalizedRequest};
use tokio::sync::{Mutex, Semaphore};
use tokio_util::sync::CancellationToken;

use crate::loop_::{AgentLoop, AgentResponse};
use crate::progress::AgentProgress;

/// Routes agent turns: parallel across sessions, serialized within one scope.
/// New inbound for the same scope preempts the in-flight turn (Hermes interrupt mode).
#[derive(Clone)]
pub struct AgentDispatcher {
    agent: Arc<AgentLoop>,
    permits: Arc<Semaphore>,
    scope_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    active_turns: Arc<Mutex<HashMap<String, CancellationToken>>>,
}

impl AgentDispatcher {
    pub async fn new(paths: BobaPaths, config: BobaConfig) -> anyhow::Result<Self> {
        let max = config.gateway.max_parallel_turns.max(1);
        Ok(Self {
            agent: Arc::new(AgentLoop::new(paths, config).await?),
            permits: Arc::new(Semaphore::new(max)),
            scope_locks: Arc::new(Mutex::new(HashMap::new())),
            active_turns: Arc::new(Mutex::new(HashMap::new())),
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
        self.preempt_scope(&scope).await;

        let _permit = self
            .permits
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| anyhow::anyhow!("agent dispatcher shut down"))?;

        let scope_mutex = self.scope_lock(scope.clone()).await;
        let _scope_guard = scope_mutex.lock().await;

        let cancel = CancellationToken::new();
        self.register_turn(&scope, cancel.clone()).await;
        let result = self.agent.handle_with_progress(req, progress, cancel).await;
        self.unregister_turn(&scope).await;
        result
    }

    /// Cancel the in-flight turn for a scope (CLI Ctrl+C, `/stop`, gateway interrupt).
    pub async fn interrupt_scope(&self, scope: &str) -> bool {
        self.preempt_scope(scope).await
    }

    pub async fn is_scope_busy(&self, scope: &str) -> bool {
        self.active_turns.lock().await.contains_key(scope)
    }

    async fn preempt_scope(&self, scope: &str) -> bool {
        let token = self.active_turns.lock().await.get(scope).cloned();
        if let Some(token) = token {
            token.cancel();
            true
        } else {
            false
        }
    }

    async fn register_turn(&self, scope: &str, token: CancellationToken) {
        self.active_turns
            .lock()
            .await
            .insert(scope.to_string(), token);
    }

    async fn unregister_turn(&self, scope: &str) {
        self.active_turns.lock().await.remove(scope);
    }

    async fn scope_lock(&self, scope: String) -> Arc<Mutex<()>> {
        let mut map = self.scope_locks.lock().await;
        map.entry(scope)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }
}
