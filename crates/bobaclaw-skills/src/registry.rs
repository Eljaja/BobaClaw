use std::path::{Path, PathBuf};

use serde::Deserialize;
use walkdir::WalkDir;

use crate::state::SkillStateStore;

#[derive(Debug, Clone)]
pub struct SkillEntry {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
    pub body: String,
    pub tags: Vec<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct SkillListing {
    pub entry: SkillEntry,
    pub record: crate::state::SkillRecord,
}

#[derive(Debug, Clone, Default)]
pub struct SkillRegistry {
    skills: Vec<SkillEntry>,
}

#[derive(Debug, Deserialize)]
struct SkillFrontmatter {
    name: Option<String>,
    description: Option<String>,
    #[serde(default)]
    metadata: Option<SkillMetadata>,
}

#[derive(Debug, Deserialize)]
struct SkillMetadata {
    bobaclaw: Option<BobaclawMeta>,
}

#[derive(Debug, Deserialize)]
struct BobaclawMeta {
    tags: Option<Vec<String>>,
}

impl SkillRegistry {
    pub fn load(workspace_group: &Path) -> anyhow::Result<Self> {
        Self::load_filtered(workspace_group, false)
    }

    /// Load only enabled skills (for agent matching and prompts).
    pub fn load_enabled(workspace_group: &Path) -> anyhow::Result<Self> {
        Self::load_filtered(workspace_group, true)
    }

    fn load_filtered(workspace_group: &Path, enabled_only: bool) -> anyhow::Result<Self> {
        let skills_dir = workspace_group.join("skills");
        if !skills_dir.exists() {
            return Ok(Self::default());
        }

        let state = SkillStateStore::load(workspace_group)?;
        let mut skills = Vec::new();
        for entry in WalkDir::new(&skills_dir).into_iter().filter_map(|e| e.ok()) {
            if entry.file_name() == ".skills-state.json" {
                continue;
            }
            if entry.file_name() == "SKILL.md" {
                if let Ok(mut skill) = parse_skill(entry.path()) {
                    skill.enabled = state.is_enabled(&skill.name);
                    if enabled_only && !skill.enabled {
                        continue;
                    }
                    skills.push(skill);
                }
            }
        }
        Ok(Self { skills })
    }

    pub fn list_all(workspace_group: &Path) -> anyhow::Result<Vec<SkillListing>> {
        let reg = Self::load(workspace_group)?;
        let state = SkillStateStore::load(workspace_group)?;
        Ok(reg
            .skills
            .iter()
            .map(|e| SkillListing {
                entry: e.clone(),
                record: state.record(&e.name),
            })
            .collect())
    }

    pub fn names(&self) -> Vec<String> {
        self.skills.iter().map(|s| s.name.clone()).collect()
    }

    pub fn list(&self) -> &[SkillEntry] {
        &self.skills
    }

    pub fn get(&self, name: &str) -> Option<&SkillEntry> {
        self.skills.iter().find(|s| s.name == name)
    }

    pub fn match_request(&self, text: &str) -> Option<&SkillEntry> {
        let lower = text.to_lowercase();
        self.skills.iter().find(|s| {
            lower.contains(&s.name.to_lowercase())
                || (!s.description.is_empty() && lower.contains(&s.description.to_lowercase()))
                || s.tags.iter().any(|t| lower.contains(&t.to_lowercase()))
        })
    }
}

fn parse_skill(path: &Path) -> anyhow::Result<SkillEntry> {
    let raw = std::fs::read_to_string(path)?;
    let (front, body) = split_frontmatter(&raw);
    let fm: SkillFrontmatter = serde_yaml::from_str(front)?;
    let name = fm.name.unwrap_or_else(|| {
        path.parent()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unnamed".into())
    });
    let tags = fm
        .metadata
        .and_then(|m| m.bobaclaw)
        .and_then(|b| b.tags)
        .unwrap_or_default();
    Ok(SkillEntry {
        description: fm.description.unwrap_or_default(),
        name,
        path: path.to_path_buf(),
        body: body.trim().to_string(),
        tags,
        enabled: true,
    })
}

fn split_frontmatter(raw: &str) -> (&str, &str) {
    let Some(rest) = raw.strip_prefix("---") else {
        return ("", raw);
    };
    if let Some(end) = rest.find("\n---") {
        let front = &rest[..end];
        let body = &rest[end + 4..];
        (front, body)
    } else {
        ("", raw)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn load_hello_skill_from_examples() {
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let ws = manifest.join("../../workspace-examples/home");
        let reg = SkillRegistry::load(&ws).unwrap();
        assert!(reg.names().contains(&"hello".to_string()));
        let skill = reg.get("hello").unwrap();
        assert!(skill.description.to_lowercase().contains("hello"));
    }

    #[test]
    fn disabled_skill_excluded_from_enabled_load() {
        let dir = tempfile::tempdir().unwrap();
        let skill_dir = dir.path().join("skills/demo");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\n\nBody",
        )
        .unwrap();
        let mut state = SkillStateStore::load(dir.path()).unwrap();
        state.set_enabled("demo", false).unwrap();
        let reg = SkillRegistry::load_enabled(dir.path()).unwrap();
        assert!(reg.get("demo").is_none());
        let all = SkillRegistry::load(dir.path()).unwrap();
        assert!(all.get("demo").is_some());
    }

    #[test]
    fn match_request_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let skill_dir = dir.path().join("skills/demo");
        fs::create_dir_all(&skill_dir).unwrap();
        fs::write(
            skill_dir.join("SKILL.md"),
            "---\nname: demo\ndescription: Demo\n---\n\nBody",
        )
        .unwrap();
        let reg = SkillRegistry::load(dir.path()).unwrap();
        assert!(reg.match_request("please run demo skill").is_some());
    }
}
