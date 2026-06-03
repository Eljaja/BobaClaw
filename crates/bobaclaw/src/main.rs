use bobaclaw_core::{BobaConfig, BobaPaths, NormalizedRequest};
use bobaclaw_executor::check_bwrap;
use bobaclaw_gateway::serve;
use bobaclaw_skill_forge::SkillForge;
use bobaclaw_skills::{guard_skill_dir, SkillRegistry, TrustLevel};
use bobaclaw_state::StateDb;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

mod interactive;

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
        Commands::Doctor => cmd_doctor(&paths, &config)?,
        Commands::Agent { message, group } => {
            cmd_agent(&paths, &config, &message, group).await?
        }
        Commands::Chat { group } => interactive::run_chat(paths, config, group).await?,
        Commands::Gateway { action } => match action {
            GatewayAction::Start => serve(paths, config).await?,
        },
        Commands::Skills { command } => {
            cmd_skills(&paths, &config, command).await?
        }
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

fn cmd_doctor(paths: &BobaPaths, config: &BobaConfig) -> anyhow::Result<()> {
    println!("BobaClaw doctor");
    println!("  home: {}", paths.home.display());
    println!("  config: {} ({})", paths.config.display(), paths.config.exists());
    println!("  state.db: {}", paths.state_db.display());

    match config.resolve_api_key() {
        Ok(_) => println!("  api key: OK ({})", config.provider.api_key_env),
        Err(e) => println!("  api key: MISSING — {e}"),
    }

    println!("  provider: {} model={}", config.provider.base_url, config.provider.model);

    let bwrap = check_bwrap();
    println!(
        "  bubblewrap: found={} user_ns={} — {}",
        bwrap.bwrap_found, bwrap.user_ns_ok, bwrap.message
    );
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
    println!("{}", resp.text);
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
