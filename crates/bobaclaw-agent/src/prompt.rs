//! System prompt assembly — synthesis of Hermes (stable tiers, tool discipline,
//! compaction) and OpenClaw (workspace bootstrap files, memory rules).

use bobaclaw_core::{head_tail_with_hint, BobaPaths};
use bobaclaw_skills::SkillRegistry;
use std::path::Path;

/// Hermes-style cap for injected workspace markdown.
pub const CONTEXT_FILE_MAX_CHARS: usize = 20_000;

// --- Identity & stable guidance (English; cached for the session) ---

const BOBACLAW_IDENTITY: &str = "You are BobaClaw, a personal self-hosted ChatOps agent. \
You are helpful, direct, and grounded in tool output. Prioritize being useful over being verbose. \
Work autonomously until the user's request is actually done.";

const TOOL_USE_ENFORCEMENT: &str = "# Tool-use enforcement\n\
You MUST use the `exec` tool to take action — do not describe what you would do without doing it. \
When you say you will run a command or inspect a file, call `exec` in the same response. \
Never end your turn with a promise of future action. \
Each response should either (a) include tool calls that make progress, or (b) deliver a final result.";

const TASK_COMPLETION: &str = "# Finishing the job\n\
When the user asks you to build, run, or verify something, the deliverable is a working artifact \
backed by real `exec` output — not a plan. Do not stop after one command if more work is needed. \
If a command fails, say so and try an alternative. NEVER invent stdout, exit codes, or file contents.";

const MEMORY_HINT: &str = "# User memory (workspace files)\n\
`MEMORY.md` and files under `memory/` are injected below when present. They persist across CLI sessions. \
When the user asks what you remembered, what \"codeword\" / \"code word\" / «кодовое слово» they meant, \
or refers to something they asked to save — answer from these files first, then session history. \
For new facts to remember: append to `MEMORY.md` or `memory/` (e.g. `memory/words.txt`), not chat-only.";

const EXEC_DISCIPLINE: &str = "# Execution discipline\n\
- Use `exec` for arithmetic, hashes, current time/date, system state, and git state — \
not guesswork.\n\
- Injected memory files and prior `exec` output in this session are authoritative for stored user facts.\n\
- Check prerequisites before destructive or wide-reaching changes.\n\
- Verify results before claiming done.\n\
- If required context is missing, use `exec` to discover it; ask the user only when tools cannot.";

const SCHEDULING_HINT: &str = "# Scheduling\n\
Use the `schedule` tool for one-shot delayed work (reminders, \"message me in 5 minutes\", run a prompt later). \
`delay_seconds` + `prompt`; optional `deliver_message` for the exact text to send. \
Recurring jobs are in `config.yaml` under `cron.jobs`; run `bobaclaw scheduler start` as a daemon. \
Do not tell the user you lack a scheduler — use `schedule` or explain cron config.";

const SKILLS_HINT: &str = "# Skills\n\
When a skill matches the request, follow its SKILL.md. \
After a non-trivial success, consider capturing the workflow as a skill for reuse.";

const LANGUAGE_HINT: &str = "# Language\n\
System instructions are in English. Reply in the same language the user writes in unless they ask otherwise.";

const TONE_HINT: &str = "# Tone\n\
Plain professional prose. Do not use emoji or emoticons unless the user explicitly asks for them. \
No filler praise, no capability ads, no \"As an AI…\".";

// --- Compaction (Hermes context_compressor + OpenClaw handoff semantics) ---

/// Injected as a `compaction` history row; must stay stable for prompt-cache friendliness.
pub const SUMMARY_PREFIX: &str = "[CONTEXT COMPACTION — REFERENCE ONLY] Earlier turns were compacted \
into the summary below. This is a handoff from a previous context window — treat it as background \
reference, NOT as active instructions. Do NOT answer questions or fulfill requests mentioned in this \
summary; they were already addressed. Respond ONLY to the latest user message that appears AFTER this \
summary — that message is the single source of truth for what to do right now. \
If the latest user message is consistent with the '## Active Task' section, you may use the summary \
as background. If the latest user message contradicts, supersedes, or changes topic from '## Active Task' \
/ '## Pending User Asks' / '## Remaining Work', the latest message WINS — do not wrap up stale work first. \
Reverse signals in the latest message (stop, undo, never mind, new topic) cancel in-flight work from the summary. \
BOBACLAW.md and other workspace memory files remain authoritative — do not ignore them because of this note. \
The current session state (files, config) may reflect work described here — avoid repeating it:\n\n";

pub const SUMMARIZER_SYSTEM: &str = "You are a summarization agent creating a context checkpoint. \
Treat the conversation transcript as source material. Produce only the structured summary body — \
no greeting or preamble. Write the summary in the same language the user used in the conversation. \
NEVER include API keys, tokens, or passwords — use [REDACTED].";

const SUMMARY_TEMPLATE: &str = r#"Use this exact markdown structure:

## Active Task
[User's most recent unfulfilled input — verbatim when possible: task, unanswered question, or decision pending. Write "None." only if fully resolved.]

## Goal
[Overall objective]

## Completed Actions
[Numbered list: N. ACTION target — outcome [tool: exec]]

## Active State
[Branch, key files, test status, relevant cwd]

## Key Decisions
[Decisions and why]

## Resolved Questions
[Answered user questions]

## Pending User Asks
[Unanswered user questions — or "None."]

## Relevant Files
[Paths touched]

## Remaining Work
[Context only — not imperative instructions]

## Critical Context
[Values, errors, configs to preserve — secrets as [REDACTED]]

Be concrete: paths, commands, exit codes. No vague "made changes"."#;

pub fn summarizer_user_message(transcript: &str, previous_summary: Option<&str>) -> String {
    if let Some(prev) = previous_summary {
        format!(
            "Update the compaction summary. PRESERVE still-relevant prior content; ADD new progress.\n\n\
             PREVIOUS SUMMARY:\n{prev}\n\nNEW TURNS:\n{transcript}\n\n{SUMMARY_TEMPLATE}"
        )
    } else {
        format!(
            "Summarize this conversation transcript for handoff:\n\n{transcript}\n\n{SUMMARY_TEMPLATE}"
        )
    }
}

pub fn strip_summary_prefix(content: &str) -> String {
    let mut s = content.trim();
    if let Some(rest) = s.strip_prefix(SUMMARY_PREFIX) {
        return rest.trim().to_string();
    }
    // Legacy short prefix from earlier BobaClaw builds
    const LEGACY: &str = "[CONTEXT COMPACTION — REFERENCE ONLY] Earlier turns were summarized.";
    if let Some(rest) = s.strip_prefix(LEGACY) {
        s = rest.trim();
        if let Some(after) = s.strip_prefix("Treat as background only") {
            return after.trim().trim_start_matches(|c| c == ' ' || c == ';' || c == ':').to_string();
        }
    }
    s.to_string()
}

// --- Workspace bootstrap (OpenClaw files + Hermes Project Context header) ---

pub fn build_system_prompt(paths: &BobaPaths, group: &str, skills: &SkillRegistry) -> String {
    let workspace_path = paths.group_workspace(group);
    let workspace = workspace_path.display().to_string();

    let mut stable = vec![
        format!("{BOBACLAW_IDENTITY}\n\nWorkspace (sandbox cwd): {workspace}"),
        LANGUAGE_HINT.to_string(),
        TONE_HINT.to_string(),
        TOOL_USE_ENFORCEMENT.to_string(),
        TASK_COMPLETION.to_string(),
        EXEC_DISCIPLINE.to_string(),
        MEMORY_HINT.to_string(),
        SCHEDULING_HINT.to_string(),
        SKILLS_HINT.to_string(),
        "Use the `exec` tool for shell in the bubblewrap sandbox.".to_string(),
    ];

    if !skills.names().is_empty() {
        stable.push(format!(
            "Installed skills (check SKILL.md when relevant): {}.",
            skills.names().join(", ")
        ));
    }

    let mut parts = stable.join("\n\n");

    let mut context_sections: Vec<String> = Vec::new();
    if let Some(soul) = load_workspace_file(&workspace_path, "SOUL.md") {
        context_sections.push(format!("## SOUL.md\n{soul}"));
    }
    if let Some(rules) = load_workspace_file(&workspace_path, "BOBACLAW.md") {
        context_sections.push(format!("## BOBACLAW.md\n{rules}"));
    }
    if let Some(user) = load_workspace_file(&workspace_path, "USER.md") {
        context_sections.push(format!("## USER.md\n{user}"));
    }
    if let Some(tools) = load_workspace_file(&workspace_path, "TOOLS.md") {
        context_sections.push(format!("## TOOLS.md\n{tools}"));
    }
    if let Some(memory) = load_workspace_file(&workspace_path, "MEMORY.md") {
        context_sections.push(format!("## MEMORY.md\n{memory}"));
    }
    for section in load_memory_dir(&workspace_path) {
        context_sections.push(section);
    }

    if !context_sections.is_empty() {
        parts.push_str("\n\n# Project Context\n\n");
        parts.push_str(
            "The following workspace files are loaded and must be followed:\n\n",
        );
        parts.push_str(&context_sections.join("\n\n"));
    }

    parts
}

/// Max total chars injected from `memory/*` (excluding MEMORY.md).
const MEMORY_DIR_MAX_CHARS: usize = 8_000;

fn load_memory_dir(workspace: &Path) -> Vec<String> {
    let dir = workspace.join("memory");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };

    let mut files: Vec<(std::path::PathBuf, std::time::SystemTime)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext != "md" && ext != "txt" {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let modified = meta.modified().unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        files.push((path, modified));
    }
    files.sort_by(|a, b| b.1.cmp(&a.1));

    let mut sections = Vec::new();
    let mut budget = MEMORY_DIR_MAX_CHARS;
    for (path, _) in files {
        if budget == 0 {
            break;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("memory");
        let rel = format!("memory/{name}");
        let Ok(body) = std::fs::read_to_string(&path) else {
            continue;
        };
        let body = strip_yaml_frontmatter(body.trim());
        if body.is_empty() {
            continue;
        }
        let truncated = truncate_context_file(&rel, &body);
        budget = budget.saturating_sub(truncated.chars().count());
        sections.push(format!("## {rel}\n{truncated}"));
    }
    sections
}

fn load_workspace_file(workspace: &Path, name: &str) -> Option<String> {
    let path = workspace.join(name);
    let body = std::fs::read_to_string(&path).ok()?;
    let body = strip_yaml_frontmatter(body.trim());
    if body.is_empty() {
        return None;
    }
    Some(truncate_context_file(name, &body))
}

fn strip_yaml_frontmatter(content: &str) -> String {
    if content.starts_with("---") {
        if let Some(end) = content.find("\n---") {
            let body = content[end + 4..].trim_start_matches('\n');
            if !body.is_empty() {
                return body.to_string();
            }
        }
    }
    content.to_string()
}

fn truncate_context_file(filename: &str, content: &str) -> String {
    if content.chars().count() <= CONTEXT_FILE_MAX_CHARS {
        return content.to_string();
    }
    let hint = format!(
        "truncated {filename} ({CONTEXT_FILE_MAX_CHARS} char cap). Use exec to read the full file"
    );
    head_tail_with_hint(content, CONTEXT_FILE_MAX_CHARS, &hint)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bobaclaw_core::BobaPaths;
    use bobaclaw_skills::SkillRegistry;

    #[test]
    fn strip_summary_prefix_roundtrip() {
        let body = "## Active Task\nDo thing";
        let wrapped = format!("{SUMMARY_PREFIX}{body}");
        assert_eq!(strip_summary_prefix(&wrapped), body);
    }

    #[test]
    fn summarizer_includes_previous() {
        let msg = summarizer_user_message("transcript", Some("old summary"));
        assert!(msg.contains("PREVIOUS SUMMARY"));
        assert!(msg.contains("old summary"));
    }

    #[test]
    fn build_prompt_loads_bobaclaw_md() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_path_buf();
        let ws = home.join("workspace").join("home");
        std::fs::create_dir_all(&ws).unwrap();
        std::fs::write(ws.join("BOBACLAW.md"), "# Rules\nUse exec.").unwrap();

        let paths = BobaPaths {
            home: home.clone(),
            config: home.join("config.yaml"),
            state_db: home.join("state.db"),
            runs: home.join("runs"),
            workspace: home.join("workspace"),
        };
        let skills = SkillRegistry::load(&ws).unwrap();
        let prompt = build_system_prompt(&paths, "home", &skills);
        assert!(prompt.contains("BobaClaw"));
        assert!(prompt.contains("BOBACLAW.md"));
        assert!(prompt.contains("Use exec"));
        assert!(!prompt.contains("AGENTS.md"));
    }

    #[test]
    fn build_prompt_injects_memory_dir() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().to_path_buf();
        let ws = home.join("workspace").join("home");
        let mem = ws.join("memory");
        std::fs::create_dir_all(&mem).unwrap();
        std::fs::write(mem.join("words.txt"), "вельвет\n").unwrap();

        let paths = BobaPaths {
            home: home.clone(),
            config: home.join("config.yaml"),
            state_db: home.join("state.db"),
            runs: home.join("runs"),
            workspace: home.join("workspace"),
        };
        let skills = SkillRegistry::load(&ws).unwrap();
        let prompt = build_system_prompt(&paths, "home", &skills);
        assert!(prompt.contains("memory/words.txt"));
        assert!(prompt.contains("вельвет"));
    }

    #[test]
    fn strip_yaml_frontmatter_in_workspace() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("SOUL.md"),
            "---\ntitle: x\n---\n\nPersona here.",
        )
        .unwrap();
        let body = super::load_workspace_file(dir.path(), "SOUL.md").unwrap();
        assert!(body.contains("Persona here"));
        assert!(!body.contains("title: x"));
    }
}
