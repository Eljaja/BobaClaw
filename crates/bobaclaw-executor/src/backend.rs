use std::path::Path;

use bobaclaw_core::{ExecutorBackend, ExecutorConfig};

use crate::bwrap::BwrapExecutor;
use crate::docker::DockerExecutor;
use crate::profile::ExecutorProfile;
use crate::run::ExecutionResult;

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
        match executor.backend {
            ExecutorBackend::Bubblewrap => {
                BwrapExecutor::exec_command(profile, workspace, run_dir, command)
            }
            ExecutorBackend::Docker => DockerExecutor::exec_command(
                executor,
                profile,
                workspace_root,
                workspace,
                run_dir,
                command,
            ),
        }
    }
}
