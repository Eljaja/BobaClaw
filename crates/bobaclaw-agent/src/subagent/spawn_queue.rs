use bobaclaw_state::SpawnJobRecord;

/// User-facing list of background spawn jobs (newest first).
pub fn format_spawn_task_list(tasks: &[SpawnJobRecord]) -> String {
    if tasks.is_empty() {
        return "Нет фоновых субагентов (spawn) для этой сессии.".into();
    }
    let mut lines = vec!["Фоновые субагенты (spawn):".to_string()];
    for t in tasks {
        let label = t.label.as_deref().unwrap_or("без метки");
        let sub = t
            .subagent_id
            .as_deref()
            .map(|id| format!(" subagent={id}"))
            .unwrap_or_default();
        let finished = t
            .finished_at
            .map(|ts| format!(" finished={ts:.0}"))
            .unwrap_or_default();
        let detail = t
            .result_preview
            .as_deref()
            .map(|p| format!("\n    {p}"))
            .unwrap_or_default();
        lines.push(format!(
            "  • {} [{label}] — {}{}{}{}",
            t.id, t.status, sub, finished, detail
        ));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_job(id: &str, status: &str) -> SpawnJobRecord {
        SpawnJobRecord {
            id: id.into(),
            subagent_id: None,
            session_id: "sess_a".into(),
            agent_group: "home".into(),
            ingress: "cli".into(),
            deliver_channel: Some("cli".into()),
            deliver_peer: None,
            deliver_thread_id: None,
            label: Some("research".into()),
            task_preview: None,
            backend: None,
            status: status.into(),
            exit_code: None,
            result_preview: None,
            result_body: None,
            parent_request_id: None,
            wake_parent: true,
            notified_at: None,
            created_at: 1.0,
            started_at: Some(1.0),
            finished_at: None,
        }
    }

    #[test]
    fn format_empty_list() {
        assert!(format_spawn_task_list(&[]).contains("Нет фоновых"));
    }

    #[test]
    fn format_running_and_completed() {
        let mut done = sample_job("spawn_2", "completed");
        done.result_preview = Some("done".into());
        let tasks = vec![sample_job("spawn_1", "running"), done];
        let out = format_spawn_task_list(&tasks);
        assert!(out.contains("spawn_1"));
        assert!(out.contains("research"));
        assert!(out.contains("running"));
        assert!(out.contains("done"));
    }
}
