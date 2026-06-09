use bobaclaw_core::TOOL_BODY_PERSIST_MAX_CHARS;
use bobaclaw_provider::ConversationMessage;

const CHARS_PER_TOKEN: usize = 4;

pub fn estimate_tokens(messages: &[ConversationMessage]) -> u32 {
    let chars: usize = messages
        .iter()
        .map(|m| {
            let mut c = m.text_content().len();
            if let Some(tc) = &m.tool_calls {
                for call in tc {
                    c += call.function.name.len() + call.function.arguments.len();
                }
            }
            c + m.role.len()
        })
        .sum();
    (chars / CHARS_PER_TOKEN).max(1) as u32
}

pub fn transcript_lines(messages: &[(String, String)]) -> String {
    let mut out = String::new();
    for (role, content) in messages {
        if role == "compaction" {
            continue;
        }
        let body = if role == "tool" {
            prune_old_tool_body(content)
        } else {
            content.clone()
        };
        out.push_str(&format!("[{role}]\n{body}\n\n"));
    }
    out
}

fn prune_old_tool_body(body: &str) -> String {
    if body.len() > TOOL_BODY_PERSIST_MAX_CHARS {
        "[Old tool output cleared to save context — full log in run_dir/result.json if needed]"
            .to_string()
    } else {
        body.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bobaclaw_provider::ConversationMessage;

    #[test]
    fn estimate_tokens_counts_roles() {
        let msgs = vec![
            ConversationMessage::user("hello world"),
            ConversationMessage::assistant_text("ok"),
        ];
        assert!(estimate_tokens(&msgs) >= 2);
    }

    #[test]
    fn transcript_skips_compaction_role() {
        let rows = vec![
            ("user".into(), "hi".into()),
            ("compaction".into(), "summary".into()),
            ("assistant".into(), "bye".into()),
        ];
        let t = transcript_lines(&rows);
        assert!(!t.contains("[compaction]"));
        assert!(t.contains("[user]"));
    }

    #[test]
    fn transcript_prunes_huge_tool_output() {
        let rows = vec![("tool".into(), "x".repeat(5000))];
        let t = transcript_lines(&rows);
        assert!(t.contains("cleared to save context"));
    }
}
