use bobaclaw_core::ExecutorBackend;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileKind {
    BwrapDefault,
    BwrapNetworked,
    DockerDefault,
    DockerNetworked,
    Readonly,
    SystemdRun,
    HostDanger,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutorProfile {
    pub kind: ProfileKind,
    pub allow_network: bool,
    /// Writable package-manager paths under workspace `.bobaclaw-sandbox/`.
    pub allow_package_install: bool,
    pub readonly_root: bool,
}

impl ExecutorProfile {
    pub fn bwrap_default() -> Self {
        Self {
            kind: ProfileKind::BwrapDefault,
            allow_network: false,
            allow_package_install: false,
            readonly_root: true,
        }
    }

    pub fn bwrap_networked() -> Self {
        Self {
            kind: ProfileKind::BwrapNetworked,
            allow_network: true,
            allow_package_install: true,
            readonly_root: true,
        }
    }

    pub fn from_network_enabled(network: bool) -> Self {
        Self::from_config(network, network)
    }

    pub fn from_config(network: bool, sandbox_packages: bool) -> Self {
        Self::from_config_with_backend(ExecutorBackend::Bubblewrap, network, sandbox_packages)
    }

    pub fn from_config_with_backend(
        backend: ExecutorBackend,
        network: bool,
        sandbox_packages: bool,
    ) -> Self {
        match backend {
            ExecutorBackend::Bubblewrap => {
                if network {
                    Self {
                        kind: ProfileKind::BwrapNetworked,
                        allow_network: true,
                        allow_package_install: sandbox_packages,
                        readonly_root: true,
                    }
                } else {
                    Self {
                        kind: ProfileKind::BwrapDefault,
                        allow_network: false,
                        allow_package_install: false,
                        readonly_root: true,
                    }
                }
            }
            ExecutorBackend::Docker => {
                if network {
                    Self {
                        kind: ProfileKind::DockerNetworked,
                        allow_network: true,
                        allow_package_install: false,
                        readonly_root: true,
                    }
                } else {
                    Self {
                        kind: ProfileKind::DockerDefault,
                        allow_network: false,
                        allow_package_install: false,
                        readonly_root: true,
                    }
                }
            }
        }
    }

    pub fn host_danger() -> Self {
        Self {
            kind: ProfileKind::HostDanger,
            allow_network: true,
            allow_package_install: true,
            readonly_root: false,
        }
    }

    pub fn id(&self) -> &'static str {
        match self.kind {
            ProfileKind::BwrapDefault => "bwrap-default",
            ProfileKind::BwrapNetworked => "bwrap-networked",
            ProfileKind::DockerDefault => "docker-default",
            ProfileKind::DockerNetworked => "docker-networked",
            ProfileKind::Readonly => "readonly",
            ProfileKind::SystemdRun => "systemd-run",
            ProfileKind::HostDanger => "host-danger",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_ids() {
        assert_eq!(ExecutorProfile::bwrap_default().id(), "bwrap-default");
        assert_eq!(
            ExecutorProfile::from_network_enabled(true).id(),
            "bwrap-networked"
        );
    }

    #[test]
    fn network_flag() {
        assert!(!ExecutorProfile::bwrap_default().allow_network);
        assert!(ExecutorProfile::from_config(true, false).allow_network);
    }

    #[test]
    fn packages_follow_config() {
        let on = ExecutorProfile::from_config(true, true);
        assert!(on.allow_package_install);
        let off = ExecutorProfile::from_config(true, false);
        assert!(!off.allow_package_install);
    }

    #[test]
    fn docker_profile_ids() {
        let net = ExecutorProfile::from_config_with_backend(ExecutorBackend::Docker, true, true);
        assert_eq!(net.id(), "docker-networked");
        assert!(!net.allow_package_install);
        let off = ExecutorProfile::from_config_with_backend(ExecutorBackend::Docker, false, true);
        assert_eq!(off.id(), "docker-default");
    }
}
