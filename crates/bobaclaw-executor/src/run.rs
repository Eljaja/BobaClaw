use std::path::{Path, PathBuf};

use bobaclaw_core::CommandCapsuleManifest;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub exit_code: i32,
    pub stdout_path: PathBuf,
    pub stderr_path: PathBuf,
    pub result_json_path: PathBuf,
    pub summary: String,
}

#[derive(Debug, Clone)]
pub struct RunArtifacts {
    pub run_dir: PathBuf,
    pub script_path: PathBuf,
    pub manifest_path: PathBuf,
}

impl RunArtifacts {
    pub fn prepare(
        run_dir: &Path,
        script: &str,
        manifest: &CommandCapsuleManifest,
    ) -> anyhow::Result<Self> {
        std::fs::create_dir_all(run_dir)?;
        let script_path = run_dir.join("script.sh");
        std::fs::write(&script_path, script)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))?;
        }
        let manifest_path = run_dir.join("capsule.yaml");
        let yaml = serde_yaml::to_string(manifest)?;
        std::fs::write(&manifest_path, yaml)?;
        Ok(Self {
            run_dir: run_dir.to_path_buf(),
            script_path,
            manifest_path,
        })
    }

    pub fn write_result(
        &self,
        exit_code: i32,
        stdout: &str,
        stderr: &str,
    ) -> anyhow::Result<ExecutionResult> {
        let stdout_path = self.run_dir.join("stdout.log");
        let stderr_path = self.run_dir.join("stderr.log");
        std::fs::write(&stdout_path, stdout)?;
        std::fs::write(&stderr_path, stderr)?;

        let summary = if exit_code == 0 {
            truncate(stdout, 500)
        } else {
            truncate(stderr, 500)
        };

        let result = serde_json::json!({
            "exit_code": exit_code,
            "summary": summary,
        });
        let result_json_path = self.run_dir.join("result.json");
        std::fs::write(&result_json_path, serde_json::to_string_pretty(&result)?)?;

        Ok(ExecutionResult {
            exit_code,
            stdout_path,
            stderr_path,
            result_json_path,
            summary,
        })
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}
