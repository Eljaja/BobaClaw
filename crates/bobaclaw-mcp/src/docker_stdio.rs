use std::process::Command as StdCommand;

use bobaclaw_core::McpServerConfig;
use tokio::process::Command;

const MCP_LABEL: &str = "bobaclaw.mcp=1";

/// Docker container name for a managed stdio MCP server.
pub fn mcp_container_name(server_name: &str) -> String {
    format!("bobaclaw-mcp-{server_name}")
}

/// Remove MCP containers left behind when a BobaClaw process died without dropping McpHub.
pub fn cleanup_stale_mcp_containers() {
    let Ok(out) = StdCommand::new("docker")
        .args(["ps", "-aq", "--filter", &format!("label={MCP_LABEL}")])
        .output()
    else {
        return;
    };
    let ids = String::from_utf8_lossy(&out.stdout);
    for id in ids.split_whitespace() {
        let Ok(inspect) = StdCommand::new("docker")
            .args([
                "inspect",
                "-f",
                "{{ index .Config.Labels \"bobaclaw.mcp.pid\" }}",
                id,
            ])
            .output()
        else {
            continue;
        };
        let owner = String::from_utf8_lossy(&inspect.stdout).trim().to_string();
        let stale = owner
            .parse::<u32>()
            .map(|p| !process_alive(p))
            .unwrap_or(true);
        if stale {
            let _ = run_docker_sync(&["rm", "-f", id]);
        }
    }

    // Legacy anonymous Obscura MCP containers (random Docker names, no label).
    let Ok(legacy) = StdCommand::new("docker")
        .args(["ps", "-aq", "--filter", "ancestor=h4ckf0r0day/obscura"])
        .output()
    else {
        return;
    };
    let legacy_ids = String::from_utf8_lossy(&legacy.stdout);
    for id in legacy_ids.split_whitespace() {
        let _ = run_docker_sync(&["rm", "-f", id]);
    }
}

/// Stop a managed MCP container (and everything running inside, e.g. `/obscura mcp`).
pub fn stop_mcp_container(name: &str) {
    let _ = run_docker_sync(&["rm", "-f", name]);
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DockerRunSpec {
    platform: Option<String>,
    image: String,
    entrypoint_args: Vec<String>,
}

/// If this MCP entry spawns `docker run …`, replace it with a managed named container
/// (`docker create` once, then `docker start -ai`) so BobaClaw does not leave random
/// `kind_edison` containers behind.
pub struct PreparedDockerMcp {
    pub command: Command,
    pub container_name: String,
}

pub fn prepare_stdio_command(
    server_name: &str,
    cfg: &McpServerConfig,
) -> anyhow::Result<Option<PreparedDockerMcp>> {
    if cfg.command.trim() != "docker" {
        return Ok(None);
    }
    let spec = match parse_docker_run(&cfg.args)? {
        Some(s) => s,
        None => return Ok(None),
    };

    let container_name = mcp_container_name(server_name);
    prepare_exclusive_container(&container_name, &spec)?;

    let mut run_args = vec![
        "run".to_string(),
        "--rm".to_string(),
        "--init".to_string(),
        "--name".to_string(),
        container_name.clone(),
        "-i".to_string(),
        "--label".to_string(),
        MCP_LABEL.to_string(),
        "--label".to_string(),
        format!("bobaclaw.mcp.pid={}", std::process::id()),
    ];
    if let Some(platform) = &spec.platform {
        run_args.push("--platform".into());
        run_args.push(platform.clone());
    }
    run_args.push(spec.image.clone());
    run_args.extend(spec.entrypoint_args.clone());

    let mut cmd = Command::new("docker");
    let arg_refs: Vec<&str> = run_args.iter().map(String::as_str).collect();
    cmd.args(arg_refs);
    Ok(Some(PreparedDockerMcp {
        command: cmd,
        container_name,
    }))
}

fn parse_docker_run(args: &[String]) -> anyhow::Result<Option<DockerRunSpec>> {
    if args.first().map(String::as_str) != Some("run") {
        return Ok(None);
    }

    let mut platform = None;
    let mut image = None;
    let mut entrypoint_args = Vec::new();
    let mut i = 1usize;
    while i < args.len() {
        match args[i].as_str() {
            "--platform" => {
                let p = args
                    .get(i + 1)
                    .ok_or_else(|| anyhow::anyhow!("docker run: --platform requires a value"))?;
                platform = Some(p.clone());
                i += 2;
            }
            flag if flag.starts_with('-') => {
                if flag == "-i" || flag == "--interactive" || flag == "--rm" {
                    i += 1;
                    continue;
                }
                if needs_value(flag) {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            value => {
                image = Some(value.to_string());
                entrypoint_args = args[i + 1..].to_vec();
                break;
            }
        }
    }

    let Some(image) = image else {
        return Ok(None);
    };
    if entrypoint_args.is_empty() {
        anyhow::bail!("docker run MCP: missing container command after image '{image}'");
    }

    Ok(Some(DockerRunSpec {
        platform,
        image,
        entrypoint_args,
    }))
}

fn needs_value(flag: &str) -> bool {
    matches!(
        flag,
        "--name"
            | "--platform"
            | "--network"
            | "-v"
            | "--volume"
            | "-e"
            | "--env"
            | "-w"
            | "--workdir"
            | "-u"
            | "--user"
            | "--label"
            | "-l"
            | "-p"
            | "--publish"
    )
}

fn prepare_exclusive_container(name: &str, _spec: &DockerRunSpec) -> anyhow::Result<()> {
    let pid = std::process::id();

    if container_exists(name)? {
        if container_running(name)? {
            match container_owner_pid(name)? {
                Some(owner) if owner == pid => {
                    run_docker_sync(&["rm", "-f", name])?;
                }
                Some(owner) if process_alive(owner) => {
                    anyhow::bail!(
                        "MCP Docker container '{name}' is already in use by BobaClaw pid {owner}. \
                         Stop that process first (gateway/chat/agent) or run: make stop-obscura-mcp"
                    );
                }
                _ => {
                    run_docker_sync(&["rm", "-f", name])?;
                }
            }
        } else {
            run_docker_sync(&["rm", "-f", name])?;
        }
    }
    Ok(())
}

fn container_exists(name: &str) -> anyhow::Result<bool> {
    let out = StdCommand::new("docker")
        .args(["inspect", "-f", "{{.Id}}", name])
        .output()?;
    Ok(out.status.success())
}

fn container_running(name: &str) -> anyhow::Result<bool> {
    let out = StdCommand::new("docker")
        .args(["inspect", "-f", "{{.State.Running}}", name])
        .output()?;
    if !out.status.success() {
        return Ok(false);
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim() == "true")
}

fn container_owner_pid(name: &str) -> anyhow::Result<Option<u32>> {
    let out = StdCommand::new("docker")
        .args([
            "inspect",
            "-f",
            "{{ index .Config.Labels \"bobaclaw.mcp.pid\" }}",
            name,
        ])
        .output()?;
    if !out.status.success() {
        return Ok(None);
    }
    let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if raw.is_empty() {
        return Ok(None);
    }
    Ok(raw.parse().ok())
}

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{pid}")).exists()
}

#[cfg(not(unix))]
fn process_alive(_pid: u32) -> bool {
    false
}

fn run_docker_sync(args: &[&str]) -> anyhow::Result<()> {
    let out = StdCommand::new("docker").args(args).output()?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    anyhow::bail!(
        "docker {} failed: {}",
        args.first().copied().unwrap_or(""),
        stderr.trim()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_obscura_run() {
        let args = vec![
            "run".into(),
            "--rm".into(),
            "-i".into(),
            "h4ckf0r0day/obscura".into(),
            "mcp".into(),
        ];
        let spec = parse_docker_run(&args).unwrap().unwrap();
        assert_eq!(spec.image, "h4ckf0r0day/obscura");
        assert_eq!(spec.entrypoint_args, vec!["mcp".to_string()]);
        assert_eq!(spec.platform, None);
    }

    #[test]
    fn parse_run_with_platform() {
        let args = vec![
            "run".into(),
            "--rm".into(),
            "-i".into(),
            "--platform".into(),
            "linux/amd64".into(),
            "h4ckf0r0day/obscura".into(),
            "mcp".into(),
            "--stealth".into(),
        ];
        let spec = parse_docker_run(&args).unwrap().unwrap();
        assert_eq!(spec.platform.as_deref(), Some("linux/amd64"));
        assert_eq!(spec.entrypoint_args, vec!["mcp", "--stealth"]);
    }

    #[test]
    fn ignores_non_run_docker_args() {
        let args = vec!["start".into(), "-ai".into(), "bobaclaw-mcp-obscura".into()];
        assert!(parse_docker_run(&args).unwrap().is_none());
    }
}
