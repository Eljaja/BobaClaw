use std::path::Path;

use bobaclaw_core::{BobaPaths, RunStatus};
use bobaclaw_skills::{guard_skill_dir, GuardVerdict, TrustLevel};
use bobaclaw_state::{RunLedger, StateDb};
use serde_json::json;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct SkillDraft {
    pub id: String,
    pub agent_group: String,
    pub name: Option<String>,
    pub staging_path: String,
    pub status: String,
    pub created_at: f64,
}

pub struct SkillForge {
    paths: BobaPaths,
    agent_group: String,
}

impl SkillForge {
    pub fn new(paths: BobaPaths, agent_group: String) -> Self {
        Self { paths, agent_group }
    }

    pub async fn draft_from_run(&self, state: &StateDb, run_id: &str) -> anyhow::Result<String> {
        let ledger = RunLedger::new(state.pool());
        let run = ledger
            .get_run(run_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("run not found: {run_id}"))?;

        if run.status != RunStatus::Completed {
            anyhow::bail!(
                "run {run_id} is not successful (status={:?}, exit_code={:?})",
                run.status,
                run.exit_code
            );
        }
        if run.exit_code != Some(0) {
            anyhow::bail!("run {run_id} exited with non-zero code");
        }

        let capsule_dir = run
            .capsule_dir
            .ok_or_else(|| anyhow::anyhow!("run has no capsule_dir"))?;

        let script = std::fs::read_to_string(Path::new(&capsule_dir).join("script.sh"))?;
        let result_path = Path::new(&capsule_dir).join("result.json");
        let result_summary = std::fs::read_to_string(&result_path).unwrap_or_default();
        let summary = run.summary.unwrap_or_default();

        let draft_id = format!("draft_{}", Uuid::new_v4());
        let staging = self
            .paths
            .group_workspace(&self.agent_group)
            .join("skills-staging")
            .join(&draft_id);
        std::fs::create_dir_all(&staging)?;

        let skill_name = suggest_name(run_id);
        let skill_md = build_skill_md(run_id, &skill_name, &summary, &result_summary);
        std::fs::write(staging.join("SKILL.md"), skill_md)?;
        std::fs::create_dir_all(staging.join("scripts"))?;
        std::fs::write(staging.join("scripts/run.sh"), script)?;

        let provenance = json!({
            "run_id": run_id,
            "capsule_dir": capsule_dir,
            "summary": summary,
        });
        std::fs::write(
            staging.join("provenance.json"),
            serde_json::to_string_pretty(&provenance)?,
        )?;

        let manifest = serde_yaml::to_string(&serde_json::json!({
            "executor_profile": "bwrap-default",
            "trust": "agent-created",
        }))?;
        std::fs::write(staging.join("manifest.yaml"), manifest)?;

        sqlx::query(
            "INSERT INTO skill_drafts (id, agent_group, name, staging_path, provenance, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .bind(&draft_id)
        .bind(&self.agent_group)
        .bind(&skill_name)
        .bind(staging.display().to_string())
        .bind(provenance.to_string())
        .bind("staged")
        .bind(chrono::Utc::now().timestamp_millis() as f64 / 1000.0)
        .execute(state.pool())
        .await?;

        Ok(draft_id)
    }

    /// Draft from a successful run and promote immediately (no operator step).
    pub async fn draft_and_promote_from_run(
        &self,
        state: &StateDb,
        run_id: &str,
    ) -> anyhow::Result<String> {
        let draft_id = self.draft_from_run(state, run_id).await?;
        self.promote_draft(state, &draft_id).await
    }

    pub async fn list_drafts(&self, state: &StateDb) -> anyhow::Result<Vec<SkillDraft>> {
        let rows = sqlx::query_as::<_, (String, String, Option<String>, String, String, f64)>(
            "SELECT id, agent_group, name, staging_path, status, created_at
             FROM skill_drafts WHERE agent_group = ?1 AND status = 'staged'
             ORDER BY created_at DESC",
        )
        .bind(&self.agent_group)
        .fetch_all(state.pool())
        .await?;

        Ok(rows
            .into_iter()
            .map(
                |(id, agent_group, name, staging_path, status, created_at)| SkillDraft {
                    id,
                    agent_group,
                    name,
                    staging_path,
                    status,
                    created_at,
                },
            )
            .collect())
    }

    pub async fn promote_draft(&self, state: &StateDb, draft_id: &str) -> anyhow::Result<String> {
        let staging = self
            .paths
            .group_workspace(&self.agent_group)
            .join("skills-staging")
            .join(draft_id);
        if !staging.exists() {
            anyhow::bail!("draft not found: {draft_id}");
        }

        let report = guard_skill_dir(&staging, TrustLevel::AgentCreated);
        if report.verdict == GuardVerdict::Dangerous {
            anyhow::bail!("guard blocked promotion: {:?}", report.findings);
        }

        let skill_md = std::fs::read_to_string(staging.join("SKILL.md"))?;
        let name = parse_name_from_skill(&skill_md)?;
        let dest = self
            .paths
            .group_workspace(&self.agent_group)
            .join("skills")
            .join(&name);
        if dest.exists() {
            anyhow::bail!("skill already exists: {name}");
        }
        copy_dir_all(&staging, &dest)?;

        let mut skill_state =
            bobaclaw_skills::SkillStateStore::load(&self.paths.group_workspace(&self.agent_group))?;
        skill_state.mark_agent_created(&name)?;
        skill_state.set_enabled(&name, true)?;

        sqlx::query("UPDATE skill_drafts SET status = 'promoted' WHERE id = ?1")
            .bind(draft_id)
            .execute(state.pool())
            .await?;

        Ok(name)
    }

    /// Promote without DB (legacy CLI path when state unavailable).
    pub fn promote_draft_fs(&self, draft_id: &str) -> anyhow::Result<String> {
        let staging = self
            .paths
            .group_workspace(&self.agent_group)
            .join("skills-staging")
            .join(draft_id);
        if !staging.exists() {
            anyhow::bail!("draft not found: {draft_id}");
        }

        let report = guard_skill_dir(&staging, TrustLevel::AgentCreated);
        if report.verdict == GuardVerdict::Dangerous {
            anyhow::bail!("guard blocked promotion: {:?}", report.findings);
        }

        let skill_md = std::fs::read_to_string(staging.join("SKILL.md"))?;
        let name = parse_name_from_skill(&skill_md)?;
        let dest = self
            .paths
            .group_workspace(&self.agent_group)
            .join("skills")
            .join(&name);
        if dest.exists() {
            anyhow::bail!("skill already exists: {name}");
        }
        copy_dir_all(&staging, &dest)?;

        let mut skill_state =
            bobaclaw_skills::SkillStateStore::load(&self.paths.group_workspace(&self.agent_group))?;
        skill_state.mark_agent_created(&name)?;
        skill_state.set_enabled(&name, true)?;

        Ok(name)
    }
}

fn build_skill_md(run_id: &str, skill_name: &str, summary: &str, result_summary: &str) -> String {
    let summary_section = if summary.trim().is_empty() {
        "See source run result below.".to_string()
    } else {
        summary.trim().to_string()
    };

    format!(
        "---\nname: {skill_name}\ndescription: Auto-drafted from successful run {run_id}\nversion: 0.1.0\nmetadata:\n  bobaclaw:\n    tags: [auto, draft]\n---\n\n# {skill_name}\n\n## When to Use\n\nRepeatable task derived from a successful BobaClaw run.\n\n## Summary\n\n{summary_section}\n\n## Procedure\n\n1. Review `scripts/run.sh` for the exact commands used.\n2. Run via executor profile `bwrap-default` in the workspace sandbox.\n3. Verify exit code 0 and expected output.\n\n## Verification\n\nCheck `result.json` exit code and summary.\n\n## Source Run\n\n- run_id: `{run_id}`\n- result: ```json\n{result_summary}\n```\n"
    )
}

fn suggest_name(run_id: &str) -> String {
    format!(
        "skill-from-{}",
        run_id
            .trim_start_matches("run_")
            .chars()
            .take(8)
            .collect::<String>()
    )
}

fn parse_name_from_skill(md: &str) -> anyhow::Result<String> {
    let Some(rest) = md.strip_prefix("---") else {
        anyhow::bail!("SKILL.md missing name in frontmatter");
    };
    if let Some(end) = rest.find("\n---") {
        let front = &rest[..end];
        let fm: serde_yaml::Value = serde_yaml::from_str(front)?;
        if let Some(name) = fm.get("name").and_then(|v| v.as_str()) {
            return Ok(name.to_string());
        }
    }
    anyhow::bail!("SKILL.md missing name in frontmatter")
}

fn copy_dir_all(src: &Path, dst: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &to)?;
        } else {
            std::fs::copy(entry.path(), to)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bobaclaw_state::StateDb;

    #[test]
    fn build_skill_md_includes_summary() {
        let md = build_skill_md("run_abc", "skill-from-abc", "did the thing", "{}");
        assert!(md.contains("did the thing"));
        assert!(md.contains("name: skill-from-abc"));
    }

    #[tokio::test]
    async fn draft_rejects_failed_run() {
        let dir = tempfile::tempdir().unwrap();
        let paths = BobaPaths::from_home(dir.path().to_path_buf());
        std::fs::create_dir_all(paths.group_workspace("home").join("skills-staging")).unwrap();
        let state = StateDb::open(&paths.state_db).await.unwrap();
        let ledger = RunLedger::new(state.pool());
        ledger
            .create_run("run_fail", None, None, "bwrap-default")
            .await
            .unwrap();
        ledger
            .mark_completed("run_fail", 1, "failed")
            .await
            .unwrap();

        let forge = SkillForge::new(paths, "home".into());
        let err = forge.draft_from_run(&state, "run_fail").await.unwrap_err();
        assert!(err.to_string().contains("not successful"));
    }
}
