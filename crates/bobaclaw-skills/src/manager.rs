use std::path::{Path, PathBuf};

use crate::guard::{guard_skill_dir, GuardVerdict, TrustLevel};
use crate::state::SkillStateStore;
use crate::validate::{
    validate_category, validate_content_size, validate_frontmatter, validate_name,
    validate_support_path,
};

#[derive(Debug, Clone)]
pub struct SkillManager {
    workspace: PathBuf,
    skills_dir: PathBuf,
}

impl SkillManager {
    pub fn new(workspace_group: &Path) -> Self {
        let skills_dir = workspace_group.join("skills");
        Self {
            workspace: workspace_group.to_path_buf(),
            skills_dir,
        }
    }

    pub fn skills_dir(&self) -> &Path {
        &self.skills_dir
    }

    pub fn find_skill_dir(&self, name: &str) -> Option<PathBuf> {
        if !self.skills_dir.exists() {
            return None;
        }
        for entry in walkdir::WalkDir::new(&self.skills_dir)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if entry.file_name() == "SKILL.md" && entry.path().parent()?.file_name()? == name {
                return entry.path().parent().map(|p| p.to_path_buf());
            }
        }
        None
    }

    pub fn resolve_new_dir(&self, name: &str, category: Option<&str>) -> PathBuf {
        if let Some(cat) = category.filter(|c| !c.is_empty()) {
            self.skills_dir.join(cat).join(name)
        } else {
            self.skills_dir.join(name)
        }
    }

    pub fn create(
        &self,
        name: &str,
        content: &str,
        category: Option<&str>,
    ) -> Result<String, String> {
        if let Some(e) = validate_name(name) {
            return Err(e);
        }
        if let Some(e) = validate_category(category) {
            return Err(e);
        }
        if let Some(e) = validate_frontmatter(content) {
            return Err(e);
        }
        if let Some(e) = validate_content_size(content, "SKILL.md") {
            return Err(e);
        }
        if self.find_skill_dir(name).is_some() {
            return Err(format!("Skill '{name}' already exists."));
        }

        let dir = self.resolve_new_dir(name, category);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::write(dir.join("SKILL.md"), content).map_err(|e| e.to_string())?;
        self.guard_after_write(&dir)?;

        let mut state = SkillStateStore::load(&self.workspace).map_err(|e| e.to_string())?;
        state.mark_agent_created(name).map_err(|e| e.to_string())?;
        Ok(format!("Created skill '{name}' at {}", dir.display()))
    }

    pub fn edit(&self, name: &str, content: &str) -> Result<String, String> {
        let dir = self.find_skill_dir(name).ok_or_else(|| format!("Skill '{name}' not found."))?;
        if let Some(e) = validate_frontmatter(content) {
            return Err(e);
        }
        if let Some(e) = validate_content_size(content, "SKILL.md") {
            return Err(e);
        }
        std::fs::write(dir.join("SKILL.md"), content).map_err(|e| e.to_string())?;
        self.guard_after_write(&dir)?;
        self.bump_patch(name)?;
        Ok(format!("Updated skill '{name}'."))
    }

    pub fn patch(
        &self,
        name: &str,
        old_string: &str,
        new_string: &str,
        file_path: Option<&str>,
    ) -> Result<String, String> {
        let dir = self.find_skill_dir(name).ok_or_else(|| format!("Skill '{name}' not found."))?;
        let rel = file_path.unwrap_or("SKILL.md");
        if rel != "SKILL.md" {
            if let Some(e) = validate_support_path(rel) {
                return Err(e);
            }
        }
        let target = dir.join(rel);
        if !target.exists() {
            return Err(format!("File not found: {rel}"));
        }
        let mut content = std::fs::read_to_string(&target).map_err(|e| e.to_string())?;
        if !content.contains(old_string) {
            return Err("old_string not found in target file.".into());
        }
        content = content.replacen(old_string, new_string, 1);
        if rel == "SKILL.md" {
            if let Some(e) = validate_frontmatter(&content) {
                return Err(e);
            }
        }
        if let Some(e) = validate_content_size(&content, rel) {
            return Err(e);
        }
        std::fs::write(&target, &content).map_err(|e| e.to_string())?;
        self.guard_after_write(&dir)?;
        self.bump_patch(name)?;
        Ok(format!("Patched '{rel}' in skill '{name}'."))
    }

    pub fn write_file(
        &self,
        name: &str,
        file_path: &str,
        file_content: &str,
    ) -> Result<String, String> {
        let dir = self.find_skill_dir(name).ok_or_else(|| format!("Skill '{name}' not found."))?;
        if let Some(e) = validate_support_path(file_path) {
            return Err(e);
        }
        if let Some(e) = validate_content_size(file_content, file_path) {
            return Err(e);
        }
        let target = dir.join(file_path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&target, file_content).map_err(|e| e.to_string())?;
        self.guard_after_write(&dir)?;
        self.bump_patch(name)?;
        Ok(format!("Wrote {file_path} in skill '{name}'."))
    }

    pub fn remove_file(&self, name: &str, file_path: &str) -> Result<String, String> {
        let dir = self.find_skill_dir(name).ok_or_else(|| format!("Skill '{name}' not found."))?;
        if file_path == "SKILL.md" {
            return Err("Cannot remove SKILL.md; use delete to remove the whole skill.".into());
        }
        if let Some(e) = validate_support_path(file_path) {
            return Err(e);
        }
        let target = dir.join(file_path);
        if !target.exists() {
            return Err(format!("File not found: {file_path}"));
        }
        std::fs::remove_file(&target).map_err(|e| e.to_string())?;
        Ok(format!("Removed {file_path} from skill '{name}'."))
    }

    pub fn delete(&self, name: &str) -> Result<String, String> {
        let state = SkillStateStore::load(&self.workspace).map_err(|e| e.to_string())?;
        if state.is_pinned(name) {
            return Err(format!(
                "Skill '{name}' is pinned and cannot be deleted. Disable it instead."
            ));
        }
        let dir = self.find_skill_dir(name).ok_or_else(|| format!("Skill '{name}' not found."))?;
        std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())?;
        let mut state = state;
        state.remove(name).map_err(|e| e.to_string())?;
        Ok(format!("Deleted skill '{name}'."))
    }

    pub fn view(&self, name: &str) -> Result<String, String> {
        let dir = self.find_skill_dir(name).ok_or_else(|| format!("Skill '{name}' not found."))?;
        let content = std::fs::read_to_string(dir.join("SKILL.md")).map_err(|e| e.to_string())?;
        let mut state = SkillStateStore::load(&self.workspace).map_err(|e| e.to_string())?;
        state.bump_view(name).ok();
        Ok(content)
    }

    fn guard_after_write(&self, dir: &Path) -> Result<(), String> {
        let report = guard_skill_dir(dir, TrustLevel::AgentCreated);
        if report.verdict == GuardVerdict::Dangerous {
            return Err(format!(
                "Security guard blocked write: {:?}",
                report.findings
            ));
        }
        Ok(())
    }

    fn bump_patch(&self, name: &str) -> Result<(), String> {
        let mut state = SkillStateStore::load(&self.workspace).map_err(|e| e.to_string())?;
        state.bump_patch(name).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, SkillManager) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        std::fs::create_dir_all(root.join("skills")).unwrap();
        (dir, SkillManager::new(&root))
    }

    #[test]
    fn create_and_find_skill() {
        let (_dir, mgr) = setup();
        let md = "---\nname: demo\ndescription: Demo skill\n---\n\n# Demo\n\nSteps here.\n";
        mgr.create("demo", md, None).unwrap();
        assert!(mgr.find_skill_dir("demo").is_some());
    }

    #[test]
    fn patch_skill_md() {
        let (_dir, mgr) = setup();
        let md = "---\nname: demo\ndescription: Demo skill\n---\n\n# Demo\n\nOld text.\n";
        mgr.create("demo", md, None).unwrap();
        mgr.patch("demo", "Old text.", "New text.", None).unwrap();
        let content = mgr.view("demo").unwrap();
        assert!(content.contains("New text."));
    }

    #[test]
    fn delete_removes_dir() {
        let (dir, mgr) = setup();
        let md = "---\nname: demo\ndescription: Demo skill\n---\n\n# Demo\n";
        mgr.create("demo", md, None).unwrap();
        mgr.delete("demo").unwrap();
        assert!(!dir.path().join("skills/demo").exists());
    }
}
