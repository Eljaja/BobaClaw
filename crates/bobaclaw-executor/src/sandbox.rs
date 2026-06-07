//! Writable dirs under the agent workspace for package managers inside bwrap.

use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use crate::profile::ExecutorProfile;

const SANDBOX_ROOT: &str = ".bobaclaw-sandbox";

/// Injected via `APT_CONFIG` and bound into `/etc/apt/apt.conf.d/` when possible.
pub const APT_CONF_NAME: &str = "bobaclaw-apt.conf";
pub const APT_DROPIN_NAME: &str = "99bobaclaw.conf";

const APT_CONF_BODY: &str = r#"# BobaClaw bubblewrap — apt method sandbox uses setegid, which bwrap blocks.
APT::Sandbox::User "root";
"#;

const APT_DROPIN_BODY: &str = r#"# BobaClaw — run apt methods as root inside the sandbox.
APT::Sandbox::User "root";
"#;

/// Extra dirs apt/dpkg expect under the writable binds.
const PACKAGE_SUBDIRS: &[&str] = &[
    "var-cache-apt/archives/partial",
    "var-lib-apt/lists/partial",
    "var-lib-apt/lists",
    "var-lib-dpkg/info",
    "var-lib-dpkg/updates",
    "var-log-apt",
];

/// Host path → guest path for package installs (networked profile only).
const PACKAGE_BINDS: &[(&str, &str)] = &[
    ("usr-local", "/usr/local"),
    ("var-cache-apt", "/var/cache/apt"),
    ("var-lib-apt", "/var/lib/apt"),
    ("var-lib-dpkg", "/var/lib/dpkg"),
    ("var-log-apt", "/var/log/apt"),
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
    for sub in PACKAGE_SUBDIRS {
        std::fs::create_dir_all(root.join(sub))?;
    }
    std::fs::write(root.join(APT_CONF_NAME), APT_CONF_BODY)?;
    std::fs::write(root.join(APT_DROPIN_NAME), APT_DROPIN_BODY)?;

    #[cfg(unix)]
    {
        let permissive = std::fs::Permissions::from_mode(0o777);
        for dir in [
            root.join("var-cache-apt"),
            root.join("var-cache-apt/archives"),
            root.join("var-cache-apt/archives/partial"),
            root.join("var-lib-apt"),
            root.join("var-lib-dpkg"),
            root.join("var-log-apt"),
        ] {
            if dir.is_dir() {
                let _ = std::fs::set_permissions(&dir, permissive.clone());
            }
        }
    }
    Ok(())
}

/// Whether bubblewrap is likely to run apt/dpkg successfully on this host.
pub fn bwrap_apt_supported(user_ns_ok: bool) -> bool {
    user_ns_ok && cfg!(target_os = "linux")
}

pub fn bwrap_apt_advisory(user_ns_ok: bool) -> Option<&'static str> {
    if !cfg!(target_os = "linux") {
        return Some(
            "apt/dpkg in bubblewrap need Linux; on macOS set executor.backend: docker \
             (./scripts/build-sandbox-image.sh) for package installs",
        );
    }
    if !user_ns_ok {
        return Some(
            "bubblewrap user namespaces unavailable: apt/dpkg will fail (setuid blocked). \
             Use executor.backend: docker or enable user namespaces on the host",
        );
    }
    None
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

    // Immediately after --unshare-all (in bwrap.rs): map to root inside the user namespace.
    cmd.args(["--uid", "0", "--gid", "0"]);

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

    let apt_conf = root.join(APT_CONF_NAME);
    if apt_conf.exists() {
        cmd.args([
            "--setenv",
            "APT_CONFIG",
            "/workspace/.bobaclaw-sandbox/bobaclaw-apt.conf",
        ]);
    }
    let apt_dropin = root.join(APT_DROPIN_NAME);
    if apt_dropin.exists() {
        let guest = format!("/etc/apt/apt.conf.d/{APT_DROPIN_NAME}");
        cmd.args(["--bind", apt_dropin.to_str().unwrap(), guest.as_str()]);
    }

    cmd.args(["--setenv", "HOME", "/home/sandbox"]);
    cmd.args(["--setenv", "TMPDIR", "/tmp"]);
    cmd.args(["--setenv", "DEBIAN_FRONTEND", "noninteractive"]);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxCommandMode {
    /// Linux bubblewrap with writable `.bobaclaw-sandbox/` binds.
    BwrapPackages,
    /// Long-lived Docker container (`docker exec`); apt needs `APT::Sandbox::User=root`
    /// because the container is created with `no-new-privileges`.
    Docker,
}

/// Normalize agent shell commands before sandbox execution (bwrap or Docker).
pub fn adapt_command_for_sandbox(command: &str, mode: SandboxCommandMode) -> String {
    let cmd = strip_leading_sudo(command.trim());
    if cmd.is_empty() {
        return cmd;
    }

    if !invokes_apt(&cmd) {
        return cmd;
    }

    match mode {
        SandboxCommandMode::BwrapPackages => format!(
            "export APT_CONFIG=/workspace/.bobaclaw-sandbox/bobaclaw-apt.conf; \
             {cmd}"
        ),
        SandboxCommandMode::Docker => format!(
            "mkdir -p /tmp/bobaclaw-apt/{{archives/partial,lists/partial,state}}; \
             printf '%s\\n' \
               'APT::Sandbox::User \"root\";' \
               'Dir::Cache \"/tmp/bobaclaw-apt\";' \
               'Dir::State \"/tmp/bobaclaw-apt/state\";' \
               'Dir::State::lists \"/tmp/bobaclaw-apt/lists\";' \
               > /tmp/bobaclaw-apt/apt.conf; \
             export APT_CONFIG=/tmp/bobaclaw-apt/apt.conf; \
             {cmd}"
        ),
    }
}

fn strip_leading_sudo(command: &str) -> String {
    let mut cmd = command.to_string();
    for prefix in ["sudo -n ", "sudo "] {
        if cmd.starts_with(prefix) {
            cmd = cmd[prefix.len()..].trim_start().to_string();
            break;
        }
    }
    cmd
}

fn invokes_apt(command: &str) -> bool {
    let lower = command.to_lowercase();
    ["apt-get", "apt ", "apt\n", "apt\t", "dpkg "]
        .iter()
        .any(|needle| lower.contains(needle))
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

    #[test]
    fn prepares_apt_config_and_cache_dirs() {
        let dir = tempfile::tempdir().unwrap();
        prepare_package_dirs(dir.path()).unwrap();
        let root = sandbox_root(dir.path());
        assert!(root.join(APT_CONF_NAME).is_file());
        assert!(root.join("var-cache-apt/archives/partial").is_dir());
        let body = std::fs::read_to_string(root.join(APT_CONF_NAME)).unwrap();
        assert!(body.contains("APT::Sandbox::User"));
    }

    #[test]
    fn adapt_strips_sudo_and_injects_apt_config() {
        let out =
            adapt_command_for_sandbox("sudo apt-get update", SandboxCommandMode::BwrapPackages);
        assert!(!out.contains("sudo"));
        assert!(out.contains("APT_CONFIG"));
        assert!(out.contains("apt-get update"));
    }

    #[test]
    fn adapt_leaves_unrelated_commands() {
        let out =
            adapt_command_for_sandbox("curl -fsS example.com", SandboxCommandMode::BwrapPackages);
        assert_eq!(out, "curl -fsS example.com");
    }

    #[test]
    fn adapt_docker_injects_tmp_apt_config() {
        let out = adapt_command_for_sandbox("apt-get install -y jq", SandboxCommandMode::Docker);
        assert!(out.contains("/tmp/bobaclaw-apt/apt.conf"));
        assert!(out.contains("APT::Sandbox::User"));
        assert!(out.contains("Dir::Cache"));
        assert!(out.contains("apt-get install"));
    }
}
