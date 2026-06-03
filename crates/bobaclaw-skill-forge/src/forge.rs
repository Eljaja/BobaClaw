use std::path::Path;

use bobaclaw_core::BobaPaths;
use bobaclaw_skills::{guard_skill_dir, GuardVerdict, TrustLevel};
use bobaclaw_state::{RunLedger, StateDb};
use serde_json::json;
use uuid::Uuid;

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

        let capsule_dir = run
            .capsule_dir
            .ok_or_else(|| anyhow::anyhow!("run has no capsule_dir"))?;

        let script = std::fs::read_to_string(Path::new(&capsule_dir).join("script.sh"))?;
        let result_path = Path::new(&capsule_dir).join("result.json");
        let result_summary = std::fs::read_to_string(&result_path).unwrap_or_default();

        let draft_id = format!("draft_{}", Uuid::new_v4());
        let staging = self
            .paths
            .group_workspace(&self.agent_group)
            .join("skills-staging")
            .join(&draft_id);
        std::fs::create_dir_all(&staging)?;

        let skill_name = suggest_name(run_id);
        let skill_md = format!(
            "---\nname: {skill_name}\ndescription: Auto-drafted from run {run_id}\nversion: 0.1.0\nmetadata:\n  bobaclaw:\n    tags: [auto, draft]\n---\n\n# {skill_name}\n\n## When to Use\n\nRepeatable task derived from a successful BobaClaw run.\n\n## Procedure\n\nRun the bundled script in `scripts/run.sh` via executor profile `bwrap-default`.\n\n## Verification\n\nCheck `result.json` exit code and summary.\n\n## Source Run\n\n- run_id: `{run_id}`\n- result: ```json\n{result_summary}\n```\n"
        );
        std::fs::write(staging.join("SKILL.md"), skill_md)?;
        std::fs::create_dir_all(staging.join("scripts"))?;
        std::fs::write(staging.join("scripts/run.sh"), script)?;

        let provenance = json!({
            "run_id": run_id,
            "capsule_dir": capsule_dir,
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

    pub fn promote_draft(&self, draft_id: &str) -> anyhow::Result<String> {
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
            anyhow::bail!(
                "guard blocked promotion: {:?}",
                report.findings
            );
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
        Ok(name)
    }
}

fn suggest_name(run_id: &str) -> String {
    format!("skill-from-{}", run_id.trim_start_matches("run_").chars().take(8).collect::<String>())
}

fn parse_name_from_skill(md: &str) -> anyhow::Result<String> {
    if md.starts_with("---") {
        if let Some(end) = md[3..].find("\n---") {
            let front = &md[3..3 + end];
            let fm: serde_yaml::Value = serde_yaml::from_str(front)?;
            if let Some(name) = fm.get("name").and_then(|v| v.as_str()) {
                return Ok(name.to_string());
            }
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
