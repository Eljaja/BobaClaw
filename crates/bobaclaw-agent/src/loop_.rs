use bobaclaw_core::{BobaConfig, BobaPaths, CommandCapsuleManifest, NormalizedRequest};
use bobaclaw_executor::{BwrapExecutor, ExecutorProfile};
use bobaclaw_provider::{ChatMessage, OpenAiCompatProvider};
use bobaclaw_skills::SkillRegistry;
use bobaclaw_state::{RunLedger, SessionStore, StateDb};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct AgentResponse {
    pub text: String,
    pub session_id: String,
    pub run_id: Option<String>,
    pub executed: bool,
}

pub struct AgentLoop {
    paths: BobaPaths,
    config: BobaConfig,
    state: StateDb,
    skills: SkillRegistry,
}

impl AgentLoop {
    pub async fn new(paths: BobaPaths, config: BobaConfig) -> anyhow::Result<Self> {
        let state = StateDb::open(&paths.state_db).await?;
        let group = &config.default_agent_group;
        let skills = SkillRegistry::load(&paths.group_workspace(group))?;
        Ok(Self {
            paths,
            config,
            state,
            skills,
        })
    }

    pub async fn handle(&self, req: NormalizedRequest) -> anyhow::Result<AgentResponse> {
        let pool = self.state.pool();
        let sessions = SessionStore::new(pool);
        let session_id = sessions
            .get_or_create_cli(&req.agent_group)
            .await?;
        sessions
            .append_message(&session_id, "user", &req.user_text)
            .await?;

        let history = sessions.recent_messages(&session_id, 20).await?;
        let mut messages: Vec<ChatMessage> = vec![ChatMessage {
            role: "system".into(),
            content: system_prompt(&self.skills),
        }];
        for (role, content) in history {
            messages.push(ChatMessage { role, content });
        }

        let api_key = self.config.resolve_api_key()?;
        let provider = OpenAiCompatProvider::new(&self.config.provider, api_key);

        if let Some(skill) = self.skills.match_request(&req.user_text) {
            let hint = format!(
                "Matched skill `{}`. Follow its procedure if the user request fits.\n\n{}",
                skill.name, skill.body
            );
            messages[0].content.push_str(&format!("\n\n{hint}"));
        }

        if should_run_capsule(&req.user_text) {
            let run_id = format!("run_{}", Uuid::new_v4());
            let run_dir = self.paths.run_dir(&run_id);
            let ledger = RunLedger::new(pool);
            let profile = ExecutorProfile::bwrap_default();
            ledger
                .create_run(
                    &run_id,
                    Some(&session_id),
                    Some(&req.request_id.to_string()),
                    profile.id(),
                )
                .await?;

            let script = build_capsule_script(&req.user_text);
            let manifest = CommandCapsuleManifest {
                language: "bash".into(),
                argv: vec!["/work/script.sh".into()],
                executor_profile: profile.id().into(),
                timeout_secs: 120,
                network: false,
            };

            ledger.set_capsule_dir(&run_id, &run_dir.display().to_string()).await?;
            ledger.mark_started(&run_id).await?;

            let result = BwrapExecutor::execute(&profile, &run_dir, &script, &manifest);
            let (text, executed, exit_code) = match result {
                Ok(exec) => {
                    ledger
                        .mark_completed(&run_id, exec.exit_code, &exec.summary)
                        .await?;
                    let msg = format!(
                        "Execution finished (exit {}).\n\n{}\n\nArtifacts: {}",
                        exec.exit_code,
                        exec.summary,
                        run_dir.display()
                    );
                    (msg, true, exec.exit_code)
                }
                Err(e) => {
                    ledger.mark_denied(&run_id, &e.to_string()).await?;
                    (
                        format!("Execution denied or failed: {e}"),
                        false,
                        1,
                    )
                }
            };

            let assistant = if exit_code == 0 {
                format!("{text}\n\nOperational summary recorded in Run Ledger `{run_id}`.")
            } else {
                text
            };
            sessions
                .append_message(&session_id, "assistant", &assistant)
                .await?;
            return Ok(AgentResponse {
                text: assistant.clone(),
                session_id,
                run_id: Some(run_id),
                executed,
            });
        }

        let reply = provider
            .chat_completion(messages, req.model_override.as_deref())
            .await?;
        sessions
            .append_message(&session_id, "assistant", &reply)
            .await?;

        Ok(AgentResponse {
            text: reply,
            session_id,
            run_id: None,
            executed: false,
        })
    }
}

fn system_prompt(skills: &SkillRegistry) -> String {
    let mut s = String::from(
        "You are BobaClaw, a flexible ChatOps execution agent.\n\
         Prefer existing skills when they fit. Never claim execution succeeded unless the executor confirms it.\n\
         Never expose secrets.\n",
    );
    if !skills.names().is_empty() {
        s.push_str("\nAvailable skills: ");
        s.push_str(&skills.names().join(", "));
        s.push('\n');
    }
    s
}

fn should_run_capsule(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("run:")
        || lower.contains("execute:")
        || lower.starts_with("! ")
        || lower.contains("bash:")
}

fn build_capsule_script(user_text: &str) -> String {
    let cmd = user_text
        .strip_prefix("run:")
        .or_else(|| user_text.strip_prefix("execute:"))
        .or_else(|| user_text.strip_prefix("! "))
        .or_else(|| user_text.strip_prefix("bash:"))
        .unwrap_or(user_text)
        .trim();
    format!(
        "#!/usr/bin/env bash\nset -euo pipefail\ncd \"$(dirname \"$0\")\"\n{cmd}\n"
    )
}
