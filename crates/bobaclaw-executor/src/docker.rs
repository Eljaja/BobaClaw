use std::path::Path;
use std::process::Command;

use bobaclaw_core::{CommandCapsuleManifest, DockerExecutorConfig, ExecutorConfig};

use crate::bwrap::is_valid_env_name;
use crate::docker_mount::docker_bind_source;
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
        secrets: &[(String, String)],
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

        let manifest = CommandCapsuleManifest {
            language: "bash".into(),
            argv: vec!["/bin/bash".into(), "-lc".into(), command.into()],
            executor_profile: profile.id().into(),
            timeout_secs: 120,
            network: profile.allow_network,
        };
        let artifacts = RunArtifacts::prepare(&run_dir, command, &manifest)?;

        let mut cmd = build_exec_command(
            &executor.docker.container_name,
            &container_workdir,
            command,
            secrets,
        )?;
        let output = cmd.output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let code = output.status.code().unwrap_or(1);
        artifacts.write_result(code, &stdout, &stderr)
    }
}

/// `docker exec` never forwards the host environment: the container sees only its image
/// env. Secrets are passed as `-e NAME` (no value in argv) with the value set on the
/// docker CLI process env, so they reach the exec'd child only.
fn build_exec_command(
    container_name: &str,
    container_workdir: &str,
    command: &str,
    secrets: &[(String, String)],
) -> anyhow::Result<Command> {
    let mut cmd = Command::new("docker");
    cmd.args(["exec", "-w", container_workdir]);
    for (name, value) in secrets {
        if !is_valid_env_name(name) {
            anyhow::bail!("invalid secret env var name: {name:?}");
        }
        cmd.args(["-e", name.as_str()]);
        cmd.env(name, value);
    }
    cmd.args([container_name, "/bin/bash", "-lc", command]);
    Ok(cmd)
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

    let workspace_abs = docker_bind_source(workspace_root)?;
    let runs_abs = docker_bind_source(runs_root)?;
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

    let args = create_args(
        workspace_abs,
        runs_abs,
        image,
        container_name,
        executor.network,
    );
    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_docker(&arg_refs)?;
    Ok(())
}

/// `docker create` argv. No `-e` / `--env-file`: the sandbox container never receives
/// host env (API keys stay in the gateway process).
fn create_args(
    workspace_abs: &Path,
    runs_abs: &Path,
    image: &str,
    container_name: &str,
    network: bool,
) -> Vec<String> {
    let network = if network { "bridge" } else { "none" };
    vec![
        "create".to_string(),
        "--name".to_string(),
        container_name.to_string(),
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
        image.to_string(),
        "sleep".to_string(),
        "infinity".to_string(),
    ]
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
    fn exec_command_passes_secret_by_name_only() {
        let secrets = vec![(
            "OPENAI_API_KEY".to_string(),
            "sk-test-docker-must-not-leak".to_string(),
        )];
        let cmd = build_exec_command("sbx", "/workspace", "codex exec 'hi'", &secrets).unwrap();
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            args,
            vec![
                "exec",
                "-w",
                "/workspace",
                "-e",
                "OPENAI_API_KEY",
                "sbx",
                "/bin/bash",
                "-lc",
                "codex exec 'hi'"
            ]
        );
        assert!(!args.join(" ").contains("sk-test-docker-must-not-leak"));
        let envs: Vec<_> = cmd.get_envs().collect();
        assert_eq!(envs.len(), 1);
        assert!(build_exec_command("sbx", "/w", "true", &[("A=B".into(), "x".into())]).is_err());
    }

    #[test]
    fn exec_and_create_do_not_forward_host_env() {
        std::env::set_var("BOBACLAW_TEST_DOCKER_FAKE_KEY", "sk-host-only");
        let cmd = build_exec_command("sbx", "/workspace", "printenv", &[]).unwrap();
        assert_eq!(cmd.get_envs().count(), 0);
        let exec_args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let create = create_args(
            Path::new("/data/workspace"),
            Path::new("/data/runs"),
            "bobaclaw/sandbox:latest",
            "sbx",
            true,
        );
        for args in [&exec_args, &create] {
            assert!(!args
                .iter()
                .any(|a| a == "-e" || a.starts_with("--env") || a.contains("sk-host-only")));
        }
        std::env::remove_var("BOBACLAW_TEST_DOCKER_FAKE_KEY");
    }

    #[test]
    fn container_workdir_root() {
        let root = PathBuf::from("/home/user/.bobaclaw/workspace");
        assert_eq!(container_workdir(&root, &root).unwrap(), "/workspace");
    }
}
