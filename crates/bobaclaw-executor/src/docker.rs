use std::path::Path;
use std::process::Command;

use bobaclaw_core::{CommandCapsuleManifest, DockerExecutorConfig, ExecutorConfig};

use crate::profile::ExecutorProfile;
use crate::run::{ExecutionResult, RunArtifacts};

const SANDBOX_LABEL: &str = "bobaclaw.sandbox=1";
const SPEC_FILE: &str = "sandbox-container.json";

#[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
struct ContainerSpec {
    image: String,
    network: bool,
    container_name: String,
}

pub struct DockerExecutor;

impl DockerExecutor {
    pub fn exec_command(
        executor: &ExecutorConfig,
        profile: &ExecutorProfile,
        workspace_root: &Path,
        workspace: &Path,
        run_dir: &Path,
        command: &str,
    ) -> anyhow::Result<ExecutionResult> {
        std::fs::create_dir_all(workspace_root)?;
        std::fs::create_dir_all(workspace)?;
        std::fs::create_dir_all(run_dir)?;

        let home = workspace_root
            .parent()
            .ok_or_else(|| anyhow::anyhow!("workspace has no parent directory"))?;
        let runs_root = home.join("runs");
        std::fs::create_dir_all(&runs_root)?;

        ensure_container(home, workspace_root, &runs_root, executor)?;

        let workspace_root = workspace_root.canonicalize()?;
        let workspace = workspace.canonicalize()?;
        let run_dir = run_dir.canonicalize()?;
        let container_workdir = container_workdir(&workspace_root, &workspace)?;

        let mut cmd = Command::new("docker");
        cmd.args([
            "exec",
            "-w",
            &container_workdir,
            &executor.docker.container_name,
            "/bin/bash",
            "-lc",
            command,
        ]);

        let output = cmd.output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let code = output.status.code().unwrap_or(1);

        let manifest = CommandCapsuleManifest {
            language: "bash".into(),
            argv: vec!["/bin/bash".into(), "-lc".into(), command.into()],
            executor_profile: profile.id().into(),
            timeout_secs: 120,
            network: profile.allow_network,
        };
        let artifacts = RunArtifacts::prepare(&run_dir, command, &manifest)?;
        artifacts.write_result(code, &stdout, &stderr)
    }
}

pub fn ensure_container(
    home: &Path,
    workspace_root: &Path,
    runs_root: &Path,
    executor: &ExecutorConfig,
) -> anyhow::Result<()> {
    let check = crate::doctor::check_docker();
    if !check.daemon_ok {
        anyhow::bail!("docker unavailable: {}", check.message);
    }

    let desired = ContainerSpec {
        image: executor.docker.image.clone(),
        network: executor.network,
        container_name: executor.docker.container_name.clone(),
    };

    let spec_path = home.join(SPEC_FILE);
    let stored: Option<ContainerSpec> = std::fs::read_to_string(&spec_path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok());

    let workspace_abs = workspace_root.canonicalize()?;
    let runs_abs = runs_root.canonicalize()?;
    let name = &executor.docker.container_name;

    let needs_recreate = stored.as_ref() != Some(&desired) || !container_exists(name)?;

    if needs_recreate {
        if container_exists(name)? {
            run_docker(&["rm", "-f", name])?;
        }
        create_container(&workspace_abs, &runs_abs, executor)?;
        std::fs::write(&spec_path, serde_json::to_string_pretty(&desired)?)?;
    }

    if !container_running(name)? {
        run_docker(&["start", name])?;
    }

    Ok(())
}

fn create_container(
    workspace_abs: &Path,
    runs_abs: &Path,
    executor: &ExecutorConfig,
) -> anyhow::Result<()> {
    let DockerExecutorConfig {
        image,
        container_name,
    } = &executor.docker;

    let network = if executor.network { "bridge" } else { "none" };

    let args = vec![
        "create".to_string(),
        "--name".to_string(),
        container_name.clone(),
        "--label".to_string(),
        SANDBOX_LABEL.to_string(),
        "--network".to_string(),
        network.to_string(),
        "--cap-drop".to_string(),
        "ALL".to_string(),
        "--init".to_string(),
        "-v".to_string(),
        format!("{}:/workspace", workspace_abs.display()),
        "-v".to_string(),
        format!("{}:/runs", runs_abs.display()),
        image.clone(),
        "sleep".to_string(),
        "infinity".to_string(),
    ];

    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_docker(&arg_refs)?;
    Ok(())
}

fn container_workdir(workspace_root: &Path, workspace: &Path) -> anyhow::Result<String> {
    if workspace == workspace_root {
        return Ok("/workspace".into());
    }
    let rel = workspace
        .strip_prefix(workspace_root)
        .map_err(|_| anyhow::anyhow!("workspace is outside workspace root"))?;
    let rel = rel
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("workspace path is not UTF-8"))?;
    Ok(format!("/workspace/{rel}"))
}

fn container_exists(name: &str) -> anyhow::Result<bool> {
    let out = Command::new("docker")
        .args(["inspect", "-f", "{{.Id}}", name])
        .output()?;
    Ok(out.status.success())
}

fn container_running(name: &str) -> anyhow::Result<bool> {
    let out = Command::new("docker")
        .args(["inspect", "-f", "{{.State.Running}}", name])
        .output()?;
    if !out.status.success() {
        return Ok(false);
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim() == "true")
}

fn run_docker(args: &[&str]) -> anyhow::Result<()> {
    let out = Command::new("docker").args(args).output()?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    anyhow::bail!(
        "docker {} failed: {}",
        args.first().copied().unwrap_or(""),
        stderr.trim()
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn container_workdir_group() {
        let root = PathBuf::from("/home/user/.bobaclaw/workspace");
        let group = root.join("home");
        assert_eq!(container_workdir(&root, &group).unwrap(), "/workspace/home");
    }

    #[test]
    fn container_workdir_root() {
        let root = PathBuf::from("/home/user/.bobaclaw/workspace");
        assert_eq!(container_workdir(&root, &root).unwrap(), "/workspace");
    }
}
