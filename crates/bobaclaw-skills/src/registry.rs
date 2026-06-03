use std::path::{Path, PathBuf};

use serde::Deserialize;
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct SkillEntry {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
    pub body: String,
    pub tags: Vec<String>,
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
        let skills_dir = workspace_group.join("skills");
        if !skills_dir.exists() {
            return Ok(Self::default());
        }

        let mut skills = Vec::new();
        for entry in WalkDir::new(&skills_dir)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_name() == "SKILL.md" {
                if let Ok(skill) = parse_skill(entry.path()) {
                    skills.push(skill);
                }
            }
        }
        Ok(Self { skills })
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
                || s.tags.iter().any(|t| lower.contains(&t.to_lowercase()))
        })
    }
}

fn parse_skill(path: &Path) -> anyhow::Result<SkillEntry> {
    let raw = std::fs::read_to_string(path)?;
    let (front, body) = split_frontmatter(&raw);
    let fm: SkillFrontmatter = serde_yaml::from_str(front)?;
    let name = fm
        .name
        .unwrap_or_else(|| path.parent().and_then(|p| p.file_name()).map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "unnamed".into()));
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
    })
}

fn split_frontmatter(raw: &str) -> (&str, &str) {
    if raw.starts_with("---") {
        if let Some(end) = raw[3..].find("\n---") {
            let front = &raw[3..3 + end];
            let body = &raw[3 + end + 4..];
            return (front, body);
        }
    }
    ("", raw)
}
