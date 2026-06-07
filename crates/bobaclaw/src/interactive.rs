use std::sync::Arc;

use bobaclaw_agent::{force_compact_session, AgentDispatcher};
use bobaclaw_core::{BobaConfig, BobaPaths, NormalizedRequest};
use bobaclaw_scheduler::spawn_embedded_scheduler;

use crate::chat_ui::ChatUi;
use bobaclaw_skills::SkillRegistry;
use bobaclaw_state::{SessionStore, StateDb};
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;

pub async fn run_chat(
    paths: BobaPaths,
    config: BobaConfig,
    group: Option<String>,
) -> anyhow::Result<()> {
    let agent_group = group.unwrap_or_else(|| config.default_agent_group.clone());

    let state = StateDb::open(&paths.state_db).await?;
    let dispatcher = match config.resolve_api_key() {
        Ok(_) => {
            let shared = Arc::new(AgentDispatcher::new(paths.clone(), config.clone()).await?);
            spawn_embedded_scheduler(paths.clone(), config.clone(), Some(shared.clone()));
            Some(shared)
        }
        Err(e) => {
            eprintln!("LLM: {e}");
            eprintln!(
                "Можно пользоваться /help, /doctor; для чата с моделью: export {}=...",
                config.provider.api_key_env
            );
            None
        }
    };
    let sessions = SessionStore::new(state.pool());
    let _session_id = sessions.get_or_create_cli(&agent_group).await?;

    let history_path = paths.home.join("chat-history.txt");
    let mut rl = DefaultEditor::new()?;
    let _ = rl.load_history(&history_path);
    let ui = ChatUi::new();

    loop {
        drain_cli_outbox(&paths);
        let line = match rl.readline("bobaclaw> ") {
            Ok(l) => l,
            Err(ReadlineError::Interrupted) => {
                println!("\n(очищено — Ctrl+C во время работы агента прерывает запрос; /quit для выхода)");
                continue;
            }
            Err(ReadlineError::Eof) => {
                println!("\nПока.");
                break;
            }
            Err(e) => {
                println!("\nОшибка ввода: {e:#} (попробуйте ещё раз или /quit)\n");
                continue;
            }
        };

        let line = line.trim().trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let _ = rl.add_history_entry(line);

        if let Some(reply) = match handle_slash(
            line,
            &paths,
            &config,
            &agent_group,
            &state,
            dispatcher.as_deref(),
        )
        .await
        {
            Ok(v) => v,
            Err(e) => {
                println!("\nОшибка команды: {e:#}\n");
                continue;
            }
        } {
            if reply == "__QUIT__" {
                println!("Пока.");
                break;
            }
            println!("{reply}");
            continue;
        }

        if line.starts_with('/') {
            println!("Неизвестная команда. /help — список.");
            continue;
        }

        let Some(dispatcher) = &dispatcher else {
            println!("Нужен API key для запросов к модели. /doctor — проверка.");
            continue;
        };

        let req = NormalizedRequest::cli(line, &agent_group);
        if let Err(e) = ui.run_dispatcher_turn(dispatcher, req).await {
            println!("\n\x1b[31mОшибка агента:\x1b[0m {e:#}\n");
        }
    }

    let _ = rl.save_history(&history_path);
    Ok(())
}

async fn handle_slash(
    line: &str,
    paths: &BobaPaths,
    config: &BobaConfig,
    agent_group: &str,
    state: &StateDb,
    dispatcher: Option<&AgentDispatcher>,
) -> anyhow::Result<Option<String>> {
    let pool = state.pool();
    let parts: Vec<&str> = line.split_whitespace().collect();
    let cmd = parts.first().copied().unwrap_or("");

    match cmd {
        "/quit" | "/exit" | "/q" => return Ok(Some("__QUIT__".into())),
        "/help" | "/h" | "/?" => Ok(Some(help_text())),
        "/new" | "/clear" => {
            let n = SessionStore::new(pool)
                .end_active_cli_sessions(agent_group)
                .await?;
            let id = SessionStore::new(pool)
                .get_or_create_cli(agent_group)
                .await?;
            Ok(Some(format!("Новая сессия ({n} закрыто). session={id}")))
        }
        "/session" => {
            let id = SessionStore::new(pool)
                .get_or_create_cli(agent_group)
                .await?;
            Ok(Some(format!("session={id}")))
        }
        "/skills" => {
            let ws = paths.group_workspace(agent_group);
            let listings = SkillRegistry::list_all(&ws)?;
            if listings.is_empty() {
                return Ok(Some("Нет skills в workspace.".into()));
            }
            let mut lines = Vec::new();
            for item in listings {
                let status = if item.entry.enabled { "on" } else { "off" };
                lines.push(format!(
                    "  [{status}] {} — {}",
                    item.entry.name, item.entry.description
                ));
            }
            Ok(Some(lines.join("\n")))
        }
        "/stop" => {
            let Some(dispatcher) = dispatcher else {
                return Ok(Some("Нет активного агента (нужен API key).".into()));
            };
            let scope = format!("cli:{agent_group}");
            if dispatcher.interrupt_scope(&scope).await {
                Ok(Some("⚡ Прерываю текущий запрос…".into()))
            } else {
                Ok(Some("Нет активного запроса.".into()))
            }
        }
        "/compact" => {
            let id = SessionStore::new(pool)
                .get_or_create_cli(agent_group)
                .await?;
            force_compact_session(pool, config, &id, None).await?;
            Ok(Some(
                "Контекст сжат: в историю добавлено compaction-сообщение (как Hermes/OpenClaw)."
                    .into(),
            ))
        }
        "/doctor" => {
            let mut out = String::from("doctor (кратко):\n");
            match config.resolve_api_key() {
                Ok(_) => out.push_str(&format!("  api key: OK\n")),
                Err(e) => out.push_str(&format!("  api key: {e}\n")),
            }
            let b = bobaclaw_executor::check_bwrap();
            out.push_str(&format!(
                "  bwrap: found={} user_ns={}\n",
                b.bwrap_found, b.user_ns_ok
            ));
            let backend = match config.executor.backend {
                bobaclaw_core::ExecutorBackend::Bubblewrap => "bubblewrap",
                bobaclaw_core::ExecutorBackend::Docker => "docker",
            };
            out.push_str(&format!(
                "  executor: backend={backend} network={} sandbox_packages={}\n",
                config.executor.network, config.executor.sandbox_packages
            ));
            if config.executor.backend == bobaclaw_core::ExecutorBackend::Docker {
                let d = bobaclaw_executor::check_docker_sandbox(&paths.home, &config.executor);
                out.push_str(&format!(
                    "  docker: daemon={} container_running={}\n",
                    d.daemon_ok, d.container_running
                ));
            }
            out.push_str(&format!(
                "  context: window={} reserve={} keep_recent={} compression={}\n",
                config.context.context_window_tokens,
                config.context.reserve_tokens,
                config.context.keep_recent_messages,
                config.context.compression_enabled
            ));
            Ok(Some(out))
        }
        _ => Ok(None),
    }
}

/// Deliver scheduled CLI messages written by the background scheduler.
fn drain_cli_outbox(paths: &BobaPaths) {
    let dir = paths.home.join("outbox");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return;
    };
    let mut files: Vec<_> = entries.flatten().filter(|e| e.path().is_file()).collect();
    files.sort_by_key(|e| e.file_name());
    for entry in files {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with("due_") {
            continue;
        }
        let Ok(body) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        println!("\n\x1b[36m⏰ Запланированное сообщение\x1b[0m\n{body}\n");
        let _ = std::fs::remove_file(entry.path());
    }
}

fn help_text() -> String {
    r#"Служебные команды (не для модели):
  /help, /?        справка
  /quit, /exit, /q выход
  /new, /clear     новая сессия
  /session         id сессии
  /compact         LLM-сжатие истории (Hermes/OpenClaw)
  /stop            прервать текущий запрос (как Hermes Ctrl+C)
  /skills          skills в workspace
  /doctor          проверка окружения

Отложенные задачи: tool schedule; список: bobaclaw schedule list
  Планировщик: bobaclaw scheduler start (daemon, отдельный терминал)"#
        .to_string()
}
