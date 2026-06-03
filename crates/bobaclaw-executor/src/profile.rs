use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProfileKind {
    BwrapDefault,
    BwrapNetworked,
    Readonly,
    SystemdRun,
    HostDanger,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutorProfile {
    pub kind: ProfileKind,
    pub allow_network: bool,
    pub readonly_root: bool,
}

impl ExecutorProfile {
    pub fn bwrap_default() -> Self {
        Self {
            kind: ProfileKind::BwrapDefault,
            allow_network: false,
            readonly_root: true,
        }
    }

    pub fn host_danger() -> Self {
        Self {
            kind: ProfileKind::HostDanger,
            allow_network: true,
            readonly_root: false,
        }
    }

    pub fn id(&self) -> &'static str {
        match self.kind {
            ProfileKind::BwrapDefault => "bwrap-default",
            ProfileKind::BwrapNetworked => "bwrap-networked",
            ProfileKind::Readonly => "readonly",
            ProfileKind::SystemdRun => "systemd-run",
            ProfileKind::HostDanger => "host-danger",
        }
    }
}
