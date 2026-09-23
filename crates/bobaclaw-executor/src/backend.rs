use std::path::Path;

use bobaclaw_core::{ExecutorBackend, ExecutorConfig};

use crate::bwrap::{BwrapExecutor, SandboxEnv};
use crate::docker::DockerExecutor;
use crate::profile::ExecutorProfile;
use crate::run::ExecutionResult;
use crate::sandbox::{adapt_command_for_sandbox, SandboxCommandMode};

pub struct SandboxExecutor;

impl SandboxExecutor {
    pub fn exec_command(
        executor: &ExecutorConfig,
        profile: &ExecutorProfile,
        workspace_root: &Path,
        workspace: &Path,
        run_dir: &Path,
        command: &str,
    ) -> anyhow::Result<ExecutionResult> {
        Self::exec_command_with_secrets(
            executor,
            profile,
            workspace_root,
            workspace,
            run_dir,
            command,
            &[],
        )
    }

    /// Like [`Self::exec_command`], but hands `secrets` (`NAME`, value) to the sandboxed
    /// child process only. Secret values never appear in the command text, `script.sh`,
    /// `capsule.yaml`, logs, or the host process argv.
    pub fn exec_command_with_secrets(
        executor: &ExecutorConfig,
        profile: &ExecutorProfile,
        workspace_root: &Path,
        workspace: &Path,
        run_dir: &Path,
        command: &str,
        secrets: &[(String, String)],
    ) -> anyhow::Result<ExecutionResult> {
        let command = prepare_command(executor, profile, command);
        match executor.backend {
            ExecutorBackend::Bubblewrap => {
                let env = SandboxEnv {
                    passthrough: executor.env_passthrough.clone(),
                    secrets: secrets.to_vec(),
                };
                BwrapExecutor::exec_command(profile, workspace, run_dir, &command, &env)
            }
            ExecutorBackend::Docker => DockerExecutor::exec_command(
                executor,
                profile,
                workspace_root,
                workspace,
                run_dir,
                &command,
                secrets,
            ),
        }
    }
}

fn prepare_command(executor: &ExecutorConfig, profile: &ExecutorProfile, command: &str) -> String {
    match executor.backend {
        ExecutorBackend::Docker if executor.network => {
            adapt_command_for_sandbox(command, SandboxCommandMode::Docker)
        }
        ExecutorBackend::Bubblewrap if profile.allow_package_install => {
            adapt_command_for_sandbox(command, SandboxCommandMode::BwrapPackages)
        }
        _ => command.to_string(),
    }
}
