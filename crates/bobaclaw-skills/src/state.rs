use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkillRecord {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub agent_created: bool,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub view_count: u32,
    #[serde(default)]
    pub patch_count: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_used_at: Option<f64>,
}

fn default_enabled() -> bool {
    true
}

impl Default for SkillRecord {
    fn default() -> Self {
        Self {
            enabled: true,
            agent_created: false,
            pinned: false,
            view_count: 0,
            patch_count: 0,
            last_used_at: None,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct StateFile {
    #[serde(default)]
    skills: HashMap<String, SkillRecord>,
}

#[derive(Debug, Clone)]
pub struct SkillStateStore {
    path: PathBuf,
    data: StateFile,
}

impl SkillStateStore {
    pub fn load(workspace_group: &Path) -> anyhow::Result<Self> {
        let path = Self::state_path(workspace_group);
        let data = if path.exists() {
            let raw = std::fs::read_to_string(&path)?;
            serde_json::from_str(&raw).unwrap_or_default()
        } else {
            StateFile::default()
        };
        Ok(Self { path, data })
    }

    pub fn state_path(workspace_group: &Path) -> PathBuf {
        workspace_group.join("skills").join(".skills-state.json")
    }

    pub fn save(&self) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(&self.data)?)?;
        std::fs::rename(tmp, &self.path)?;
        Ok(())
    }

    pub fn record(&self, name: &str) -> SkillRecord {
        self.data.skills.get(name).cloned().unwrap_or_default()
    }

    pub fn is_enabled(&self, name: &str) -> bool {
        self.record(name).enabled
    }

    pub fn set_enabled(&mut self, name: &str, enabled: bool) -> anyhow::Result<()> {
        let rec = self.data.skills.entry(name.to_string()).or_default();
        rec.enabled = enabled;
        self.save()
    }

    pub fn mark_agent_created(&mut self, name: &str) -> anyhow::Result<()> {
        let rec = self.data.skills.entry(name.to_string()).or_default();
        rec.agent_created = true;
        self.save()
    }

    pub fn bump_view(&mut self, name: &str) -> anyhow::Result<()> {
        let rec = self.data.skills.entry(name.to_string()).or_default();
        rec.view_count = rec.view_count.saturating_add(1);
        rec.last_used_at = Some(now_secs());
        self.save()
    }

    pub fn bump_patch(&mut self, name: &str) -> anyhow::Result<()> {
        let rec = self.data.skills.entry(name.to_string()).or_default();
        rec.patch_count = rec.patch_count.saturating_add(1);
        rec.last_used_at = Some(now_secs());
        self.save()
    }

    pub fn is_pinned(&self, name: &str) -> bool {
        self.record(name).pinned
    }

    pub fn all_records(&self) -> &HashMap<String, SkillRecord> {
        &self.data.skills
    }

    pub fn remove(&mut self, name: &str) -> anyhow::Result<()> {
        self.data.skills.remove(name);
        self.save()
    }
}

fn now_secs() -> f64 {
    chrono::Utc::now().timestamp_millis() as f64 / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_skill_is_enabled() {
        let dir = tempfile::tempdir().unwrap();
        let store = SkillStateStore::load(dir.path()).unwrap();
        assert!(store.is_enabled("hello"));
    }

    #[test]
    fn disable_persists() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = SkillStateStore::load(dir.path()).unwrap();
        store.set_enabled("demo", false).unwrap();
        let store2 = SkillStateStore::load(dir.path()).unwrap();
        assert!(!store2.is_enabled("demo"));
    }
}
