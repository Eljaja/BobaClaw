use std::io::{self, IsTerminal, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use bobaclaw_agent::{
    format_step_block, ActivityLog, AgentDispatcher, AgentEvent, AgentLoop, AgentResponse,
};
use bobaclaw_core::NormalizedRequest;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::terminal_md::render_markdown_lines;

const FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub struct ChatUi {
    color: bool,
}

impl ChatUi {
    pub fn new() -> Self {
        let color = std::env::var("NO_COLOR").is_err() && io::stdout().is_terminal();
        Self { color }
    }

    pub async fn run_dispatcher_turn(
        &self,
        dispatcher: &AgentDispatcher,
        req: NormalizedRequest,
    ) -> anyhow::Result<AgentResponse> {
        let activity = Arc::new(ActivityLog::new());
        let status = Arc::new(Mutex::new(String::from("Starting…")));
        let done = Arc::new(AtomicBool::new(false));
        let spinner = self.spawn_spinner(status.clone(), done.clone());
        let ui = self;
        let activity_cb = activity.clone();
        let progress_cb = move |event: AgentEvent| {
            ui.on_progress(&status, &activity_cb, event);
        };

        let scope = req.dispatch_scope();
        let interrupt_dispatcher = dispatcher.clone();
        let interrupt_scope = scope.clone();
        let interrupt_done = done.clone();
        let interrupt_listener = tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            if !interrupt_done.load(Ordering::Relaxed) {
                interrupt_dispatcher.interrupt_scope(&interrupt_scope).await;
            }
        });

        let result = dispatcher
            .handle_with_progress(req, Some(&progress_cb))
            .await;

        done.store(true, Ordering::Relaxed);
        interrupt_listener.abort();
        let _ = spinner.await;
        clear_line();
        result.map(|resp| {
            self.print_response(&resp);
            resp
        })
    }

    pub async fn run_agent_turn(
        &self,
        agent: &AgentLoop,
        req: NormalizedRequest,
    ) -> anyhow::Result<AgentResponse> {
        let activity = Arc::new(ActivityLog::new());
        let status = Arc::new(Mutex::new(String::from("Starting…")));
        let done = Arc::new(AtomicBool::new(false));
        let spinner = self.spawn_spinner(status.clone(), done.clone());
        let ui = self;
        let activity_cb = activity.clone();
        let progress_cb = move |event: AgentEvent| {
            ui.on_progress(&status, &activity_cb, event);
        };

        let cancel = CancellationToken::new();
        let interrupt_cancel = cancel.clone();
        let interrupt_done = done.clone();
        let interrupt_listener = tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            if !interrupt_done.load(Ordering::Relaxed) {
                interrupt_cancel.cancel();
            }
        });

        let result = agent
            .handle_with_progress(req, Some(&progress_cb), cancel)
            .await;

        done.store(true, Ordering::Relaxed);
        interrupt_listener.abort();
        let _ = spinner.await;
        clear_line();
        result.map(|resp| {
            self.print_response(&resp);
            resp
        })
    }

    fn spawn_spinner(
        &self,
        status: Arc<Mutex<String>>,
        done: Arc<AtomicBool>,
    ) -> JoinHandle<()> {
        let color = self.color;
        tokio::spawn(async move {
            let mut frame = 0usize;
            let started = Instant::now();
            while !done.load(Ordering::Relaxed) {
                let msg = status.lock().map(|s| s.clone()).unwrap_or_default();
                let elapsed = started.elapsed().as_secs();
                let spin = FRAMES[frame % FRAMES.len()];
                frame = frame.wrapping_add(1);
                if color {
                    print!(
                        "\r\x1b[2K\x1b[36m{spin}\x1b[0m \x1b[2m{msg}\x1b[0m \x1b[90m({elapsed}s)\x1b[0m"
                    );
                } else {
                    print!("\r\x1b[2K{spin} {msg} ({elapsed}s)");
                }
                let _ = io::stdout().flush();
                tokio::time::sleep(Duration::from_millis(90)).await;
            }
        })
    }

    fn on_progress(
        &self,
        status: &Arc<Mutex<String>>,
        activity: &ActivityLog,
        event: AgentEvent,
    ) {
        activity.push_event(&event);
        if let Ok(mut s) = status.lock() {
            *s = activity.spinner_line();
        }

        let block = format_step_block(&event);
        if block.is_empty() {
            return;
        }

        if self.color {
            let accent = match &event {
                AgentEvent::LlmThinking { .. } => "\x1b[35m",
                AgentEvent::ToolStart { .. } => "\x1b[33m",
                AgentEvent::ToolEnd { exit_code, .. } if *exit_code == 0 => "\x1b[32m",
                AgentEvent::ToolEnd { .. } => "\x1b[31m",
                AgentEvent::Compacting { .. } => "\x1b[34m",
                AgentEvent::AssistantChunk { .. } => "\x1b[36m",
                AgentEvent::EmptyResponseRetry { .. } => "\x1b[35m",
                AgentEvent::Interrupted => "\x1b[33m",
            };
            for line in block.lines() {
                eprintln!("{accent}  │ {line}\x1b[0m");
            }
        } else {
            for line in block.lines() {
                eprintln!("  | {line}");
            }
        }
    }

    fn print_response(&self, resp: &AgentResponse) {
        let width = 58usize;
        println!();
        if self.color {
            println!(
                "\x1b[1;36m  ╭─ BobaClaw \x1b[0m\x1b[2m{}\x1b[0m",
                "─".repeat(width.saturating_sub(12))
            );
        } else {
            println!("  ┌─ BobaClaw {}", "─".repeat(width.saturating_sub(12)));
        }

        for line in render_markdown_lines(&resp.text, self.color) {
            if line.is_empty() {
                println!("  │");
            } else {
                println!("  │ {line}");
            }
        }

        if self.color {
            println!("\x1b[2m  ╰{}\x1b[0m", "─".repeat(width));
        } else {
            println!("  └{}", "─".repeat(width));
        }

        if resp.interrupted {
            if self.color {
                eprintln!("\x1b[2m  ⚡ прервано\x1b[0m");
            } else {
                eprintln!("  ⚡ прервано");
            }
        }

        if resp.executed {
            if let Some(run_id) = &resp.run_id {
                if self.color {
                    eprintln!(
                        "\x1b[2m  run {run_id} · session {}\x1b[0m",
                        resp.session_id
                    );
                } else {
                    eprintln!("  run {run_id} · session {}", resp.session_id);
                }
            }
        }
        println!();
    }
}

fn clear_line() {
    print!("\r\x1b[2K");
    let _ = io::stdout().flush();
}
