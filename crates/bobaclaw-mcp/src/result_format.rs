use rmcp::model::{CallToolResult, Content, RawContent};

/// Turn MCP `CallToolResult` into text for the LLM tool message.
pub fn format_call_tool_result(result: &CallToolResult) -> String {
    if result.is_error.unwrap_or(false) {
        let body = format_content_blocks(&result.content);
        return if body.is_empty() {
            "MCP tool returned an error (no details)".into()
        } else {
            format!("MCP tool error:\n{body}")
        };
    }

    let body = format_content_blocks(&result.content);
    if body.is_empty() {
        if let Some(structured) = &result.structured_content {
            return serde_json::to_string_pretty(structured)
                .unwrap_or_else(|_| format!("{structured}"));
        }
        return "(MCP tool returned no content)".into();
    }
    body
}

fn format_content_blocks(blocks: &[Content]) -> String {
    let mut parts = Vec::new();
    for block in blocks {
        match &block.raw {
            RawContent::Text(t) => parts.push(t.text.clone()),
            RawContent::Image(img) => {
                parts.push(format!(
                    "[image: {} bytes, mime={}]",
                    img.data.len(),
                    img.mime_type
                ));
            }
            RawContent::Resource(res) => {
                parts.push(format!("[resource: {:?}]", res.resource));
            }
            RawContent::Audio(a) => {
                parts.push(format!("[audio: mime={}]", a.mime_type));
            }
            RawContent::ResourceLink(link) => {
                parts.push(format!("[resource link: {:?}]", link.uri));
            }
        }
    }
    parts.join("\n")
}
