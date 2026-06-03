use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Created,
    ScriptSaved,
    Approved,
    Started,
    Completed,
    Failed,
    Timeout,
    Denied,
    Partial,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunEventKind {
    Created,
    ScriptSaved,
    Approved,
    Started,
    Stdout,
    Stderr,
    Artifact,
    ResultJson,
    Completed,
    Failed,
    Timeout,
    Denied,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandCapsuleManifest {
    pub language: String,
    pub argv: Vec<String>,
    pub executor_profile: String,
    pub timeout_secs: u64,
    pub network: bool,
}
