use std::process::Command;

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
