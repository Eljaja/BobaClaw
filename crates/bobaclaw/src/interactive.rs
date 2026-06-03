use bobaclaw_agent::AgentLoop;
use bobaclaw_core::{BobaConfig, BobaPaths, NormalizedRequest};
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
    let agent = match config.resolve_api_key() {
        Ok(_) => Some(AgentLoop::new(paths.clone(), config.clone()).await?),
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
    let session_id = sessions.get_or_create_cli(&agent_group).await?;

    let skills = SkillRegistry::load(&paths.group_workspace(&agent_group))?;
    print_banner(&config, &agent_group, &session_id, &skills);

    let history_path = paths.home.join("chat-history.txt");
    let mut rl = DefaultEditor::new()?;
    let _ = rl.load_history(&history_path);

    loop {
        let line = match rl.readline("bobaclaw> ") {
            Ok(l) => l,
            Err(ReadlineError::Interrupted) => {
                println!("\n(прервано — /quit для выхода)");
                continue;
            }
            Err(ReadlineError::Eof) => {
                println!("\nПока.");
                break;
            }
            Err(e) => return Err(e.into()),
        };

        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let _ = rl.add_history_entry(line);

        if let Some(reply) = handle_slash(line, &paths, &config, &agent_group, &state).await? {
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

        let Some(agent) = &agent else {
            println!("Нужен API key для запросов к модели. /doctor — проверка.");
            continue;
        };

        print!("… ");
        let _ = std::io::Write::flush(&mut std::io::stdout());
        let req = NormalizedRequest::cli(line, &agent_group);
        match agent.handle(req).await {
            Ok(resp) => {
                println!("\n{}\n", resp.text);
                if resp.executed {
                    if let Some(run_id) = &resp.run_id {
                        eprintln!("[run {run_id} · session {}]", resp.session_id);
                    }
                }
            }
            Err(e) => println!("\nОшибка: {e}\n"),
        }
    }

    let _ = rl.save_history(&history_path);
    Ok(())
}

fn print_banner(config: &BobaConfig, group: &str, session_id: &str, skills: &SkillRegistry) {
    println!("BobaClaw interactive · group={group} · model={}", config.provider.model);
    println!("session={session_id}");
    if !skills.names().is_empty() {
        println!("skills: {}", skills.names().join(", "));
    }
    println!("Выполнение: run: <cmd>  |  /help  |  /quit\n");
}

async fn handle_slash(
    line: &str,
    paths: &BobaPaths,
    config: &BobaConfig,
    agent_group: &str,
    state: &StateDb,
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
            let id = SessionStore::new(pool).get_or_create_cli(agent_group).await?;
            Ok(Some(format!(
                "Новая сессия ({n} закрыто). session={id}"
            )))
        }
        "/session" => {
            let id = SessionStore::new(pool).get_or_create_cli(agent_group).await?;
            Ok(Some(format!("session={id}")))
        }
        "/skills" => {
            let reg = SkillRegistry::load(&paths.group_workspace(agent_group))?;
            if reg.list().is_empty() {
                return Ok(Some("Нет skills в workspace.".into()));
            }
            let mut lines = Vec::new();
            for s in reg.list() {
                lines.push(format!("  {} — {}", s.name, s.description));
            }
            Ok(Some(lines.join("\n")))
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
            Ok(Some(out))
        }
        _ => Ok(None),
    }
}

fn help_text() -> String {
    r#"Команды:
  /help, /?        эта справка
  /quit, /exit, /q выход
  /new, /clear     новая сессия (история в state.db)
  /session         показать id сессии
  /skills          список skills
  /doctor          быстрая проверка окружения

Сообщения агенту — обычный текст.
Sandbox: run: echo hello
         execute: ls -la
         ! pwd
         bash: whoami"#
        .to_string()
}
