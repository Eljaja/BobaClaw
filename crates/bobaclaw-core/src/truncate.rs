/// Head+tail preserve for huge tool transcripts (Hermes/OpenClaw style), with artifact hint.
pub fn head_tail_with_hint(content: &str, max_chars: usize, hint: &str) -> String {
    if content.chars().count() <= max_chars {
        return content.to_string();
    }
    let head_chars = (max_chars as f64 * 0.65) as usize;
    let tail_chars = max_chars.saturating_sub(head_chars).saturating_sub(hint.chars().count() + 40);
    let head: String = content.chars().take(head_chars).collect();
    let tail: String = content
        .chars()
        .rev()
        .take(tail_chars)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("{head}\n\n[... {hint} ...]\n\n{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unchanged_when_under_limit() {
        let s = "hello";
        assert_eq!(head_tail_with_hint(s, 100, "hint"), "hello");
    }

    #[test]
    fn truncates_with_hint() {
        let s = "a".repeat(500);
        let out = head_tail_with_hint(&s, 80, "artifact");
        assert!(out.contains("artifact"));
        assert!(out.chars().count() < 500);
    }
}
