use std::sync::Arc;

use bobaclaw_core::{BobaConfig, BobaPaths, NormalizedRequest};
use bobaclaw_state::{SessionStore, SpawnJobRecord};
use tokio::sync::Mutex;

use crate::channel_delivery::DeliveryRegistry;
use crate::loop_::{AgentLoop, AgentResponse};
use crate::progress::AgentProgress;
use crate::scope_gate::ScopeGate;
use crate::spawn_completer::SpawnCompleter;

/// Routes agent turns: parallel across sessions, serialized within one session.
///
/// Every request is first resolved to its session id (Telegram peer route, CLI/REST
/// ingress session, explicit `session_id` for spawn wakes / scheduled tasks), and the
/// scope key is always `session:<id>`, so all writers to one session history are
/// serialized regardless of ingress.
///
/// Preemption policy (see [`bobaclaw_core::IngressKind::preempts_in_flight`] and
/// [`crate::scope_gate`]):
/// * Interactive user messages (CLI, chat, Telegram) cancel the in-flight turn on
///   their session and any older user message still queued there — newest wins.
///   Superseded queued messages are still recorded in history and return
///   `interrupted` without calling the LLM.
/// * Background / programmatic ingress (spawn wake, cron, webhook, REST, OpenAI-compat)
///   never cancels anything; it queues behind the running turn. Concurrent REST or
///   OpenAI-compat calls without `session_id` share one ingress session per group and
///   therefore run one after another instead of cancelling each other.
/// * Queued requests do not hold a global `max_parallel_turns` permit.
#[derive(Clone)]
pub struct AgentDispatcher {
    agent: Arc<AgentLoop>,
    gate: Arc<ScopeGate>,
    /// Serializes get-or-create session resolution so two concurrent first messages
    /// from one peer cannot create two sessions.
    resolve_lock: Arc<Mutex<()>>,
}

impl AgentDispatcher {
    pub async fn new(paths: BobaPaths, config: BobaConfig) -> anyhow::Result<Self> {
        let max = config.gateway.max_parallel_turns.max(1);
        Ok(Self {
            agent: Arc::new(AgentLoop::new(paths, config).await?),
            gate: Arc::new(ScopeGate::new(max)),
            resolve_lock: Arc::new(Mutex::new(())),
        })
    }

    pub async fn handle(&self, req: NormalizedRequest) -> anyhow::Result<AgentResponse> {
        self.handle_with_progress(req, None).await
    }

    pub async fn handle_with_progress(
        &self,
        mut req: NormalizedRequest,
        progress: Option<&dyn AgentProgress>,
    ) -> anyhow::Result<AgentResponse> {
        let session_id = self.resolve_session(&req).await?;
        req.session_id = Some(session_id.clone());
        let scope = NormalizedRequest::session_scope(&session_id);

        let turn = self
            .gate
            .enter(&scope, req.ingress.preempts_in_flight())
            .await?;
        let result = self
            .agent
            .handle_with_progress(req, progress, turn.cancel_token())
            .await;
        drop(turn);
        result
    }

    /// Cancel the in-flight turn for a raw scope key (`session:<id>`).
    pub async fn interrupt_scope(&self, scope: &str) -> bool {
        self.gate.interrupt(scope)
    }

    /// `true` while a turn for this raw scope key is running or queued.
    pub async fn is_scope_busy(&self, scope: &str) -> bool {
        self.gate.is_busy(scope)
    }

    /// Cancel the in-flight turn on a session (CLI Ctrl+C, `/stop`, gateway interrupt).
    pub async fn interrupt_session(&self, session_id: &str) -> bool {
        self.gate
            .interrupt(&NormalizedRequest::session_scope(session_id))
    }

    /// Cancel the in-flight turn on the session `req` would be routed to (without
    /// creating a session). Returns `false` if there is no such session or no turn.
    pub async fn interrupt_request(&self, req: &NormalizedRequest) -> bool {
        match SessionStore::new(self.agent.pool()).find_session(req).await {
            Ok(Some(sid)) => self.interrupt_session(&sid).await,
            Ok(None) => false,
            Err(e) => {
                tracing::warn!("interrupt: session lookup failed: {e}");
                false
            }
        }
    }

    async fn resolve_session(&self, req: &NormalizedRequest) -> anyhow::Result<String> {
        if let Some(ref sid) = req.session_id {
            return Ok(sid.clone());
        }
        let _guard = self.resolve_lock.lock().await;
        SessionStore::new(self.agent.pool())
            .resolve_session(req)
            .await
    }

    pub async fn wire_spawn_feedback(
        self: &Arc<Self>,
        config: BobaConfig,
        deliveries: Arc<DeliveryRegistry>,
    ) {
        let pool = self.agent.pool().clone();
        let completer = Arc::new(SpawnCompleter::new(config, pool, self.clone(), deliveries));
        self.agent.set_spawn_completer(completer).await;
    }

    pub async fn list_spawn_jobs(&self, session_id: &str) -> Vec<SpawnJobRecord> {
        self.agent.list_spawn_jobs(session_id).await
    }

    pub async fn get_spawn_job(&self, id: &str) -> Option<SpawnJobRecord> {
        bobaclaw_state::SpawnJobStore::new(self.agent.pool())
            .get(id)
            .await
            .ok()
            .flatten()
    }
}
