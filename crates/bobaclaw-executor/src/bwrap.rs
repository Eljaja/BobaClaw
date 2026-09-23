use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use bobaclaw_core::CommandCapsuleManifest;

use crate::doctor::check_bwrap;
use crate::profile::{ExecutorProfile, ProfileKind};
use crate::run::{ExecutionResult, RunArtifacts};
use crate::sandbox::{append_sandbox_args, prepare_package_dirs};

pub struct BwrapExecutor;

impl BwrapExecutor {
    /// Run a shell command with the agent workspace mounted read-write at `/workspace`.
    ///
    /// The sandbox starts from a cleared environment (`--clearenv`); see [`SandboxEnv`].
    pub fn exec_command(
        profile: &ExecutorProfile,
        workspace: &Path,
        run_dir: &Path,
        command: &str,
        env: &SandboxEnv,
    ) -> anyhow::Result<ExecutionResult> {
        let check = check_bwrap();
        if !check.user_ns_ok {
            anyhow::bail!("bubblewrap unavailable: {}", check.message);
        }
        let bwrap = which_bwrap()?;

        let prepared = prepare_exec_command(&bwrap, profile, workspace, run_dir, command, env)?;
        let PreparedBwrap {
            mut cmd,
            artifacts,
            secrets,
        } = prepared;

        let output = cmd.output();
        // Remove the secret env file as soon as the sandbox exits.
        drop(secrets);
        let output = output?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let code = output.status.code().unwrap_or(1);
        artifacts.write_result(code, &stdout, &stderr)
    }

    pub fn execute(
        profile: &ExecutorProfile,
        run_dir: &Path,
        script: &str,
        manifest: &CommandCapsuleManifest,
    ) -> anyhow::Result<ExecutionResult> {
        let artifacts = RunArtifacts::prepare(run_dir, script, manifest)?;

        match profile.kind {
            ProfileKind::HostDanger => {
                anyhow::bail!(
                    "host-danger requires explicit approval; not implemented in this run path"
                )
            }
            ProfileKind::BwrapDefault | ProfileKind::BwrapNetworked | ProfileKind::Readonly => {
                Self::run_bwrap(profile, &artifacts)
            }
            ProfileKind::DockerDefault | ProfileKind::DockerNetworked => {
                anyhow::bail!("docker profiles require SandboxExecutor::exec_command")
            }
            ProfileKind::SystemdRun => Self::run_systemd_run(&artifacts),
        }
    }

    fn run_bwrap(
        profile: &ExecutorProfile,
        artifacts: &RunArtifacts,
    ) -> anyhow::Result<ExecutionResult> {
        let check = check_bwrap();
        if !check.user_ns_ok {
            anyhow::bail!("bubblewrap unavailable: {}", check.message);
        }

        let bwrap = which_bwrap()?;
        let work = artifacts.run_dir.canonicalize()?;
        let mut cmd = Command::new(&bwrap);
        append_base_ro_binds(&mut cmd);
        append_env_args(&mut cmd, "/work", &SandboxEnv::default(), None);
        cmd.args([
            "--bind",
            work.to_str().unwrap(),
            "/work",
            "--chdir",
            "/work",
            "--dev",
            "/dev",
        ]);
        append_sandbox_args(&mut cmd, profile, &work);
        cmd.args(["--", "/work/script.sh"]);

        let output = cmd.output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let code = output.status.code().unwrap_or(1);
        artifacts.write_result(code, &stdout, &stderr)
    }

    fn run_systemd_run(artifacts: &RunArtifacts) -> anyhow::Result<ExecutionResult> {
        let wd = artifacts.run_dir.display().to_string();
        // Transient units are spawned by the service manager and do not inherit our env;
        // env_clear() additionally keeps secrets out of the systemd-run client itself.
        let output = Command::new("systemd-run")
            .env_clear()
            .env("PATH", SANDBOX_PATH)
            .arg("--wait")
            .arg("--collect")
            .arg("--pipe")
            .arg(format!("--working-directory={wd}"))
            .arg(format!("{}/script.sh", artifacts.run_dir.display()))
            .output();

        match output {
            Ok(out) if out.status.success() || out.stderr.is_empty() => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                let code = out.status.code().unwrap_or(1);
                artifacts.write_result(code, &stdout, &stderr)
            }
            _ => Self::run_bwrap(&ExecutorProfile::bwrap_default(), artifacts),
        }
    }
}

/// Explicit environment for a sandboxed command. The host environment is never inherited.
#[derive(Debug, Clone, Default)]
pub struct SandboxEnv {
    /// Host env var names copied into the sandbox via `--setenv` (non-secret only:
    /// values are visible in the host process list).
    pub passthrough: Vec<String>,
    /// Secret `(NAME, value)` pairs for the child process only. Written to a `0600`
    /// env file outside the run capsule, bind-mounted read-only and sourced before the
    /// command; never placed in argv, `script.sh`, `capsule.yaml`, or logs.
    pub secrets: Vec<(String, String)>,
}

impl SandboxEnv {
    pub fn with_passthrough(passthrough: &[String]) -> Self {
        Self {
            passthrough: passthrough.to_vec(),
            secrets: Vec::new(),
        }
    }
}

/// `PATH` inside the sandbox (fixed; the host `PATH` is not inherited).
pub const SANDBOX_PATH: &str = "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin";

/// Guest path of the secret env file (see [`SandboxEnv::secrets`]).
pub const SECRET_ENV_GUEST_PATH: &str = "/run/bobaclaw/secrets.env";

/// Variables the sandbox always sets itself; `passthrough` cannot override them.
const RESERVED_ENV: &[&str] = &["PATH", "HOME", "LANG", "TERM", "TMPDIR", "APT_CONFIG"];

pub(crate) struct PreparedBwrap {
    pub cmd: Command,
    pub artifacts: RunArtifacts,
    pub secrets: Option<SecretEnvFile>,
}

/// Build the full `bwrap` invocation for [`BwrapExecutor::exec_command`] without running it.
/// Capsule artifacts (`script.sh`, `capsule.yaml`) are written before the run and always
/// contain the caller's command, never secret values.
pub(crate) fn prepare_exec_command(
    bwrap: &str,
    profile: &ExecutorProfile,
    workspace: &Path,
    run_dir: &Path,
    command: &str,
    env: &SandboxEnv,
) -> anyhow::Result<PreparedBwrap> {
    std::fs::create_dir_all(workspace)?;
    std::fs::create_dir_all(run_dir)?;
    if profile.allow_package_install {
        prepare_package_dirs(workspace)?;
    }
    let workspace = workspace.canonicalize()?;
    let run_dir = run_dir.canonicalize()?;

    let manifest = CommandCapsuleManifest {
        language: "bash".into(),
        argv: vec!["/bin/bash".into(), "-lc".into(), command.into()],
        executor_profile: profile.id().into(),
        timeout_secs: 120,
        network: profile.allow_network,
    };
    let artifacts = RunArtifacts::prepare(&run_dir, command, &manifest)?;

    let secrets = if env.secrets.is_empty() {
        None
    } else {
        Some(SecretEnvFile::write(&env.secrets)?)
    };

    let mut cmd = Command::new(bwrap);
    append_base_ro_binds(&mut cmd);
    append_env_args(&mut cmd, "/workspace", env, secrets.as_ref());
    cmd.args([
        "--bind",
        workspace.to_str().unwrap(),
        "/workspace",
        "--bind",
        run_dir.to_str().unwrap(),
        "/capsule",
        "--chdir",
        "/workspace",
        "--dev",
        "/dev",
    ]);
    append_sandbox_args(&mut cmd, profile, &workspace);

    let inner = match secrets {
        Some(_) => format!("set -a; . {SECRET_ENV_GUEST_PATH}; set +a; {command}"),
        None => command.to_string(),
    };
    cmd.args(["--", "/bin/bash", "-lc", inner.as_str()]);

    Ok(PreparedBwrap {
        cmd,
        artifacts,
        secrets,
    })
}

/// Clear the environment of both `bwrap` and the sandboxed child, then set a minimal
/// whitelist. Must run before any other `--setenv` (bwrap applies env flags in order).
pub(crate) fn append_env_args(
    cmd: &mut Command,
    home: &str,
    env: &SandboxEnv,
    secrets: Option<&SecretEnvFile>,
) {
    // bwrap itself never sees host secrets (e.g. via /proc/<pid>/environ).
    cmd.env_clear();
    cmd.env("PATH", SANDBOX_PATH);

    cmd.arg("--clearenv");
    cmd.args(["--setenv", "PATH", SANDBOX_PATH]);
    cmd.args(["--setenv", "HOME", home]);
    let lang = host_var_or("LANG", "C.UTF-8");
    cmd.args(["--setenv", "LANG", lang.as_str()]);
    let term = host_var_or("TERM", "dumb");
    cmd.args(["--setenv", "TERM", term.as_str()]);

    for name in &env.passthrough {
        let name = name.trim();
        if !is_valid_env_name(name) || RESERVED_ENV.contains(&name) {
            continue;
        }
        if let Ok(value) = std::env::var(name) {
            cmd.args(["--setenv", name, value.as_str()]);
        }
    }

    if let Some(file) = secrets {
        cmd.args([
            "--ro-bind",
            file.path().to_str().unwrap(),
            SECRET_ENV_GUEST_PATH,
        ]);
    }
}

fn host_var_or(name: &str, default: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|v| !v.is_empty() && !v.contains('\0'))
        .unwrap_or_else(|| default.to_string())
}

pub(crate) fn is_valid_env_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c == '_' || c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

/// Private `0600` env file in a `0700` temp dir (outside the run capsule); removed on drop.
pub(crate) struct SecretEnvFile {
    dir: PathBuf,
    file: PathBuf,
}

impl SecretEnvFile {
    pub(crate) fn write(secrets: &[(String, String)]) -> anyhow::Result<Self> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let dir = std::env::temp_dir().join(format!(
            "bobaclaw-secrets-{}-{}-{}",
            std::process::id(),
            nanos,
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(&dir)?;
        let guard = Self {
            file: dir.join("secrets.env"),
            dir,
        };

        let mut body = String::new();
        for (name, value) in secrets {
            if !is_valid_env_name(name) {
                anyhow::bail!("invalid secret env var name: {name:?}");
            }
            body.push_str(name);
            body.push('=');
            body.push_str(&shell_single_quote(value));
            body.push('\n');
        }

        let mut opts = std::fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut f = opts.open(&guard.file)?;
        f.write_all(body.as_bytes())?;
        Ok(guard)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.file
    }
}

impl Drop for SecretEnvFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.file);
        let _ = std::fs::remove_dir(&self.dir);
    }
}

fn shell_single_quote(s: &str) -> String {
    let mut out = String::from("'");
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

fn append_base_ro_binds(cmd: &mut Command) {
    cmd.args([
        "--unshare-all",
        "--die-with-parent",
        "--new-session",
        "--ro-bind",
        "/usr",
        "/usr",
        "--ro-bind",
        "/bin",
        "/bin",
        "--ro-bind",
        "/lib",
        "/lib",
    ]);
    if Path::new("/lib64").exists() {
        cmd.args(["--ro-bind", "/lib64", "/lib64"]);
    }
}

fn which_bwrap() -> anyhow::Result<String> {
    for path in ["/usr/bin/bwrap", "/bin/bwrap"] {
        if Path::new(path).exists() {
            return Ok(path.into());
        }
    }
    let out = Command::new("which").arg("bwrap").output()?;
    if out.status.success() {
        let s = String::from_utf8(out.stdout)?.trim().to_string();
        if !s.is_empty() {
            return Ok(s);
        }
    }
    anyhow::bail!("bwrap not found")
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAKE_KEY_VAR: &str = "BOBACLAW_TEST_BWRAP_FAKE_API_KEY";
    const FAKE_KEY_VALUE: &str = "sk-test-bwrap-must-not-leak-0123456789";

    fn args_of(cmd: &Command) -> Vec<String> {
        cmd.get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    fn envs_of(cmd: &Command) -> Vec<(String, Option<String>)> {
        cmd.get_envs()
            .map(|(k, v)| {
                (
                    k.to_string_lossy().into_owned(),
                    v.map(|v| v.to_string_lossy().into_owned()),
                )
            })
            .collect()
    }

    #[test]
    fn exec_command_clears_env_and_does_not_forward_parent_api_keys() {
        std::env::set_var(FAKE_KEY_VAR, FAKE_KEY_VALUE);
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("ws");
        let run = dir.path().join("run");
        let prepared = prepare_exec_command(
            "/usr/bin/bwrap",
            &ExecutorProfile::from_config(true, false),
            &ws,
            &run,
            "printenv",
            &SandboxEnv::default(),
        )
        .unwrap();
        let args = args_of(&prepared.cmd);

        // --clearenv comes before every --setenv (bwrap applies env flags in order).
        let clear = args.iter().position(|a| a == "--clearenv").unwrap();
        let first_setenv = args.iter().position(|a| a == "--setenv").unwrap();
        assert!(clear < first_setenv);

        let joined = args.join(" ");
        assert!(!joined.contains(FAKE_KEY_VAR));
        assert!(!joined.contains(FAKE_KEY_VALUE));
        assert!(joined.contains(&format!("--setenv PATH {SANDBOX_PATH}")));
        assert!(joined.contains("--setenv HOME /workspace"));

        // The bwrap process itself gets only an explicit PATH (env_clear()).
        let envs = envs_of(&prepared.cmd);
        assert_eq!(
            envs,
            vec![("PATH".to_string(), Some(SANDBOX_PATH.to_string()))]
        );
        assert!(prepared.secrets.is_none());
        std::env::remove_var(FAKE_KEY_VAR);
    }

    #[test]
    fn passthrough_forwards_only_named_non_reserved_vars() {
        std::env::set_var("BOBACLAW_TEST_BWRAP_PROXY", "http://proxy.invalid:3128");
        let dir = tempfile::tempdir().unwrap();
        let env = SandboxEnv::with_passthrough(&[
            "BOBACLAW_TEST_BWRAP_PROXY".into(),
            "PATH".into(),
            "bad-name".into(),
        ]);
        let prepared = prepare_exec_command(
            "/usr/bin/bwrap",
            &ExecutorProfile::bwrap_default(),
            &dir.path().join("ws"),
            &dir.path().join("run"),
            "true",
            &env,
        )
        .unwrap();
        let args = args_of(&prepared.cmd);
        let joined = args.join(" ");
        assert!(joined.contains("--setenv BOBACLAW_TEST_BWRAP_PROXY http://proxy.invalid:3128"));
        assert_eq!(args.iter().filter(|a| *a == "PATH").count(), 1);
        assert!(!joined.contains("bad-name"));
        std::env::remove_var("BOBACLAW_TEST_BWRAP_PROXY");
    }

    #[test]
    fn secrets_go_to_private_env_file_not_argv_or_capsule() {
        let dir = tempfile::tempdir().unwrap();
        let run = dir.path().join("run");
        let env = SandboxEnv {
            passthrough: Vec::new(),
            secrets: vec![("OPENAI_API_KEY".into(), FAKE_KEY_VALUE.into())],
        };
        let prepared = prepare_exec_command(
            "/usr/bin/bwrap",
            &ExecutorProfile::bwrap_default(),
            &dir.path().join("ws"),
            &run,
            "codex exec --json 'hi'",
            &env,
        )
        .unwrap();

        let joined = args_of(&prepared.cmd).join(" ");
        assert!(!joined.contains(FAKE_KEY_VALUE));
        assert!(joined.contains(SECRET_ENV_GUEST_PATH));
        assert!(envs_of(&prepared.cmd)
            .iter()
            .all(|(_, v)| v.as_deref() != Some(FAKE_KEY_VALUE)));

        let script = std::fs::read_to_string(run.join("script.sh")).unwrap();
        assert_eq!(script, "codex exec --json 'hi'");
        let manifest = std::fs::read_to_string(run.join("capsule.yaml")).unwrap();
        assert!(!manifest.contains(FAKE_KEY_VALUE));

        let secret = prepared.secrets.as_ref().unwrap();
        let secret_path = secret.path().to_path_buf();
        assert!(!secret_path.starts_with(run.canonicalize().unwrap()));
        let body = std::fs::read_to_string(&secret_path).unwrap();
        assert_eq!(body, format!("OPENAI_API_KEY='{FAKE_KEY_VALUE}'\n"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&secret_path)
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }

        drop(prepared);
        assert!(!secret_path.exists());
    }

    #[test]
    fn secret_env_file_rejects_bad_names_and_quotes_values() {
        assert!(SecretEnvFile::write(&[("A=B".into(), "x".into())]).is_err());
        let f = SecretEnvFile::write(&[("K".into(), "it's".into())]).unwrap();
        let body = std::fs::read_to_string(f.path()).unwrap();
        assert_eq!(body, "K='it'\\''s'\n");
    }
}
