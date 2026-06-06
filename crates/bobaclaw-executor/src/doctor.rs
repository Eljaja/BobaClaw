use std::path::Path;
use std::process::Command;

use bobaclaw_core::ExecutorConfig;

#[derive(Debug, Clone)]
pub struct BwrapCheck {
    pub bwrap_found: bool,
    pub user_ns_ok: bool,
    pub message: String,
}

pub fn check_bwrap() -> BwrapCheck {
    let bwrap = which_bwrap();
    if bwrap.is_none() {
        return BwrapCheck {
            bwrap_found: false,
            user_ns_ok: false,
            message: "bubblewrap (bwrap) not found in PATH".into(),
        };
    }

    let probe = Command::new(bwrap.as_ref().unwrap())
        .args([
            "--unshare-user",
            "--uid",
            "65534",
            "--gid",
            "65534",
            "--ro-bind",
            "/",
            "/",
            "--dev",
            "/dev",
            "--",
            "/bin/true",
        ])
        .output();

    match probe {
        Ok(out) if out.status.success() => BwrapCheck {
            bwrap_found: true,
            user_ns_ok: true,
            message: "bubblewrap user namespace probe OK".into(),
        },
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            BwrapCheck {
                bwrap_found: true,
                user_ns_ok: false,
                message: format!("bwrap probe failed: {stderr}"),
            }
        }
        Err(e) => BwrapCheck {
            bwrap_found: true,
            user_ns_ok: false,
            message: format!("bwrap probe error: {e}"),
        },
    }
}

#[derive(Debug, Clone)]
pub struct DockerCheck {
    pub docker_found: bool,
    pub daemon_ok: bool,
    pub container_running: bool,
    pub message: String,
}

pub fn check_docker() -> DockerCheck {
    let docker_found = which_docker().is_some();
    if !docker_found {
        return DockerCheck {
            docker_found: false,
            daemon_ok: false,
            container_running: false,
            message: "docker CLI not found in PATH".into(),
        };
    }

    let probe = Command::new("docker")
        .args(["info", "--format", "{{.ServerVersion}}"])
        .output();

    match probe {
        Ok(out) if out.status.success() => DockerCheck {
            docker_found: true,
            daemon_ok: true,
            container_running: false,
            message: format!(
                "docker daemon OK ({})",
                String::from_utf8_lossy(&out.stdout).trim()
            ),
        },
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            DockerCheck {
                docker_found: true,
                daemon_ok: false,
                container_running: false,
                message: format!("docker daemon unreachable: {stderr}"),
            }
        }
        Err(e) => DockerCheck {
            docker_found: true,
            daemon_ok: false,
            container_running: false,
            message: format!("docker probe error: {e}"),
        },
    }
}

pub fn check_docker_sandbox(home: &Path, executor: &ExecutorConfig) -> DockerCheck {
    let mut check = check_docker();
    if !check.daemon_ok {
        return check;
    }

    let name = &executor.docker.container_name;
    let out = Command::new("docker")
        .args(["inspect", "-f", "{{.State.Running}}", name])
        .output();

    match out {
        Ok(o) if o.status.success() => {
            let running = String::from_utf8_lossy(&o.stdout).trim() == "true";
            check.container_running = running;
            let spec_path = home.join("sandbox-container.json");
            let spec_note = if spec_path.exists() {
                "spec on disk"
            } else {
                "not created yet (starts on first exec)"
            };
            check.message = if running {
                format!("container {name} running ({spec_note})")
            } else if container_exists_quick(name) {
                format!("container {name} stopped ({spec_note})")
            } else {
                format!("container {name} not created ({spec_note})")
            };
        }
        _ => {
            check.container_running = false;
            check.message = format!("container {name} not created (starts on first exec)");
        }
    }

    check
}

fn container_exists_quick(name: &str) -> bool {
    Command::new("docker")
        .args(["inspect", "-f", "{{.Id}}", name])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn which_docker() -> Option<String> {
    for path in ["/usr/bin/docker", "/usr/local/bin/docker"] {
        if Path::new(path).exists() {
            return Some(path.into());
        }
    }
    Command::new("which")
        .arg("docker")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            String::from_utf8(o.stdout)
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
}

fn which_bwrap() -> Option<String> {
    for path in ["/usr/bin/bwrap", "/bin/bwrap"] {
        if std::path::Path::new(path).exists() {
            return Some(path.into());
        }
    }
    Command::new("which")
        .arg("bwrap")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            String::from_utf8(o.stdout)
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
}
