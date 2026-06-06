/// Build the OpenAI tool name exposed to the LLM: `mcp_{server}_{tool}`.
pub fn prefixed_tool_name(server_name: &str, tool_name: &str) -> String {
    format!(
        "mcp_{}_{}",
        sanitize_component(server_name),
        sanitize_component(tool_name)
    )
}

pub fn sanitize_component(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "tool".into()
    } else if out.as_bytes()[0].is_ascii_digit() {
        format!("t_{out}")
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixed_name() {
        assert_eq!(
            prefixed_tool_name("filesystem", "read_file"),
            "mcp_filesystem_read_file"
        );
    }

    #[test]
    fn unsafe_chars_sanitized() {
        assert_eq!(sanitize_component("my-server"), "my_server");
    }
}
