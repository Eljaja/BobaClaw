use std::path::{Path, PathBuf};

/// Resolve a BobaClaw data path for `docker create -v` when the gateway runs in a container.
///
/// The gateway uses `BOBACLAW_HOME` (e.g. `/data`) inside its container, but sibling sandbox
/// containers are created via the host Docker socket. Bind mounts must reference host paths
/// (e.g. `/opt/bobaclaw/data/workspace`), not in-container paths.
pub fn docker_bind_source(path: &Path) -> anyhow::Result<PathBuf> {
    let canonical = path.canonicalize()?;
    let Some(container_home) = std::env::var_os("BOBACLAW_HOME") else {
        return Ok(canonical);
    };
    let Some(host_home) = std::env::var_os("BOBACLAW_HOST_HOME") else {
        return Ok(canonical);
    };

    let container_home = PathBuf::from(container_home);
    let host_home = PathBuf::from(host_home);
    let container_home = container_home.canonicalize().unwrap_or(container_home);
    let host_home = host_home.canonicalize().unwrap_or(host_home);

    if let Ok(rel) = canonical.strip_prefix(&container_home) {
        return Ok(host_home.join(rel));
    }

    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};
    use tempfile::TempDir;

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var_os(key);
            // SAFETY: tests hold env_lock() so env mutations do not race.
            unsafe { std::env::set_var(key, value) };
            Self { key, previous }
        }

        fn clear(key: &'static str) -> Self {
            let previous = std::env::var_os(key);
            unsafe { std::env::remove_var(key) };
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(v) => unsafe { std::env::set_var(self.key, v) },
                None => unsafe { std::env::remove_var(self.key) },
            }
        }
    }

    #[test]
    fn docker_bind_source_without_host_home_uses_canonical() {
        let _lock = env_lock().lock().unwrap();
        let _host = EnvVarGuard::clear("BOBACLAW_HOST_HOME");

        let dir = TempDir::new().unwrap();
        let got = docker_bind_source(dir.path()).unwrap();
        assert_eq!(got, dir.path().canonicalize().unwrap());
    }

    #[test]
    fn docker_bind_source_remaps_container_home_to_host_home() {
        let _lock = env_lock().lock().unwrap();
        let container = TempDir::new().unwrap();
        let host = TempDir::new().unwrap();
        let workspace = container.path().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();

        let _home = EnvVarGuard::set("BOBACLAW_HOME", container.path().to_str().unwrap());
        let _host_home = EnvVarGuard::set("BOBACLAW_HOST_HOME", host.path().to_str().unwrap());

        let got = docker_bind_source(&workspace).unwrap();
        assert_eq!(got, host.path().join("workspace"));
    }
}
