/// Parse `/cmd` or `/cmd@botname` from a Telegram message.
pub fn parse_slash_command<'a>(text: &'a str, bot_username: Option<&str>) -> Option<(&'a str, &'a str)> {
    let text = text.trim();
    if !text.starts_with('/') {
        return None;
    }
    let first = text.split_whitespace().next()?;
    let mut cmd = first.trim_start_matches('/');
    if let Some(at) = cmd.find('@') {
        let mention = &cmd[at + 1..];
        match bot_username {
            Some(bot) if mention.eq_ignore_ascii_case(bot) => cmd = &cmd[..at],
            Some(_) => return None,
            None => cmd = &cmd[..at],
        }
    }
    if cmd.is_empty() {
        return None;
    }
    let rest = text[first.len()..].trim();
    Some((cmd, rest))
}

pub fn telegram_help_text() -> &'static str {
    "Команды BobaClaw:\n\
     /new — новая сессия (сброс истории чата)\n\
     /help — эта справка\n\
     /pair — код pairing (личные сообщения)"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_new_command() {
        assert_eq!(
            parse_slash_command("/new", Some("bobaClawBot")),
            Some(("new", ""))
        );
        assert_eq!(
            parse_slash_command("/new@bobaClawBot", Some("bobaClawBot")),
            Some(("new", ""))
        );
        assert_eq!(parse_slash_command("/new@otherBot", Some("bobaClawBot")), None);
    }

    #[test]
    fn parse_help_with_bot_suffix() {
        assert_eq!(
            parse_slash_command("/help@MyBot extra", Some("MyBot")),
            Some(("help", "extra"))
        );
    }
}
