use bobaclaw_channel_telegram::{approve_pairing, list_pending_pairing, run_telegram_polling};
use bobaclaw_scheduler::{run_scheduler_daemon, spawn_embedded_scheduler};
use bobaclaw_core::{BobaConfig, BobaPaths, NormalizedRequest};
use bobaclaw_core::ExecutorBackend;
use bobaclaw_executor::{bwrap_apt_advisory, check_bwrap, check_docker, check_docker_sandbox};
use bobaclaw_gateway::serve;
use bobaclaw_skill_forge::SkillForge;
use bobaclaw_skills::{guard_skill_dir, SkillRegistry, TrustLevel};
use bobaclaw_state::StateDb;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

mod chat_ui;
mod interactive;
mod terminal_md;

#[derive(Parser)]
#[command(name = "bobaclaw", about = "BobaClaw ChatOps execution agent")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Create default config and workspace layout
    Init,
    /// Health and environment checks
    Doctor,
    /// Send a single message via CLI channel
    Agent {
        #[arg(short, long)]
        message: String,
        #[arg(long)]
        group: Option<String>,
    },
    /// Interactive chat REPL (readline, history, slash commands)
    Chat {
        #[arg(long)]
        group: Option<String>,
    },
    /// Start HTTP gateway (REST + OpenAI-compatible)
    Gateway {
        #[command(subcommand)]
        action: GatewayAction,
    },
    Skills {
        #[command(subcommand)]
        command: SkillsCommand,
    },
    /// External messaging channels
    Channel {
        #[command(subcommand)]
        command: ChannelCommand,
    },
    /// Approve DM pairing codes
    Pairing {
        #[command(subcommand)]
        command: PairingCommand,
    },
    /// List or cancel one-shot scheduled tasks
    Schedule {
        #[command(subcommand)]
        command: ScheduleCommand,
    },
    /// Background scheduler (cron + delayed tasks)
    Scheduler {
        #[command(subcommand)]
        action: SchedulerAction,
    },
}

#[derive(Subcommand)]
enum SchedulerAction {
    /// Run scheduler as a foreground daemon (Ctrl+C to stop)
    Start,
}

#[derive(Subcommand)]
enum ScheduleCommand {
    List,
    Cancel { id: String },
}

#[derive(Subcommand)]
enum ChannelCommand {
    /// Long-poll Telegram Bot API and run the agent per message
    Telegram {
        #[command(subcommand)]
        action: TelegramAction,
    },
}

#[derive(Subcommand)]
enum TelegramAction {
    Start,
}

#[derive(Subcommand)]
enum PairingCommand {
    List {
        #[arg(long, default_value = "telegram")]
        channel: String,
    },
    Approve {
        channel: String,
        code: String,
    },
}

#[derive(Subcommand)]
enum GatewayAction {
    Start,
}

#[derive(Subcommand)]
enum SkillsCommand {
    List,
    View { name: String },
    Guard { path: String },
    DraftFromRun { run_id: String },
    Promote { draft_id: String },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("bobaclaw=info".parse()?))
        .init();

    let cli = Cli::parse();
    let paths = BobaPaths::resolve()?;
    paths.ensure_dirs()?;
    let config = BobaConfig::load(&paths.config)?;

    match cli.command {
        Commands::Init => cmd_init(&paths, &config)?,
        Commands::Doctor => cmd_doctor(&paths, &config).await?,
        Commands::Agent { message, group } => {
            cmd_agent(&paths, &config, &message, group).await?
        }
        Commands::Chat { group } => {
            spawn_embedded_scheduler(paths.clone(), config.clone());
            interactive::run_chat(paths, config, group).await?
        }
        Commands::Gateway { action } => match action {
            GatewayAction::Start => serve(paths, config).await?,
        },
        Commands::Skills { command } => {
            cmd_skills(&paths, &config, command).await?
        }
        Commands::Channel { command } => cmd_channel(paths, config, command).await?,
        Commands::Pairing { command } => cmd_pairing(&paths, command).await?,
        Commands::Schedule { command } => cmd_schedule(&paths, command).await?,
        Commands::Scheduler { action } => match action {
            SchedulerAction::Start => run_scheduler_daemon(paths, config).await?,
        },
    }
    Ok(())
}

fn cmd_init(paths: &BobaPaths, config: &BobaConfig) -> anyhow::Result<()> {
    BobaConfig::save(&paths.config, config)?;
    let group = paths.group_workspace(&config.default_agent_group);
    std::fs::create_dir_all(group.join("skills"))?;
    std::fs::create_dir_all(group.join("skills-staging"))?;
    std::fs::create_dir_all(group.join("memory"))?;
    seed_workspace_example(&group)?;
    println!("Initialized BobaClaw at {}", paths.home.display());
    println!("Config: {}", paths.config.display());
    println!(
        "Set {} then run: bobaclaw chat   (or: bobaclaw agent --message \"hello\")",
        config.provider.api_key_env
    );
    Ok(())
}

fn seed_workspace_example(group: &std::path::Path) -> anyhow::Result<()> {
    let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let example = manifest_dir.join("../../workspace-examples/home");
    if !example.exists() {
        return Ok(());
    }
    copy_tree(&example, group)?;
    Ok(())
}

fn copy_tree(src: &std::path::Path, dst: &std::path::Path) -> anyhow::Result<()> {
    if !dst.exists() {
        std::fs::create_dir_all(dst)?;
    }
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let to = dst.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &to)?;
        } else if !to.exists() {
            std::fs::copy(entry.path(), to)?;
        }
    }
    Ok(())
}

async fn cmd_doctor(paths: &BobaPaths, config: &BobaConfig) -> anyhow::Result<()> {
    println!("BobaClaw doctor");
    println!("  home: {}", paths.home.display());
    println!("  config: {} ({})", paths.config.display(), paths.config.exists());
    println!("  state.db: {}", paths.state_db.display());

    match config.resolve_api_key() {
        Ok(_) if !config.provider.api_key.trim().is_empty() => {
            println!("  api key: OK (inline in config.yaml)");
        }
        Ok(_) => println!("  api key: OK ({})", config.provider.api_key_env),
        Err(e) => println!("  api key: MISSING — {e}"),
    }

    println!("  provider: {} model={}", config.provider.base_url, config.provider.model);
    println!(
        "  llm timeout: {}s",
        config.provider.request_timeout_secs
    );
    let backend = match config.executor.backend {
        ExecutorBackend::Bubblewrap => "bubblewrap",
        ExecutorBackend::Docker => "docker",
    };
    println!(
        "  executor: backend={backend} network={} sandbox_packages={}",
        config.executor.network, config.executor.sandbox_packages
    );

    let bwrap = check_bwrap();
    println!(
        "  bubblewrap: found={} user_ns={} — {}",
        bwrap.bwrap_found, bwrap.user_ns_ok, bwrap.message
    );
    if config.executor.backend == ExecutorBackend::Bubblewrap
        && config.executor.sandbox_packages
    {
        if let Some(note) = bwrap_apt_advisory(bwrap.user_ns_ok) {
            println!("  bwrap apt: WARNING — {note}");
        } else {
            println!("  bwrap apt: supported (APT::Sandbox::User=root, writable cache binds)");
        }
    }

    if config.executor.backend == ExecutorBackend::Docker {
        let docker = check_docker_sandbox(&paths.home, &config.executor);
        println!(
            "  docker: found={} daemon={} container_running={} — {}",
            docker.docker_found,
            docker.daemon_ok,
            docker.container_running,
            docker.message
        );
        println!(
            "  docker config: image={} container={}",
            config.executor.docker.image, config.executor.docker.container_name
        );
    } else {
        let docker = check_docker();
        println!(
            "  docker: found={} daemon={} — {}",
            docker.docker_found, docker.daemon_ok, docker.message
        );
    }

    let tg = &config.channels.telegram;
    let pid = paths.home.join("scheduler.pid");
    let daemon = if pid.exists() {
        format!("pidfile {}", pid.display())
    } else {
        "not running (start: bobaclaw scheduler start)".into()
    };
    println!(
        "  scheduler: enabled={} embedded={} tick={}s cron_jobs={} daemon: {daemon}",
        config.scheduler.enabled,
        config.scheduler.embedded,
        config.scheduler.tick_secs,
        config.cron.jobs.len(),
    );
    println!(
        "  telegram: enabled={} dm_policy={:?} group_policy={:?}",
        tg.enabled, tg.dm_policy, tg.group_policy
    );
    match tg.resolve_bot_token() {
        Ok(_) => println!("  telegram token: OK"),
        Err(e) => println!("  telegram token: {e}"),
    }
    match tg.resolve_proxy() {
        Some(url) => println!("  telegram proxy: {url}"),
        None => println!("  telegram proxy: (direct)"),
    }

    if config.mcp_servers.is_empty() {
        println!("  mcp: none configured (add mcp_servers in config.yaml)");
    } else {
        let hub = bobaclaw_mcp::McpHub::connect(&config.mcp_servers).await;
        for st in hub.statuses(&config.mcp_servers) {
            if st.connected {
                println!(
                    "  mcp {}: OK, {} tool(s)",
                    st.name, st.tool_count
                );
            } else {
                let err = st.error.unwrap_or_else(|| "connect failed".into());
                println!("  mcp {}: FAIL — {err}", st.name);
            }
        }
    }
    Ok(())
}

async fn cmd_channel(
    paths: BobaPaths,
    config: BobaConfig,
    command: ChannelCommand,
) -> anyhow::Result<()> {
    match command {
        ChannelCommand::Telegram { action } => match action {
            TelegramAction::Start => {
                if !config.channels.telegram.enabled {
                    anyhow::bail!("enable channels.telegram.enabled in config.yaml");
                }
                run_telegram_polling(paths, config).await?;
            }
        },
    }
    Ok(())
}

async fn cmd_schedule(paths: &BobaPaths, command: ScheduleCommand) -> anyhow::Result<()> {
    let state = StateDb::open(&paths.state_db).await?;
    let store = bobaclaw_state::ScheduledTaskStore::new(state.pool());
    match command {
        ScheduleCommand::List => {
            let rows = store.list_pending().await?;
            if rows.is_empty() {
                println!("no pending scheduled tasks");
            }
            for t in rows {
                println!(
                    "{} run_at={} group={} deliver={}/{} prompt={}",
                    t.id,
                    t.run_at,
                    t.agent_group,
                    t.deliver_channel.as_deref().unwrap_or("cli"),
                    t.deliver_peer.as_deref().unwrap_or("-"),
                    t.prompt.chars().take(60).collect::<String>()
                );
            }
        }
        ScheduleCommand::Cancel { id } => {
            if store.cancel(&id).await? {
                println!("cancelled {id}");
            } else {
                println!("not found or not pending: {id}");
            }
        }
    }
    Ok(())
}

async fn cmd_pairing(paths: &BobaPaths, command: PairingCommand) -> anyhow::Result<()> {
    match command {
        PairingCommand::List { channel } => {
            let rows = list_pending_pairing(paths, Some(&channel)).await?;
            if rows.is_empty() {
                println!("no pending pairing for {channel}");
            }
            for r in rows {
                println!(
                    "{} peer={} code={} name={}",
                    r.channel, r.peer, r.code, r.display_name
                );
            }
        }
        PairingCommand::Approve { channel, code } => {
            match approve_pairing(paths, &channel, &code).await? {
                Some(peer) => println!("approved {channel} peer={peer}"),
                None => println!("no pending request for code {code}"),
            }
        }
    }
    Ok(())
}

async fn cmd_agent(
    paths: &BobaPaths,
    config: &BobaConfig,
    message: &str,
    group: Option<String>,
) -> anyhow::Result<()> {
    let agent_group = group.unwrap_or_else(|| config.default_agent_group.clone());
    let req = NormalizedRequest::cli(message, &agent_group);
    let agent = bobaclaw_agent::AgentLoop::new(paths.clone(), config.clone()).await?;
    let resp = agent.handle(req).await?;
    let color = std::env::var("NO_COLOR").is_err()
        && std::io::IsTerminal::is_terminal(&std::io::stdout());
    for line in terminal_md::render_markdown_lines(&resp.text, color) {
        println!("{line}");
    }
    if let Some(run_id) = resp.run_id {
        eprintln!("run_id={run_id} session_id={}", resp.session_id);
    }
    Ok(())
}

async fn cmd_skills(
    paths: &BobaPaths,
    config: &BobaConfig,
    command: SkillsCommand,
) -> anyhow::Result<()> {
    let group = &config.default_agent_group;
    let ws = paths.group_workspace(group);
    match command {
        SkillsCommand::List => {
            let reg = SkillRegistry::load(&ws)?;
            for s in reg.list() {
                println!("{} — {}", s.name, s.description);
            }
        }
        SkillsCommand::View { name } => {
            let reg = SkillRegistry::load(&ws)?;
            let s = reg.get(&name).ok_or_else(|| anyhow::anyhow!("skill not found"))?;
            println!("{}", std::fs::read_to_string(&s.path)?);
        }
        SkillsCommand::Guard { path } => {
            let report = guard_skill_dir(std::path::Path::new(&path), TrustLevel::Community);
            println!("verdict: {:?}", report.verdict);
            for f in &report.findings {
                println!("  - {f}");
            }
        }
        SkillsCommand::DraftFromRun { run_id } => {
            let state = StateDb::open(&paths.state_db).await?;
            let forge = SkillForge::new(paths.clone(), group.clone());
            let draft = forge.draft_from_run(&state, &run_id).await?;
            println!("draft_id={draft}");
        }
        SkillsCommand::Promote { draft_id } => {
            let forge = SkillForge::new(paths.clone(), group.clone());
            let name = forge.promote_draft(&draft_id)?;
            println!("promoted skill: {name}");
        }
    }
    Ok(())
}
