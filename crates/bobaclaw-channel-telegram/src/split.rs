//! Split long text for Telegram's 4096 UTF-16 code unit limit.

pub const TELEGRAM_MAX_UTF16: usize = 4096;
/// Raw markdown chunk size before HTML formatting (headroom for tags/escaping).
pub const TELEGRAM_SPLIT_UTF16: usize = 3800;

pub fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

pub fn split_for_telegram(text: &str, max_utf16: usize) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    if utf16_len(text) <= max_utf16 {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut rest = text;

    while !rest.is_empty() {
        if utf16_len(rest) <= max_utf16 {
            chunks.push(rest.to_string());
            break;
        }

        let byte_end = find_split_byte_index(rest, max_utf16);
        let (head, tail) = rest.split_at(byte_end);
        chunks.push(head.to_string());
        rest = tail;
    }

    chunks
}

fn find_split_byte_index(text: &str, max_utf16: usize) -> usize {
    let mut utf16_count = 0;
    let mut last_paragraph_break: Option<usize> = None;
    let mut last_line_break: Option<usize> = None;
    let mut last_space_break: Option<usize> = None;
    let mut prev_char: Option<char> = None;

    for (byte_idx, ch) in text.char_indices() {
        let char_utf16 = ch.len_utf16();
        if utf16_count + char_utf16 > max_utf16 {
            break;
        }
        utf16_count += char_utf16;
        let after = byte_idx + ch.len_utf8();

        if ch == '\n' {
            if prev_char == Some('\n') {
                last_paragraph_break = Some(after);
            }
            last_line_break = Some(after);
        } else if ch.is_whitespace() {
            last_space_break = Some(after);
        }
        prev_char = Some(ch);
    }

    if let Some(idx) = last_paragraph_break
        .or(last_line_break)
        .or(last_space_break)
        .filter(|idx| *idx > 0)
    {
        return idx;
    }

    utf16_count = 0;
    for (byte_idx, ch) in text.char_indices() {
        let char_utf16 = ch.len_utf16();
        if utf16_count + char_utf16 > max_utf16 {
            return byte_idx.max(1);
        }
        utf16_count += char_utf16;
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_single_chunk() {
        let parts = split_for_telegram("hello", TELEGRAM_SPLIT_UTF16);
        assert_eq!(parts, vec!["hello"]);
    }

    #[test]
    fn splits_on_paragraph_boundary() {
        let head = "a".repeat(2000);
        let tail = "b".repeat(2000);
        let text = format!("{head}\n\n{tail}");
        let parts = split_for_telegram(&text, TELEGRAM_SPLIT_UTF16);
        assert_eq!(parts.len(), 2);
        assert!(utf16_len(&parts[0]) <= TELEGRAM_SPLIT_UTF16);
        assert!(utf16_len(&parts[1]) <= TELEGRAM_SPLIT_UTF16);
        assert_eq!(parts.join(""), text);
    }

    #[test]
    fn counts_utf16_for_emoji() {
        assert_eq!(utf16_len("a"), 1);
        assert_eq!(utf16_len("😀"), 2);
    }
}
