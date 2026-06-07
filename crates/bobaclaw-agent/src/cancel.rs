use tokio_util::sync::CancellationToken;

/// User-visible suffix when a turn stops mid-reply (Hermes `*[interrupted]*`).
pub const INTERRUPTED_MARKER: &str = "\n\n*[прервано]*";

/// Reply when the turn produced no assistant text before interrupt.
pub const INTERRUPTED_TEXT: &str = "⚡ Запрос прерван.";

pub use bobaclaw_core::TurnInterrupted;

pub fn check_cancel(token: &CancellationToken) -> Result<(), TurnInterrupted> {
    if token.is_cancelled() {
        Err(TurnInterrupted)
    } else {
        Ok(())
    }
}

/// Merge partial assistant text with the standard interrupt marker.
pub fn interrupted_reply(partial: &str) -> String {
    let trimmed = partial.trim();
    if trimmed.is_empty() {
        INTERRUPTED_TEXT.to_string()
    } else {
        format!("{trimmed}{INTERRUPTED_MARKER}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interrupted_reply_empty() {
        assert_eq!(interrupted_reply(""), INTERRUPTED_TEXT);
        assert_eq!(interrupted_reply("  \n"), INTERRUPTED_TEXT);
    }

    #[test]
    fn interrupted_reply_partial() {
        let out = interrupted_reply("частичный ответ");
        assert!(out.contains("частичный ответ"));
        assert!(out.contains("*[прервано]*"));
    }
}
