//! Shared utility functions.

/// Truncate a string to `max_chars` characters, appending "…" if truncated.
pub fn truncate_str(s: &str, max_chars: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        format!("{truncated}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_string_unchanged() {
        assert_eq!(truncate_str("hello", 80), "hello");
    }

    #[test]
    fn long_string_truncated_with_ellipsis() {
        let s = "a".repeat(100);
        let result = truncate_str(&s, 10);
        assert!(result.ends_with('…'));
        assert!(result.chars().count() <= 10);
    }

    #[test]
    fn multibyte_safe() {
        let s = "こんにちは世界"; // 7 chars
        let result = truncate_str(s, 5);
        assert!(result.ends_with('…'));
        assert_eq!(result.chars().count(), 5);
    }

    #[test]
    fn exact_length_unchanged() {
        assert_eq!(truncate_str("abcde", 5), "abcde");
    }
}
