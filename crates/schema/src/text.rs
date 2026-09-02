//! Char-boundary-safe text truncation.
//!
//! Transcript text (user prompts, tool output, commit subjects, ...) is arbitrary
//! UTF-8 and can contain multi-byte characters anywhere, so a byte-index slice like
//! `&s[..80]` panics the moment the cut lands inside one. Every helper here cuts on
//! a `char` boundary instead, so it can never panic no matter what the input is.

/// Cut `s` to at most `max_chars` characters, landing on a char boundary.
///
/// Cheap: character *count* is O(n) either way, but this only counts far enough to
/// find the `max_chars`-th boundary rather than the whole string.
#[allow(
    clippy::string_slice,
    reason = "idx comes from char_indices(), always a char boundary -- this is the safe primitive every other allow in the workspace points back to"
)]
pub fn truncate_chars(s: &str, max_chars: usize) -> &str {
    match s.char_indices().nth(max_chars) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

/// Keep the last `max_chars` characters of `s`, landing on a char boundary.
#[allow(
    clippy::string_slice,
    reason = "idx comes from char_indices(), always a char boundary -- this is the safe primitive every other allow in the workspace points back to"
)]
pub fn tail_chars(s: &str, max_chars: usize) -> &str {
    let total = s.chars().count();
    if total <= max_chars {
        return s;
    }
    let skip = total - max_chars;
    match s.char_indices().nth(skip) {
        Some((idx, _)) => &s[idx..],
        // `skip == total` only when `max_chars` is 0 (we already returned above for
        // `total <= max_chars`), and `nth()` has no `total`-th char to report -- that
        // is "keep zero characters", not "keep them all".
        None => "",
    }
}

/// Trim `s`, then cut to at most `max_chars` characters, appending `…` when
/// anything was cut. What most call sites actually want when showing a snippet
/// of transcript text in a table, log line, or summary.
pub fn preview(s: &str, max_chars: usize) -> String {
    let trimmed = s.trim();
    if trimmed.chars().count() <= max_chars {
        trimmed.to_string()
    } else {
        format!("{}…", truncate_chars(trimmed, max_chars))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MULTIBYTE: &str = "abcאבגד你好こんにちは🎉🎉👨‍👩‍👧‍👦e\u{0301}f\u{0301}gh";

    #[test]
    fn truncate_chars_never_panics_and_stays_on_boundary() {
        for n in 0..=MULTIBYTE.chars().count() + 5 {
            let cut = truncate_chars(MULTIBYTE, n);
            assert!(MULTIBYTE.is_char_boundary(cut.len()));
            assert_eq!(cut.chars().count().min(n), cut.chars().count());
        }
    }

    #[test]
    fn truncate_chars_exact_and_short() {
        assert_eq!(truncate_chars("hello", 3), "hel");
        assert_eq!(truncate_chars("hello", 100), "hello");
        assert_eq!(truncate_chars("hello", 0), "");
    }

    #[test]
    fn truncate_chars_at_every_old_panic_offset() {
        // The crash was a byte-offset cut (79/80/81) landing inside a 2-byte Hebrew
        // char. Char-count cuts can't reproduce that class of bug, but assert the
        // boundary holds at counts that straddle where those byte offsets used to.
        let s = "ך".repeat(200); // every char is 2 bytes
        for n in [0, 1, 39, 40, 41, 60, 100, 200, 500] {
            let cut = truncate_chars(&s, n);
            assert!(s.is_char_boundary(cut.len()));
        }
    }

    #[test]
    fn tail_chars_keeps_last_n_and_stays_on_boundary() {
        assert_eq!(tail_chars("hello", 3), "llo");
        assert_eq!(tail_chars("hello", 100), "hello");
        assert_eq!(tail_chars("hello", 0), "");
        for n in 0..=MULTIBYTE.chars().count() + 5 {
            let cut = tail_chars(MULTIBYTE, n);
            assert!(MULTIBYTE.is_char_boundary(MULTIBYTE.len() - cut.len()));
        }
    }

    #[test]
    fn preview_trims_and_appends_ellipsis_only_when_cut() {
        assert_eq!(preview("  hello  ", 10), "hello");
        assert_eq!(preview("hello world", 5), "hello…");
        assert_eq!(preview(&"ם".repeat(10), 5), format!("{}…", "ם".repeat(5)));
    }

    #[test]
    fn preview_never_panics_on_random_utf8_lengths() {
        // Lightweight property check over the existing multibyte fixture plus
        // straddling lengths around the old byte-index panic points (79-81).
        let fixtures = [
            MULTIBYTE.to_string(),
            "ك".repeat(163), // 2 bytes/char: 163 chars = 326 bytes, straddles 79-81 many times over
            "你".repeat(100), // 3 bytes/char
            "🎉".repeat(50),  // 4 bytes/char
            "e\u{0301}".repeat(80), // combining marks
        ];
        for f in &fixtures {
            for n in [0usize, 1, 2, 39, 40, 41, 79, 80, 81, 120, 200, 1000] {
                let _ = preview(f, n);
                let _ = truncate_chars(f, n);
                let _ = tail_chars(f, n);
            }
        }
    }
}
