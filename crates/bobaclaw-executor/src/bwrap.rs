use std::path::Path;
use std::process::Command;

use bobaclaw_core::CommandCapsuleManifest;

use crate::doctor::check_bwrap;
use crate::profile::{ExecutorProfile, ProfileKind};
use crate::run::{ExecutionResult, RunArtifacts};

pub struct BwrapExecutor;

impl BwrapExecutor {
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

        cmd.args([
            "--bind",
            work.to_str().unwrap(),
            "/work",
            "--chdir",
            "/work",
            "--dev",
            "/dev",
            "--",
            "/work/script.sh",
        ]);

        if !profile.allow_network {
            // default bwrap has no network namespace egress without --share-net
        } else {
            cmd.arg("--share-net");
        }

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
