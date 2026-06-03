use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct BobaPaths {
    pub home: PathBuf,
    pub config: PathBuf,
    pub state_db: PathBuf,
    pub runs: PathBuf,
    pub workspace: PathBuf,
}

impl BobaPaths {
    pub fn resolve() -> anyhow::Result<Self> {
        let home = match std::env::var("BOBACLAW_HOME") {
            Ok(p) => PathBuf::from(p),
            Err(_) => dirs::home_dir()
                .map(|h| h.join(".bobaclaw"))
                .ok_or_else(|| anyhow::anyhow!("cannot resolve home directory"))?,
        };
        Ok(Self::from_home(home))
    }

    pub fn from_home(home: PathBuf) -> Self {
        Self {
            config: home.join("config.yaml"),
            state_db: home.join("state.db"),
            runs: home.join("runs"),
            workspace: home.join("workspace"),
            home,
        }
    }

    pub fn run_dir(&self, run_id: &str) -> PathBuf {
        self.runs.join(run_id)
    }

    pub fn group_workspace(&self, group: &str) -> PathBuf {
        self.workspace.join(group)
    }

    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.home)?;
        std::fs::create_dir_all(&self.runs)?;
        std::fs::create_dir_all(&self.workspace)?;
        Ok(())
    }
}

impl AsRef<Path> for BobaPaths {
    fn as_ref(&self) -> &Path {
        &self.home
    }
}
