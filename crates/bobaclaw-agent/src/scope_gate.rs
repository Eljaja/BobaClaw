//! Per-scope turn serialization with preemption and a global concurrency limit.
//!
//! [`ScopeGate`] is the scheduling core of [`crate::AgentDispatcher`], kept free of
//! LLM/session concerns so it can be tested with fake jobs.
//!
//! # Semantics
//!
//! * Turns with the same scope key run strictly one at a time, in arrival order
//!   (tokio's `Mutex` is FIFO-fair).
//! * Turns with different scope keys run in parallel, bounded by the global permit
//!   count. The per-scope lock is taken **before** a permit, so requests queued
//!   behind a busy scope never hold a permit (no head-of-line blocking of other scopes).
//! * A *preempting* entry (user message) cancels, at arrival time, the running turn
//!   of its scope and every *preempting* entry still queued there — newest user
//!   message wins. Cancelled queued entries still get their turn slot, but with an
//!   already-cancelled token, so the caller can record the input and return
//!   "interrupted" immediately.
//! * A *non-preempting* entry (background work) never cancels anything and is never
//!   cancelled by later arrivals while queued; it simply waits its turn. Once running,
//!   it can be cancelled by a later preempting entry or [`ScopeGate::interrupt`].
//! * Scope bookkeeping is reference counted and removed as soon as no entry
//!   (queued or running) references it, so the map does not grow without bound.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use tokio::sync::{Mutex, OwnedMutexGuard, OwnedSemaphorePermit, Semaphore};
use tokio_util::sync::CancellationToken;

#[derive(Default)]
struct ScopeState {
    lock: Arc<Mutex<()>>,
    /// Entries (queued or running) that currently reference this scope.
    users: usize,
    running: Option<(u64, CancellationToken)>,
    /// Queued preempting entries (cancelled by the next preempting arrival).
    queued_preemptible: Vec<(u64, CancellationToken)>,
}

impl ScopeState {
    fn cancel_preemptible(&self) -> bool {
        let mut any = false;
        if let Some((_, token)) = &self.running {
            token.cancel();
            any = true;
        }
        for (_, token) in &self.queued_preemptible {
            token.cancel();
            any = true;
        }
        any
    }
}

type ScopeMap = Arc<StdMutex<HashMap<String, ScopeState>>>;

pub struct ScopeGate {
    permits: Arc<Semaphore>,
    scopes: ScopeMap,
    next_id: AtomicU64,
}

/// Held for the duration of a turn. Dropping it releases the permit, the scope lock
/// and the scope bookkeeping (in that order).
pub struct TurnGuard {
    cancel: CancellationToken,
    _permit: OwnedSemaphorePermit,
    _scope_guard: OwnedMutexGuard<()>,
    _registration: Registration,
}

impl TurnGuard {
    /// Token cancelled on preemption / interrupt. May already be cancelled if the
    /// entry was superseded while queued.
    pub fn cancel_token(&self) -> CancellationToken {
        self.cancel.clone()
    }
}

/// Removes an entry from the scope bookkeeping on drop (also when the `enter`
/// future is dropped while still waiting).
struct Registration {
    scopes: ScopeMap,
    scope: String,
    id: u64,
}

impl Drop for Registration {
    fn drop(&mut self) {
        let mut map = self.scopes.lock().unwrap_or_else(|e| e.into_inner());
        let Some(state) = map.get_mut(&self.scope) else {
            return;
        };
        state.users = state.users.saturating_sub(1);
        if state.running.as_ref().is_some_and(|(id, _)| *id == self.id) {
            state.running = None;
        }
        state.queued_preemptible.retain(|(id, _)| *id != self.id);
        if state.users == 0 {
            map.remove(&self.scope);
        }
    }
}

impl ScopeGate {
    pub fn new(max_parallel: usize) -> Self {
        Self {
            permits: Arc::new(Semaphore::new(max_parallel.max(1))),
            scopes: Arc::new(StdMutex::new(HashMap::new())),
            next_id: AtomicU64::new(0),
        }
    }

    fn map(&self) -> std::sync::MutexGuard<'_, HashMap<String, ScopeState>> {
        self.scopes.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Wait for this scope's turn and a global permit. See module docs for semantics.
    pub async fn enter(&self, scope: &str, preempt: bool) -> anyhow::Result<TurnGuard> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let cancel = CancellationToken::new();
        let lock = {
            let mut map = self.map();
            let state = map.entry(scope.to_string()).or_default();
            state.users += 1;
            if preempt {
                state.cancel_preemptible();
                state.queued_preemptible.push((id, cancel.clone()));
            }
            state.lock.clone()
        };
        let registration = Registration {
            scopes: self.scopes.clone(),
            scope: scope.to_string(),
            id,
        };

        let scope_guard = lock.lock_owned().await;
        let permit = self
            .permits
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| anyhow::anyhow!("agent dispatcher shut down"))?;

        {
            let mut map = self.map();
            if let Some(state) = map.get_mut(scope) {
                state.queued_preemptible.retain(|(qid, _)| *qid != id);
                state.running = Some((id, cancel.clone()));
            }
        }

        Ok(TurnGuard {
            cancel,
            _permit: permit,
            _scope_guard: scope_guard,
            _registration: registration,
        })
    }

    /// Cancel the running turn and queued preempting entries of a scope.
    /// Returns `true` if anything was cancelled.
    pub fn interrupt(&self, scope: &str) -> bool {
        self.map()
            .get(scope)
            .is_some_and(ScopeState::cancel_preemptible)
    }

    /// `true` while a turn for this scope is running or queued.
    pub fn is_busy(&self, scope: &str) -> bool {
        self.map().get(scope).is_some_and(|s| s.users > 0)
    }

    #[cfg(test)]
    fn tracked_scopes(&self) -> usize {
        self.map().len()
    }

    #[cfg(test)]
    fn available_permits(&self) -> usize {
        self.permits.available_permits()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    const SHORT: Duration = Duration::from_millis(50);

    async fn is_pending<T>(handle: &mut tokio::task::JoinHandle<T>) -> bool {
        tokio::time::timeout(SHORT, handle).await.is_err()
    }

    /// Fake job: enter the gate, record concurrency, sleep, report whether cancelled.
    async fn fake_job(
        gate: Arc<ScopeGate>,
        scope: &'static str,
        preempt: bool,
        active: Arc<AtomicUsize>,
        max_active: Arc<AtomicUsize>,
        order: Arc<StdMutex<Vec<&'static str>>>,
        name: &'static str,
    ) -> bool {
        let turn = gate.enter(scope, preempt).await.unwrap();
        let now = active.fetch_add(1, Ordering::SeqCst) + 1;
        max_active.fetch_max(now, Ordering::SeqCst);
        order.lock().unwrap().push(name);
        let token = turn.cancel_token();
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(30)) => {}
            _ = token.cancelled() => {}
        }
        active.fetch_sub(1, Ordering::SeqCst);
        token.is_cancelled()
    }

    #[tokio::test]
    async fn background_jobs_same_scope_run_sequentially() {
        let gate = Arc::new(ScopeGate::new(4));
        let active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let order = Arc::new(StdMutex::new(Vec::new()));
        let mut handles = Vec::new();
        for name in ["a", "b", "c"] {
            handles.push(tokio::spawn(fake_job(
                gate.clone(),
                "session:1",
                false,
                active.clone(),
                max_active.clone(),
                order.clone(),
                name,
            )));
            tokio::task::yield_now().await;
        }
        for h in handles {
            assert!(!h.await.unwrap(), "background job must not be cancelled");
        }
        assert_eq!(max_active.load(Ordering::SeqCst), 1);
        assert_eq!(*order.lock().unwrap(), vec!["a", "b", "c"]);
        assert_eq!(gate.tracked_scopes(), 0);
    }

    #[tokio::test]
    async fn user_preempts_in_flight_background_does_not() {
        let gate = Arc::new(ScopeGate::new(4));

        // Background arrival does not cancel a running turn; it queues.
        let running = gate.enter("session:1", true).await.unwrap();
        let g = gate.clone();
        let mut bg = tokio::spawn(async move { g.enter("session:1", false).await.unwrap() });
        assert!(is_pending(&mut bg).await);
        assert!(!running.cancel_token().is_cancelled());
        assert!(gate.is_busy("session:1"));

        // User arrival cancels the running turn, but not the queued background entry.
        let g = gate.clone();
        let mut user = tokio::spawn(async move { g.enter("session:1", true).await.unwrap() });
        tokio::time::sleep(SHORT).await;
        assert!(running.cancel_token().is_cancelled());
        drop(running);

        // FIFO: queued background runs first, uncancelled; the user turn waits.
        let bg_turn = bg.await.unwrap();
        assert!(!bg_turn.cancel_token().is_cancelled());
        assert!(is_pending(&mut user).await);
        drop(bg_turn);

        let user_turn = user.await.unwrap();
        assert!(!user_turn.cancel_token().is_cancelled());
        drop(user_turn);
        assert!(!gate.is_busy("session:1"));
        assert_eq!(gate.tracked_scopes(), 0);
    }

    #[tokio::test]
    async fn newest_user_message_wins_over_queued_ones() {
        let gate = Arc::new(ScopeGate::new(4));
        let first = gate.enter("session:1", true).await.unwrap();

        let g = gate.clone();
        let mut second = tokio::spawn(async move { g.enter("session:1", true).await.unwrap() });
        assert!(is_pending(&mut second).await);
        assert!(first.cancel_token().is_cancelled());

        let g = gate.clone();
        let mut third = tokio::spawn(async move { g.enter("session:1", true).await.unwrap() });
        assert!(is_pending(&mut third).await);
        drop(first);

        // The superseded queued message gets its slot with an already-cancelled token.
        let second_turn = second.await.unwrap();
        assert!(second_turn.cancel_token().is_cancelled());
        drop(second_turn);

        let third_turn = third.await.unwrap();
        assert!(!third_turn.cancel_token().is_cancelled());
        drop(third_turn);
        assert_eq!(gate.tracked_scopes(), 0);
    }

    #[tokio::test]
    async fn interrupt_cancels_running_turn() {
        let gate = ScopeGate::new(1);
        assert!(!gate.interrupt("session:1"));
        let turn = gate.enter("session:1", false).await.unwrap();
        assert!(gate.interrupt("session:1"));
        assert!(turn.cancel_token().is_cancelled());
        assert!(!gate.interrupt("session:2"));
    }

    #[tokio::test]
    async fn different_scopes_parallel_up_to_permit_limit() {
        let gate = Arc::new(ScopeGate::new(2));
        let a = gate.enter("session:a", false).await.unwrap();

        // Queued same-scope entry must not hold a permit.
        let g = gate.clone();
        let mut a2 = tokio::spawn(async move { g.enter("session:a", false).await.unwrap() });
        assert!(is_pending(&mut a2).await);
        assert_eq!(gate.available_permits(), 1);

        // Another scope still gets the free permit despite the queued a2.
        let b = tokio::time::timeout(SHORT, gate.enter("session:b", false))
            .await
            .expect("session:b must not be blocked by queued session:a")
            .unwrap();
        assert_eq!(gate.available_permits(), 0);

        // Third scope waits for a permit.
        let g = gate.clone();
        let mut c = tokio::spawn(async move { g.enter("session:c", false).await.unwrap() });
        assert!(is_pending(&mut c).await);

        drop(b);
        let c_turn = c.await.unwrap();
        drop(a);
        let a2_turn = a2.await.unwrap();
        drop(c_turn);
        drop(a2_turn);
        assert_eq!(gate.available_permits(), 2);
        assert_eq!(gate.tracked_scopes(), 0);
    }

    #[tokio::test]
    async fn lock_map_cleaned_up_including_abandoned_waiters() {
        let gate = Arc::new(ScopeGate::new(2));
        for i in 0..50 {
            let scope = format!("session:{i}");
            let turn = gate.enter(&scope, i % 2 == 0).await.unwrap();
            drop(turn);
        }
        assert_eq!(gate.tracked_scopes(), 0);

        // A waiter whose future is dropped (e.g. client disconnect) is unregistered.
        let turn = gate.enter("session:x", false).await.unwrap();
        let g = gate.clone();
        let waiter = tokio::spawn(async move { g.enter("session:x", true).await.map(|_| ()) });
        tokio::time::sleep(SHORT).await;
        waiter.abort();
        let _ = waiter.await;
        assert!(gate.is_busy("session:x"));
        drop(turn);
        assert!(!gate.is_busy("session:x"));
        assert_eq!(gate.tracked_scopes(), 0);
    }
}
