//! Cutting untrusted text to a byte budget without splitting a character.

/// The longest prefix of `value` that is at most `max_bytes` long and ends on
/// a character boundary.
pub(crate) fn truncate_utf8(value: &str, max_bytes: usize) -> &str {
    if value.len() <= max_bytes {
        return value;
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}

#[cfg(test)]
mod tests {
    use super::truncate_utf8;

    #[test]
    fn text_within_the_budget_is_kept() {
        assert_eq!(truncate_utf8("", 0), "");
        assert_eq!(truncate_utf8("abc", 3), "abc");
        assert_eq!(truncate_utf8("a日", 8), "a日");
    }

    #[test]
    fn a_character_across_the_budget_is_dropped_whole() {
        // "é" is two bytes, "日" three and "😀" four.
        assert_eq!(truncate_utf8("aé", 2), "a");
        assert_eq!(truncate_utf8("a日b", 2), "a");
        assert_eq!(truncate_utf8("a日b", 3), "a");
        assert_eq!(truncate_utf8("a日b", 4), "a日");
        assert_eq!(truncate_utf8("😀x", 3), "");
        assert_eq!(truncate_utf8("😀x", 4), "😀");
    }

    #[test]
    fn a_zero_budget_keeps_nothing() {
        assert_eq!(truncate_utf8("日本", 0), "");
        assert_eq!(truncate_utf8("abc", 0), "");
    }
}
