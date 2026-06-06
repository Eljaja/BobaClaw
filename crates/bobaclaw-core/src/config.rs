use serde::{Deserialize, Serialize};

use crate::channels::{ChannelsConfig, RoutingConfig};
use crate::context_config::ContextConfig;
use crate::mcp::McpServers;
use crate::scheduler::{CronConfig, SchedulerConfig};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BobaConfig {
    #[serde(default)]
    pub provider: ProviderConfig,
    #[serde(default)]
    pub gateway: GatewayConfig,
    #[serde(default)]
    pub executor: ExecutorConfig,
    #[serde(default)]
    pub context: ContextConfig,
    #[serde(default)]
    pub channels: ChannelsConfig,
    #[serde(default)]
    pub routing: RoutingConfig,
    #[serde(default)]
    pub scheduler: SchedulerConfig,
    #[serde(default)]
    pub cron: CronConfig,
    #[serde(default, rename = "mcp_servers")]
    pub mcp_servers: McpServers,
    #[serde(default = "default_agent_group")]
    pub default_agent_group: String,
}

fn default_agent_group() -> String {
    "home".into()
}

impl Default for BobaConfig {
    fn default() -> Self {
        Self {
            provider: ProviderConfig::default(),
            gateway: GatewayConfig::default(),
            executor: ExecutorConfig::default(),
            context: ContextConfig::default(),
            channels: ChannelsConfig::default(),
            routing: RoutingConfig::default(),
            scheduler: SchedulerConfig::default(),
            cron: CronConfig::default(),
            mcp_servers: McpServers::default(),
            default_agent_group: default_agent_group(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    #[serde(default = "default_base_url")]
    pub base_url: String,
    /// Inline key (optional). Prefer env via `api_key_env` for anything non-throwaway.
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub api_key_env: String,
    #[serde(default = "default_model")]
    pub model: String,
    /// HTTP timeout for each LLM request (seconds).
    #[serde(default = "default_request_timeout_secs")]
    pub request_timeout_secs: u64,
}

fn default_base_url() -> String {
    "https://api.openai.com/v1".into()
}

fn default_model() -> String {
    "gpt-4o-mini".into()
}

fn default_request_timeout_secs() -> u64 {
    300
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            base_url: default_base_url(),
            api_key: String::new(),
            api_key_env: "OPENAI_API_KEY".into(),
            model: default_model(),
            request_timeout_secs: default_request_timeout_secs(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayConfig {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_port")]
    pub port: u16,
}

fn default_bind() -> String {
    "127.0.0.1".into()
}

fn default_port() -> u16 {
    18790
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ExecutorBackend {
    Bubblewrap,
    Docker,
}

fn default_executor_backend() -> ExecutorBackend {
    if cfg!(target_os = "macos") {
        ExecutorBackend::Docker
    } else {
        ExecutorBackend::Bubblewrap
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerExecutorConfig {
    #[serde(default = "default_docker_image")]
    pub image: String,
    #[serde(default = "default_docker_container_name")]
    pub container_name: String,
}

fn default_docker_image() -> String {
    "bobaclaw/sandbox:latest".into()
}

fn default_docker_container_name() -> String {
    "bobaclaw-sandbox".into()
}

impl Default for DockerExecutorConfig {
    fn default() -> Self {
        Self {
            image: default_docker_image(),
            container_name: default_docker_container_name(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutorConfig {
    /// Sandbox backend: bubblewrap (default) or a long-lived Docker container.
    #[serde(default = "default_executor_backend")]
    pub backend: ExecutorBackend,
    /// Network egress for `exec` (bwrap `--share-net` or Docker `--network bridge`).
    #[serde(default = "default_executor_network")]
    pub network: bool,
    /// Writable package paths under `workspace/.bobaclaw-sandbox/` (bubblewrap only).
    #[serde(default = "default_executor_sandbox_packages")]
    pub sandbox_packages: bool,
    #[serde(default)]
    pub docker: DockerExecutorConfig,
}

fn default_executor_network() -> bool {
    true
}

fn default_executor_sandbox_packages() -> bool {
    true
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            backend: default_executor_backend(),
            network: default_executor_network(),
            sandbox_packages: default_executor_sandbox_packages(),
            docker: DockerExecutorConfig::default(),
        }
    }
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            port: default_port(),
        }
    }
}

impl BobaConfig {
    pub fn load(path: &std::path::Path) -> anyhow::Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(path)?;
        Ok(serde_yaml::from_str(&raw)?)
    }

    pub fn save(path: &std::path::Path, cfg: &Self) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let raw = serde_yaml::to_string(cfg)?;
        std::fs::write(path, raw)?;
        Ok(())
    }

    pub fn resolve_api_key(&self) -> anyhow::Result<String> {
        let inline = self.provider.api_key.trim();
        if !inline.is_empty() {
            return Ok(inline.to_string());
        }
        if self.provider.api_key_env.is_empty() {
            anyhow::bail!("set provider.api_key or provider.api_key_env");
        }
        std::env::var(&self.provider.api_key_env).map_err(|_| {
            anyhow::anyhow!(
                "missing API key: set provider.api_key, env {}, or run `bobaclaw init`",
                self.provider.api_key_env
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_inline_api_key() {
        let mut cfg = BobaConfig::default();
        cfg.provider.api_key = "sk-test".into();
        assert_eq!(cfg.resolve_api_key().unwrap(), "sk-test");
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        let cfg = BobaConfig::default();
        BobaConfig::save(&path, &cfg).unwrap();
        let loaded = BobaConfig::load(&path).unwrap();
        assert_eq!(loaded.default_agent_group, cfg.default_agent_group);
    }

    #[test]
    fn load_missing_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nope.yaml");
        assert!(BobaConfig::load(&path).unwrap().context.compression_enabled);
    }

    #[test]
    fn load_executor_docker_backend() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.yaml");
        std::fs::write(
            &path,
            "executor:\n  backend: docker\n  network: false\n  docker:\n    image: alpine:3.20\n",
        )
        .unwrap();
        let loaded = BobaConfig::load(&path).unwrap();
        assert_eq!(loaded.executor.backend, ExecutorBackend::Docker);
        assert!(!loaded.executor.network);
        assert_eq!(loaded.executor.docker.image, "alpine:3.20");
        assert_eq!(
            loaded.executor.docker.container_name,
            "bobaclaw-sandbox"
        );
    }
}
