#[derive(Debug, Clone)]
pub struct TurnContext {
    pub delegation_depth: u8,
    pub parent_run_id: Option<String>,
    pub parent_session_id: String,
    pub subagent_label: Option<String>,
}

impl TurnContext {
    pub fn parent(session_id: &str) -> Self {
        Self {
            delegation_depth: 0,
            parent_run_id: None,
            parent_session_id: session_id.to_string(),
            subagent_label: None,
        }
    }

    pub fn child(parent: &Self, label: Option<String>) -> Self {
        Self {
            delegation_depth: parent.delegation_depth.saturating_add(1),
            parent_run_id: parent.parent_run_id.clone(),
            parent_session_id: parent.parent_session_id.clone(),
            subagent_label: label,
        }
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
}
