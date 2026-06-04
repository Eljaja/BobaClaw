//! Writable dirs under the agent workspace for package managers inside bwrap.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::profile::ExecutorProfile;

const SANDBOX_ROOT: &str = ".bobaclaw-sandbox";

/// Host path → guest path for package installs (networked profile only).
const PACKAGE_BINDS: &[(&str, &str)] = &[
    ("usr-local", "/usr/local"),
    ("var-cache-apt", "/var/cache/apt"),
    ("var-lib-apt", "/var/lib/apt"),
    ("var-lib-dpkg", "/var/lib/dpkg"),
    ("home", "/home/sandbox"),
];

const PACKAGE_RO_BINDS: &[(&str, &str)] = &[
    ("/etc/apt", "/etc/apt"),
    ("/etc/passwd", "/etc/passwd"),
    ("/etc/group", "/etc/group"),
    ("/etc/hosts", "/etc/hosts"),
];

const NETWORK_RO_BINDS: &[(&str, &str)] = &[
    ("/etc/resolv.conf", "/etc/resolv.conf"),
    ("/etc/nsswitch.conf", "/etc/nsswitch.conf"),
    ("/etc/ssl", "/etc/ssl"),
    ("/etc/ca-certificates", "/etc/ca-certificates"),
    ("/etc/hosts", "/etc/hosts"),
];

pub fn sandbox_root(workspace: &Path) -> PathBuf {
    workspace.join(SANDBOX_ROOT)
}

pub fn prepare_package_dirs(workspace: &Path) -> std::io::Result<()> {
    let root = sandbox_root(workspace);
    for (name, _) in PACKAGE_BINDS {
        std::fs::create_dir_all(root.join(name))?;
    }
    Ok(())
}

pub fn append_sandbox_args(cmd: &mut Command, profile: &ExecutorProfile, workspace: &Path) {
    if profile.allow_network {
        cmd.arg("--share-net");
        for (host, guest) in NETWORK_RO_BINDS {
            if Path::new(host).exists() {
                cmd.args(["--ro-bind", host, guest]);
            }
        }
    }

    if !profile.allow_package_install {
        return;
    }

    let _ = prepare_package_dirs(workspace);
    let root = sandbox_root(workspace);

    cmd.args(["--proc", "/proc"]);
    cmd.args(["--tmpfs", "/tmp"]);

    for (name, guest) in PACKAGE_BINDS {
        let host = root.join(name);
        if host.exists() {
            cmd.args(["--bind", host.to_str().unwrap(), guest]);
        }
    }

    for (host, guest) in PACKAGE_RO_BINDS {
        if Path::new(host).exists() {
            cmd.args(["--ro-bind", host, guest]);
        }
    }

    cmd.args(["--setenv", "HOME", "/home/sandbox"]);
    cmd.args(["--setenv", "TMPDIR", "/tmp"]);
    cmd.args([
        "--setenv",
        "DEBIAN_FRONTEND",
        "noninteractive",
    ]);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::ExecutorProfile;

    #[test]
    fn prepares_sandbox_directories() {
        let dir = tempfile::tempdir().unwrap();
        prepare_package_dirs(dir.path()).unwrap();
        assert!(sandbox_root(dir.path()).join("usr-local").is_dir());
        assert!(sandbox_root(dir.path()).join("var-lib-apt").is_dir());
    }

    #[test]
    fn networked_profile_enables_packages_by_default() {
        let p = ExecutorProfile::from_config(true, true);
        assert!(p.allow_network);
        assert!(p.allow_package_install);
    }
}
