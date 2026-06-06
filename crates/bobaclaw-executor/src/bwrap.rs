use std::path::Path;
use std::process::Command;

use bobaclaw_core::CommandCapsuleManifest;

use crate::doctor::check_bwrap;
use crate::profile::{ExecutorProfile, ProfileKind};
use crate::run::{ExecutionResult, RunArtifacts};
use crate::sandbox::{adapt_command_for_package_sandbox, append_sandbox_args, prepare_package_dirs};

pub struct BwrapExecutor;

impl BwrapExecutor {
    /// Run a shell command with the agent workspace mounted read-write at `/workspace`.
    pub fn exec_command(
        profile: &ExecutorProfile,
        workspace: &Path,
        run_dir: &Path,
        command: &str,
    ) -> anyhow::Result<ExecutionResult> {
        std::fs::create_dir_all(workspace)?;
        std::fs::create_dir_all(run_dir)?;
        if profile.allow_package_install {
            prepare_package_dirs(workspace)?;
        }

        let check = check_bwrap();
        if !check.user_ns_ok {
            anyhow::bail!("bubblewrap unavailable: {}", check.message);
        }

        let bwrap = which_bwrap()?;
        let workspace = workspace.canonicalize()?;
        let run_dir = run_dir.canonicalize()?;

        let mut cmd = Command::new(&bwrap);
        append_base_ro_binds(&mut cmd);
        cmd.args([
            "--bind",
            workspace.to_str().unwrap(),
            "/workspace",
            "--bind",
            run_dir.to_str().unwrap(),
            "/capsule",
            "--chdir",
            "/workspace",
            "--dev",
            "/dev",
        ]);
        append_sandbox_args(&mut cmd, profile, &workspace);
        let shell_command = if profile.allow_package_install {
            adapt_command_for_package_sandbox(command)
        } else {
            command.to_string()
        };
        cmd.args(["--", "/bin/bash", "-lc", &shell_command]);

        let output = cmd.output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let code = output.status.code().unwrap_or(1);

        let manifest = CommandCapsuleManifest {
            language: "bash".into(),
            argv: vec!["/bin/bash".into(), "-lc".into(), shell_command.clone()],
            executor_profile: profile.id().into(),
            timeout_secs: 120,
            network: profile.allow_network,
        };
        let artifacts = RunArtifacts::prepare(&run_dir, &shell_command, &manifest)?;
        artifacts.write_result(code, &stdout, &stderr)
    }

    pub fn execute(
        profile: &ExecutorProfile,
        run_dir: &Path,
        script: &str,
        manifest: &CommandCapsuleManifest,
    ) -> anyhow::Result<ExecutionResult> {
        let artifacts = RunArtifacts::prepare(run_dir, script, manifest)?;

        match profile.kind {
            ProfileKind::HostDanger => {
                anyhow::bail!("host-danger requires explicit approval; not implemented in this run path")
            }
            ProfileKind::BwrapDefault | ProfileKind::BwrapNetworked | ProfileKind::Readonly => {
                Self::run_bwrap(profile, &artifacts)
            }
            ProfileKind::DockerDefault | ProfileKind::DockerNetworked => {
                anyhow::bail!("docker profiles require SandboxExecutor::exec_command")
            }
            ProfileKind::SystemdRun => Self::run_systemd_run(&artifacts),
        }
    }

    fn run_bwrap(profile: &ExecutorProfile, artifacts: &RunArtifacts) -> anyhow::Result<ExecutionResult> {
        let check = check_bwrap();
        if !check.user_ns_ok {
            anyhow::bail!("bubblewrap unavailable: {}", check.message);
        }

        let bwrap = which_bwrap()?;
        let work = artifacts.run_dir.canonicalize()?;
        let mut cmd = Command::new(&bwrap);
        append_base_ro_binds(&mut cmd);
        cmd.args([
            "--bind",
            work.to_str().unwrap(),
            "/work",
            "--chdir",
            "/work",
            "--dev",
            "/dev",
        ]);
        append_sandbox_args(&mut cmd, profile, &work);
        cmd.args(["--", "/work/script.sh"]);

        let output = cmd.output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let code = output.status.code().unwrap_or(1);
        artifacts.write_result(code, &stdout, &stderr)
    }

    fn run_systemd_run(artifacts: &RunArtifacts) -> anyhow::Result<ExecutionResult> {
        let wd = artifacts.run_dir.display().to_string();
        let output = Command::new("systemd-run")
            .arg("--wait")
            .arg("--collect")
            .arg("--pipe")
            .arg(format!("--working-directory={wd}"))
            .arg(format!("{}/script.sh", artifacts.run_dir.display()))
            .output();

        match output {
            Ok(out) if out.status.success() || out.stderr.is_empty() => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                let code = out.status.code().unwrap_or(1);
                artifacts.write_result(code, &stdout, &stderr)
            }
            _ => Self::run_bwrap(&ExecutorProfile::bwrap_default(), artifacts),
        }
    }
}

fn append_base_ro_binds(cmd: &mut Command) {
    cmd.args([
        "--unshare-all",
        "--die-with-parent",
        "--new-session",
        "--ro-bind",
        "/usr",
        "/usr",
        "--ro-bind",
        "/bin",
        "/bin",
        "--ro-bind",
        "/lib",
        "/lib",
    ]);
    if Path::new("/lib64").exists() {
        cmd.args(["--ro-bind", "/lib64", "/lib64"]);
    }
}

fn which_bwrap() -> anyhow::Result<String> {
    for path in ["/usr/bin/bwrap", "/bin/bwrap"] {
        if Path::new(path).exists() {
            return Ok(path.into());
        }
    }
    let out = Command::new("which").arg("bwrap").output()?;
    if out.status.success() {
        let s = String::from_utf8(out.stdout)?.trim().to_string();
        if !s.is_empty() {
            return Ok(s);
        }
    }
    anyhow::bail!("bwrap not found")
}
