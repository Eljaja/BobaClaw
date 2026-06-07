mod backend;
mod bwrap;
mod docker;
mod doctor;
mod profile;
mod run;
mod sandbox;

pub use backend::SandboxExecutor;
pub use bwrap::BwrapExecutor;
pub use docker::ensure_container;
pub use doctor::{check_bwrap, check_docker, check_docker_sandbox};
pub use profile::{ExecutorProfile, ProfileKind};
pub use run::{ExecutionResult, RunArtifacts};
pub use sandbox::{bwrap_apt_advisory, bwrap_apt_supported};
