use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerConfig {
    #[serde(default = "default_scheduler_enabled")]
    pub enabled: bool,
    /// Run scheduler inside `bobaclaw chat` / `gateway` (legacy). Prefer daemon: `bobaclaw scheduler start`.
    #[serde(default)]
    pub embedded: bool,
    #[serde(default = "default_tick_secs")]
    pub tick_secs: u64,
}

fn default_scheduler_enabled() -> bool {
    true
}

fn default_tick_secs() -> u64 {
    15
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            enabled: default_scheduler_enabled(),
            embedded: false,
            tick_secs: default_tick_secs(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CronConfig {
    #[serde(default)]
    pub jobs: Vec<CronJobConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJobConfig {
    pub id: String,
    pub cron: String,
    pub agent_group: String,
    pub prompt: String,
    #[serde(default)]
    pub deliver: Option<DeliverTarget>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliverTarget {
    pub channel: String,
    pub peer: String,
}
