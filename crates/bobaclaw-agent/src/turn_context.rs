#[derive(Debug, Clone)]
pub struct TurnContext {
    pub delegation_depth: u8,
    /// Run id of the immediate parent (ledger / tracing chain).
    pub parent_run_id: Option<String>,
    /// Run id of this turn (latest exec or subagent), when known.
    pub run_id: Option<String>,
    pub parent_session_id: String,
    pub subagent_label: Option<String>,
}

impl TurnContext {
    pub fn parent(session_id: &str) -> Self {
        Self {
            delegation_depth: 0,
            parent_run_id: None,
            run_id: None,
            parent_session_id: session_id.to_string(),
            subagent_label: None,
        }
    }

    pub fn child(parent: &Self, label: Option<String>) -> Self {
        Self {
            delegation_depth: parent.delegation_depth.saturating_add(1),
            parent_run_id: parent.run_id.clone(),
            run_id: None,
            parent_session_id: parent.parent_session_id.clone(),
            subagent_label: label,
        }
    }

    /// Parent context immediately before `subagent` / `spawn`: attach latest exec run id.
    pub fn for_delegation(&self, active_run_id: Option<&str>) -> Self {
        let mut ctx = self.clone();
        if ctx.run_id.is_none() {
            ctx.run_id = active_run_id.map(str::to_string);
        }
        ctx
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnMode {
    Parent,
    Child,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_increments_depth() {
        let parent = TurnContext::parent("sess_1");
        assert_eq!(parent.delegation_depth, 0);
        let child = TurnContext::child(&parent, Some("research".into()));
        assert_eq!(child.delegation_depth, 1);
        assert_eq!(child.parent_session_id, "sess_1");
        assert_eq!(child.subagent_label.as_deref(), Some("research"));
    }

    #[test]
    fn nested_child_would_exceed_default_max_depth() {
        let parent = TurnContext::parent("sess_1");
        let child = TurnContext::child(&parent, None);
        let nested = TurnContext::child(&child, None);
        assert_eq!(nested.delegation_depth, 2);
    }

    #[test]
    fn for_delegation_attaches_exec_run_id() {
        let parent = TurnContext::parent("sess_1");
        let ctx = parent.for_delegation(Some("run_exec_1"));
        assert_eq!(ctx.run_id.as_deref(), Some("run_exec_1"));
    }

    #[test]
    fn child_parent_run_id_links_immediate_parent() {
        let root = TurnContext::parent("sess_1");
        let mut depth1 =
            TurnContext::child(&root.for_delegation(Some("run_root")), Some("d1".into()));
        depth1.run_id = Some("sub_1".into());
        let depth2 = TurnContext::child(&depth1, None);
        assert_eq!(depth2.parent_run_id.as_deref(), Some("sub_1"));
        assert_ne!(depth2.parent_run_id.as_deref(), Some("run_root"));
    }
}
