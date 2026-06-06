use regex::Regex;
use std::sync::LazyLock;

const MAX_NAME_LEN: usize = 64;
const MAX_DESC_LEN: usize = 1024;
const MAX_CONTENT_CHARS: usize = 100_000;

static VALID_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[a-z0-9][a-z0-9._-]*$").unwrap());

pub fn validate_name(name: &str) -> Option<String> {
    if name.is_empty() {
        return Some("Skill name is required.".into());
    }
    if name.len() > MAX_NAME_LEN {
        return Some(format!("Skill name exceeds {MAX_NAME_LEN} characters."));
    }
    if !VALID_NAME.is_match(name) {
        return Some(format!(
            "Invalid skill name '{name}'. Use lowercase letters, numbers, hyphens, dots, underscores."
        ));
    }
    None
}

pub fn validate_category(category: Option<&str>) -> Option<String> {
    let Some(category) = category else {
        return None;
    };
    let category = category.trim();
    if category.is_empty() {
        return None;
    }
    if category.contains('/') || category.contains('\\') {
        return Some(format!("Invalid category '{category}'. Must be a single directory name."));
    }
    validate_name(category)
}

pub fn validate_frontmatter(content: &str) -> Option<String> {
    if content.trim().is_empty() {
        return Some("Content cannot be empty.".into());
    }
    if !content.starts_with("---") {
        return Some("SKILL.md must start with YAML frontmatter (---).".into());
    }
    let Some(end) = content[3..].find("\n---") else {
        return Some("SKILL.md frontmatter is not closed.".into());
    };
    let front = &content[3..3 + end];
    let fm: serde_yaml::Value = match serde_yaml::from_str(front) {
        Ok(v) => v,
        Err(e) => return Some(format!("YAML frontmatter parse error: {e}")),
    };
    let Some(obj) = fm.as_mapping() else {
        return Some("Frontmatter must be a YAML mapping.".into());
    };
    if !obj.contains_key("name") {
        return Some("Frontmatter must include 'name' field.".into());
    }
    let desc = obj.get("description").and_then(|v| v.as_str()).unwrap_or("");
    if desc.is_empty() {
        return Some("Frontmatter must include 'description' field.".into());
    }
    if desc.len() > MAX_DESC_LEN {
        return Some(format!("Description exceeds {MAX_DESC_LEN} characters."));
    }
    let body = content[3 + end + 4..].trim();
    if body.is_empty() {
        return Some("SKILL.md must have content after the frontmatter.".into());
    }
    None
}

pub fn validate_content_size(content: &str, label: &str) -> Option<String> {
    if content.len() > MAX_CONTENT_CHARS {
        return Some(format!(
            "{label} content is {} characters (limit: {MAX_CONTENT_CHARS}).",
            content.len()
        ));
    }
    None
}

pub const ALLOWED_SUBDIRS: &[&str] = &["references", "templates", "scripts", "assets"];

pub fn validate_support_path(file_path: &str) -> Option<String> {
    let path = file_path.trim().trim_start_matches('/');
    if path.is_empty() || path.contains("..") {
        return Some("Invalid file_path.".into());
    }
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() < 2 {
        return Some(
            "file_path must be under references/, templates/, scripts/, or assets/.".into(),
        );
    }
    if !ALLOWED_SUBDIRS.contains(&parts[0]) {
        return Some(format!(
            "Subdirectory '{}' is not allowed. Use: {}",
            parts[0],
            ALLOWED_SUBDIRS.join(", ")
        ));
    }
    None
}
