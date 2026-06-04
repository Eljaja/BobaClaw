use std::sync::Arc;
use std::time::Duration;

use bobaclaw_agent::AgentLoop;
use bobaclaw_core::{BobaConfig, BobaPaths, IngressKind, NormalizedRequest};
use bobaclaw_state::{CronStore, ScheduledTask, ScheduledTaskStore, StateDb};
use chrono::Utc;
use cron::Schedule;
use std::str::FromStr;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

use crate::deliver::deliver_message;

/// Background task inside chat/gateway when `scheduler.embedded: true`.
pub fn spawn_embedded_scheduler(paths: BobaPaths, config: BobaConfig) {
    if !config.scheduler.enabled || !config.scheduler.embedded {
        return;
    }
    tokio::spawn(async move {
        if let Err(e) = run_scheduler_loop(paths, config).await {
            error!("embedded scheduler exited: {e}");
        }
    });
}

/// Foreground daemon (`bobaclaw scheduler start`). Holds until Ctrl+C.
pub async fn run_scheduler_daemon(paths: BobaPaths, config: BobaConfig) -> anyhow::Result<()> {
    if !config.scheduler.enabled {
        anyhow::bail!("scheduler.enabled is false in config.yaml");
    }

    let pid_path = paths.home.join("scheduler.pid");
    if pid_path.exists() {
        if let Ok(old) = std::fs::read_to_string(&pid_path) {
            if let Ok(pid) = old.trim().parse::<u32>() {
                if process_alive(pid) {
                    anyhow::bail!(
                        "scheduler already running (pid {pid}, {}). Stop it first.",
                        pid_path.display()
                    );
                }
            }
        }
        let _ = std::fs::remove_file(&pid_path);
    }

    std::fs::write(&pid_path, std::process::id().to_string())?;
    info!(
        "scheduler daemon pid={} pidfile={}",
        std::process::id(),
        pid_path.display()
    );

    let result = tokio::select! {
        res = run_scheduler_loop(paths, config) => res,
        _ = shutdown_signal() => {
            info!("scheduler daemon shutting down");
            Ok(())
        }
    };

    let _ = std::fs::remove_file(&pid_path);
    result
}

pub async fn run_scheduler_loop(paths: BobaPaths, config: BobaConfig) -> anyhow::Result<()> {
    if !config.scheduler.enabled {
        return Ok(());
    }
    let tick = Duration::from_secs(config.scheduler.tick_secs.max(5));
    sync_cron_from_config(&paths, &config).await?;
    info!(
        "scheduler running (tick={}s, embedded={}); one-shot + cron",
        tick.as_secs(),
        config.scheduler.embedded
    );
    loop {
        if let Err(e) = tick_once(&paths, &config).await {
            error!("scheduler tick: {e}");
        }
        tokio::time::sleep(tick).await;
    }
}

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

#[cfg(not(unix))]
fn process_alive(_pid: u32) -> bool {
    false
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut term =
            signal(SignalKind::terminate()).expect("install SIGTERM handler for scheduler");
        let mut int = signal(SignalKind::interrupt()).expect("install SIGINT handler for scheduler");
        tokio::select! {
            _ = term.recv() => {}
            _ = int.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

pub async fn sync_cron_from_config(paths: &BobaPaths, config: &BobaConfig) -> anyhow::Result<()> {
    let state = StateDb::open(&paths.state_db).await?;
    let store = CronStore::new(state.pool());
    for job in &config.cron.jobs {
        store
            .upsert(&job.id, &job.cron, &job.agent_group, &job.prompt)
            .await?;
    }
    Ok(())
}

async fn tick_once(paths: &BobaPaths, config: &BobaConfig) -> anyhow::Result<()> {
    let state = StateDb::open(&paths.state_db).await?;
    let pool = state.pool();
    let now = Utc::now().timestamp_millis() as f64 / 1000.0;

    run_due_scheduled(paths, config, pool, now).await?;
    run_due_cron(paths, config, pool).await?;
    Ok(())
}

async fn run_due_scheduled(
    paths: &BobaPaths,
    config: &BobaConfig,
    pool: &sqlx::SqlitePool,
    now: f64,
) -> anyhow::Result<()> {
    let store = ScheduledTaskStore::new(pool);
    let due = store.list_due(now).await?;
    if due.is_empty() {
        return Ok(());
    }

    let agent = AgentLoop::new(paths.clone(), config.clone()).await?;
    let agent = Arc::new(Mutex::new(agent));

    for task in due {
        if !store.mark_running(&task.id).await? {
            continue;
        }
        if let Err(e) = execute_scheduled_task(paths, config, &agent, pool, &task).await {
            let _ = store.mark_failed(&task.id, &e.to_string()).await;
            error!("scheduled task {} failed: {e}", task.id);
        }
    }
    Ok(())
}

async fn execute_scheduled_task(
    paths: &BobaPaths,
    config: &BobaConfig,
    agent: &Arc<Mutex<AgentLoop>>,
    pool: &sqlx::SqlitePool,
    task: &ScheduledTask,
) -> anyhow::Result<()> {
    let req = NormalizedRequest {
        request_id: uuid::Uuid::new_v4(),
        ingress: IngressKind::Cron,
        agent_group: task.agent_group.clone(),
        session_id: None,
        channel_peer: None,
        user_text: task.prompt.clone(),
        model_override: None,
    };

    let resp = {
        let a = agent.lock().await;
        a.handle(req).await?
    };

    let text = task
        .deliver_text
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(resp.text.as_str());

    let channel = task.deliver_channel.as_deref().unwrap_or("cli");
    let peer = task.deliver_peer.as_deref();
    deliver_message(config, &paths.home, channel, peer, text).await?;

    ScheduledTaskStore::new(pool).mark_done(&task.id).await?;
    info!("scheduled task {} delivered via {channel}", task.id);
    Ok(())
}

async fn run_due_cron(
    paths: &BobaPaths,
    config: &BobaConfig,
    pool: &sqlx::SqlitePool,
) -> anyhow::Result<()> {
    let store = CronStore::new(pool);
    let jobs = store.list_enabled().await?;
    let now = Utc::now();

    for job in jobs {
        let schedule = match Schedule::from_str(&job.cron_expr) {
            Ok(s) => s,
            Err(e) => {
                warn!("cron {} invalid expr: {e}", job.id);
                continue;
            }
        };

        let window = chrono::Duration::seconds(config.scheduler.tick_secs as i64 + 2);
        let window_start = now - window;
        let fire_in_window = schedule
            .after(&window_start)
            .take(4)
            .find(|t| *t <= now);

        let Some(fire_at) = fire_in_window else {
            continue;
        };

        let last = store.last_run_at(&job.id).await?;
        let fire_ts = fire_at.timestamp_millis() as f64 / 1000.0;
        if last.is_some_and(|l| l >= fire_ts - 1.0) {
            continue;
        }

        let _ = store.record_run(&job.id, "started").await?;
        info!("cron job {} firing", job.id);

        let cfg_job = config.cron.jobs.iter().find(|j| j.id == job.id);
        let deliver = cfg_job.and_then(|j| j.deliver.as_ref());

        let agent = AgentLoop::new(paths.clone(), config.clone()).await?;
        let req = NormalizedRequest {
            request_id: uuid::Uuid::new_v4(),
            ingress: IngressKind::Cron,
            agent_group: job.agent_group.clone(),
            session_id: None,
            channel_peer: None,
            user_text: job.prompt.clone(),
            model_override: None,
        };
        match agent.handle(req).await {
            Ok(resp) => {
                if let Some(d) = deliver {
                    if let Err(e) = deliver_message(
                        config,
                        &paths.home,
                        &d.channel,
                        Some(&d.peer),
                        &resp.text,
                    )
                    .await
                    {
                        warn!("cron {} deliver: {e}", job.id);
                    }
                }
                let _ = store.record_run(&job.id, "done").await;
            }
            Err(e) => {
                let _ = store.record_run(&job.id, "failed").await;
                warn!("cron {} agent: {e}", job.id);
            }
        }
    }
    Ok(())
}
