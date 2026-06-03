use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BobaConfig {
    #[serde(default)]
    pub provider: ProviderConfig,
    #[serde(default)]
    pub gateway: GatewayConfig,
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
            default_agent_group: default_agent_group(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    #[serde(default = "default_base_url")]
    pub base_url: String,
    #[serde(default)]
    pub api_key_env: String,
    #[serde(default = "default_model")]
    pub model: String,
}

fn default_base_url() -> String {
    "https://api.openai.com/v1".into()
}

fn default_model() -> String {
    "gpt-4o-mini".into()
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            base_url: default_base_url(),
            api_key_env: "OPENAI_API_KEY".into(),
            model: default_model(),
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
        if self.provider.api_key_env.is_empty() {
            anyhow::bail!("provider.api_key_env is empty");
        }
        std::env::var(&self.provider.api_key_env).map_err(|_| {
            anyhow::anyhow!(
                "missing API key: set {} or run `bobaclaw init`",
                self.provider.api_key_env
            )
        })
    }
}
