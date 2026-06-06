use bobaclaw_core::BobaPaths;
use bobaclaw_provider::{FunctionSpec, ToolCall, ToolSpec};
use bobaclaw_skills::{SkillManager, SkillRegistry};
use serde::Deserialize;
use serde_json::json;

pub const SKILL_MANAGE: &str = "skill_manage";
pub const SKILL_VIEW: &str = "skill_view";
pub const SKILLS_LIST: &str = "skills_list";

pub fn skill_tool_specs() -> Vec<ToolSpec> {
    vec![
        skill_manage_spec(),
        skill_view_spec(),
        skills_list_spec(),
    ]
}

pub fn is_skill_tool(name: &str) -> bool {
    matches!(name, SKILL_MANAGE | SKILL_VIEW | SKILLS_LIST)
}

fn skill_manage_spec() -> ToolSpec {
    ToolSpec {
        kind: "function".into(),
        function: FunctionSpec {
            name: SKILL_MANAGE.into(),
            description: "Create, update, or delete workspace skills (procedural memory). \
                Actions: create (name+content), patch (name+old_string+new_string), \
                edit (name+content), delete (name), write_file (name+file_path+file_content), \
                remove_file (name+file_path). New skills need YAML frontmatter with name and description."
                .into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "action": {
                        "type": "string",
                        "enum": ["create", "patch", "edit", "delete", "write_file", "remove_file"]
                    },
                    "name": { "type": "string" },
                    "content": { "type": "string", "description": "Full SKILL.md for create/edit" },
                    "old_string": { "type": "string" },
                    "new_string": { "type": "string" },
                    "file_path": { "type": "string" },
                    "file_content": { "type": "string" },
                    "category": { "type": "string", "description": "Optional category subdirectory" }
                },
                "required": ["action", "name"]
            }),
        },
    }
}

fn skill_view_spec() -> ToolSpec {
    ToolSpec {
        kind: "function".into(),
        function: FunctionSpec {
            name: SKILL_VIEW.into(),
            description: "Read a skill's SKILL.md content by name.".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string" }
                },
                "required": ["name"]
            }),
        },
    }
}

fn skills_list_spec() -> ToolSpec {
    ToolSpec {
        kind: "function".into(),
        function: FunctionSpec {
            name: SKILLS_LIST.into(),
            description: "List installed workspace skills with enabled/disabled status.".into(),
            parameters: json!({
                "type": "object",
                "properties": {}
            }),
        },
    }
}

#[derive(Debug, Deserialize)]
struct SkillManageArgs {
    action: String,
    name: String,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    old_string: Option<String>,
    #[serde(default)]
    new_string: Option<String>,
    #[serde(default)]
    file_path: Option<String>,
    #[serde(default)]
    file_content: Option<String>,
    #[serde(default)]
    category: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SkillNameArgs {
    name: String,
}

pub fn handle_skill_tool(
    paths: &BobaPaths,
    agent_group: &str,
    call: &ToolCall,
) -> anyhow::Result<String> {
    let workspace = paths.group_workspace(agent_group);
    let mgr = SkillManager::new(&workspace);

    match call.function.name.as_str() {
        SKILL_MANAGE => {
            let args: SkillManageArgs = serde_json::from_str(&call.function.arguments)
                .map_err(|e| anyhow::anyhow!("invalid skill_manage arguments: {e}"))?;
            let result = match args.action.as_str() {
                "create" => {
                    let content = args
                        .content
                        .ok_or_else(|| anyhow::anyhow!("create requires content"))?;
                    mgr.create(&args.name, &content, args.category.as_deref())
                }
                "edit" => {
                    let content = args
                        .content
                        .ok_or_else(|| anyhow::anyhow!("edit requires content"))?;
                    mgr.edit(&args.name, &content)
                }
                "patch" => {
                    let old = args
                        .old_string
                        .ok_or_else(|| anyhow::anyhow!("patch requires old_string"))?;
                    let new = args
                        .new_string
                        .ok_or_else(|| anyhow::anyhow!("patch requires new_string"))?;
                    mgr.patch(
                        &args.name,
                        &old,
                        &new,
                        args.file_path.as_deref(),
                    )
                }
                "delete" => mgr.delete(&args.name),
                "write_file" => {
                    let path = args
                        .file_path
                        .ok_or_else(|| anyhow::anyhow!("write_file requires file_path"))?;
                    let content = args
                        .file_content
                        .ok_or_else(|| anyhow::anyhow!("write_file requires file_content"))?;
                    mgr.write_file(&args.name, &path, &content)
                }
                "remove_file" => {
                    let path = args
                        .file_path
                        .ok_or_else(|| anyhow::anyhow!("remove_file requires file_path"))?;
                    mgr.remove_file(&args.name, &path)
                }
                other => return Err(anyhow::anyhow!("unknown skill_manage action: {other}")),
            };
            result.map_err(|e| anyhow::anyhow!(e))
        }
        SKILL_VIEW => {
            let args: SkillNameArgs = serde_json::from_str(&call.function.arguments)
                .map_err(|e| anyhow::anyhow!("invalid skill_view arguments: {e}"))?;
            mgr.view(&args.name).map_err(|e| anyhow::anyhow!(e))
        }
        SKILLS_LIST => {
            let listings = SkillRegistry::list_all(&workspace)?;
            if listings.is_empty() {
                return Ok("No skills installed.".into());
            }
            let mut lines = Vec::new();
            for item in listings {
                let status = if item.entry.enabled { "enabled" } else { "disabled" };
                lines.push(format!(
                    "- {} ({}) — {}",
                    item.entry.name, status, item.entry.description
                ));
            }
            Ok(lines.join("\n"))
        }
        other => anyhow::bail!("unknown skill tool: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bobaclaw_provider::FunctionCallPayload;

    #[test]
    fn create_via_skill_manage_handler() {
        let dir = tempfile::tempdir().unwrap();
        let paths = bobaclaw_core::BobaPaths::from_home(dir.path().to_path_buf());
        std::fs::create_dir_all(paths.group_workspace("home")).unwrap();
        let md = "---\nname: t1\ndescription: Test\n---\n\n# T1\n\nDo thing.\n";
        let call = ToolCall {
            id: "1".into(),
            kind: "function".into(),
            function: FunctionCallPayload {
                name: SKILL_MANAGE.into(),
                arguments: json!({
                    "action": "create",
                    "name": "t1",
                    "content": md
                })
                .to_string(),
            },
        };
        let out = handle_skill_tool(&paths, "home", &call).unwrap();
        assert!(out.contains("Created skill"));
    }
}
