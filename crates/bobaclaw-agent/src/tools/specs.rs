use std::sync::Arc;

use bobaclaw_core::SubagentPreset;
use bobaclaw_mcp::McpHub;
use bobaclaw_provider::ToolSpec;

use super::{
    child_skill_tool_specs, exec_tool_spec, memory_tool_spec, schedule_tool_specs,
    skill_tool_specs, spawn_status_tool_spec, spawn_tool_spec, subagent_tool_spec,
};

pub fn build_parent_tool_specs(
    mcp: Option<&Arc<McpHub>>,
    subagents_enabled: bool,
) -> Vec<ToolSpec> {
    let mut t = vec![exec_tool_spec()];
    t.extend(schedule_tool_specs());
    t.extend(skill_tool_specs());
    t.push(memory_tool_spec());
    if subagents_enabled {
        t.push(subagent_tool_spec());
        t.push(spawn_tool_spec());
        t.push(spawn_status_tool_spec());
    }
    if let Some(hub) = mcp {
        t.extend(hub.tool_specs());
    }
    t
}

pub fn build_child_tool_specs(
    mcp: Option<&Arc<McpHub>>,
    preset: Option<&SubagentPreset>,
) -> Vec<ToolSpec> {
    let mut t = vec![exec_tool_spec()];
    t.extend(child_skill_tool_specs());
    if let Some(hub) = mcp {
        t.extend(hub.tool_specs());
    }
    if let Some(preset) = preset {
        if !preset.tools_allowlist.is_empty() {
            return filter_tools(t, &preset.tools_allowlist);
        }
    }
    t
}

fn filter_tools(tools: Vec<ToolSpec>, allowlist: &[String]) -> Vec<ToolSpec> {
    tools
        .into_iter()
        .filter(|spec| {
            let name = spec.function.name.as_str();
            allowlist.iter().any(|pat| tool_matches_pattern(name, pat))
        })
        .collect()
}

fn tool_matches_pattern(name: &str, pattern: &str) -> bool {
    if pattern.ends_with('*') {
        let prefix = pattern.trim_end_matches('*');
        name.starts_with(prefix)
    } else {
        name == pattern
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_specs_exclude_subagent_memory_schedule() {
        let specs = build_child_tool_specs(None, None);
        let names: Vec<_> = specs.iter().map(|s| s.function.name.as_str()).collect();
        assert!(names.contains(&"exec"));
        assert!(names.contains(&"skill_view"));
        assert!(!names
            .iter()
            .any(|n| { *n == "subagent" || *n == "spawn" || *n == "spawn_status" }));
        assert!(!names.iter().any(|n| n.starts_with("schedule")));
        assert!(!names.iter().any(|n| *n == "memory_manage"));
    }

    #[test]
    fn parent_specs_include_subagent_when_enabled() {
        let specs = build_parent_tool_specs(None, true);
        assert!(specs.iter().any(|s| s.function.name == "subagent"));
    }

    #[test]
    fn allowlist_filters_tools() {
        let specs = build_child_tool_specs(None, None);
        let preset = SubagentPreset {
            tools_allowlist: vec!["exec".into()],
            ..Default::default()
        };
        let filtered = build_child_tool_specs(None, Some(&preset));
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].function.name, "exec");
        assert!(specs.len() > 1);
    }
}
