use std::path::Path;

use bobaclaw_core::{ExecutorBackend, ExecutorConfig};

use crate::bwrap::BwrapExecutor;
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
        let command = prepare_command(executor, profile, command);
        match executor.backend {
            ExecutorBackend::Bubblewrap => {
                BwrapExecutor::exec_command(profile, workspace, run_dir, &command)
            }
            ExecutorBackend::Docker => DockerExecutor::exec_command(
                executor,
                profile,
                workspace_root,
                workspace,
                run_dir,
                &command,
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
