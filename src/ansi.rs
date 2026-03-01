use unicode_width::UnicodeWidthChar;

/// Check if we're inside an ANSI escape sequence starting at `bytes[pos]`.
/// Returns the length of the escape sequence if found, or 0.
fn escape_seq_len(bytes: &[u8], pos: usize) -> usize {
    if bytes[pos] != 0x1b {
        return 0;
    }
    if pos + 1 >= bytes.len() {
        return 1;
    }
    match bytes[pos + 1] {
        b'[' => {
            // CSI sequence: ESC [ ... final_byte
            let mut i = pos + 2;
            while i < bytes.len() {
                let b = bytes[i];
                if (0x40..=0x7e).contains(&b) {
                    return i - pos + 1;
                }
                i += 1;
            }
            // Unterminated CSI — consume what we have
            bytes.len() - pos
        }
        b']' => {
            // OSC sequence: ESC ] ... ST (ESC \ or BEL)
            let mut i = pos + 2;
            while i < bytes.len() {
                if bytes[i] == 0x07 {
                    return i - pos + 1;
                }
                if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                    return i - pos + 2;
                }
                i += 1;
            }
            bytes.len() - pos
        }
        0x20..=0x2f => {
            // Two-byte escape like ESC ( B
            if pos + 2 < bytes.len() {
                3
            } else {
                2
            }
        }
        _ => 2, // Simple two-byte escape
    }
}

/// Strip all ANSI escape sequences from a line, returning visible text only.
pub fn strip_ansi(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < bytes.len() {
        let esc_len = escape_seq_len(bytes, i);
        if esc_len > 0 {
            i += esc_len;
        } else {
            // Safe because we're walking byte by byte and only skipping escape sequences
            // We need to handle UTF-8 properly
            let rest = &line[i..];
            if let Some(ch) = rest.chars().next() {
                out.push(ch);
                i += ch.len_utf8();
            } else {
                i += 1;
            }
        }
    }
    out
}

/// Compute the visible (display) width of a line, ignoring ANSI escapes.
pub fn visible_width(line: &str) -> usize {
    let stripped = strip_ansi(line);
    display_width(&stripped)
}

/// Display width of a string (no ANSI expected).
fn display_width(s: &str) -> usize {
    s.chars()
        .map(|ch| UnicodeWidthChar::width(ch).unwrap_or(0))
        .sum()
}

/// Truncate a line for display at a given horizontal offset and max width.
/// Preserves ANSI escape state across the truncation boundary so colors work.
/// In raw mode, escapes are treated as visible characters.
pub fn truncate_to_width(line: &str, start_col: usize, max_width: usize, raw_mode: bool) -> String {
    if raw_mode {
        return truncate_raw(line, start_col, max_width);
    }

    let bytes = line.as_bytes();
    let mut out = String::with_capacity(max_width + 64);
    let mut col: usize = 0; // current visible column
    let mut emitting = false;
    let mut cols_emitted: usize = 0;

    // Track ANSI state to replay at the start of visible region
    let mut pending_escapes = String::new();
    let mut active_escapes = String::new();

    let mut i = 0;
    while i < bytes.len() && cols_emitted < max_width {
        let esc_len = escape_seq_len(bytes, i);
        if esc_len > 0 {
            let seq = &line[i..i + esc_len];
            if emitting {
                out.push_str(seq);
            } else {
                // Track SGR sequences (color/style) for replay
                if is_sgr_sequence(seq) {
                    if is_sgr_reset(seq) {
                        pending_escapes.clear();
                    } else {
                        pending_escapes.push_str(seq);
                    }
                }
            }
            active_escapes.clear();
            active_escapes.push_str(&pending_escapes);
            i += esc_len;
            continue;
        }

        let rest = &line[i..];
        let ch = match rest.chars().next() {
            Some(c) => c,
            None => {
                i += 1;
                continue;
            }
        };
        let w = if ch == '\t' {
            // Tab stops at every 8 columns
            8 - (col % 8)
        } else {
            UnicodeWidthChar::width(ch).unwrap_or(0)
        };

        if col + w > start_col && !emitting {
            // We've reached the visible region
            emitting = true;
            // Replay active ANSI state
            out.push_str(&active_escapes);

            // If this character straddles the start boundary (wide char partially hidden),
            // emit spaces for the visible portion
            if col < start_col {
                let visible_part = w - (start_col - col);
                for _ in 0..visible_part {
                    out.push(' ');
                }
                cols_emitted += visible_part;
                col += w;
                i += ch.len_utf8();
                continue;
            }
        }

        if emitting {
            if cols_emitted + w > max_width {
                // Wide character would overflow — fill with space if room
                if cols_emitted < max_width {
                    out.push(' ');
                }
                break;
            }
            if ch == '\t' {
                for _ in 0..w {
                    if cols_emitted < max_width {
                        out.push(' ');
                        cols_emitted += 1;
                    }
                }
            } else {
                out.push(ch);
                cols_emitted += w;
            }
        }

        col += w;
        if col >= start_col && !emitting {
            emitting = true;
            out.push_str(&active_escapes);
        }
        i += ch.len_utf8();
    }

    // Reset attributes if we emitted any ANSI
    if !pending_escapes.is_empty() && !out.is_empty() {
        out.push_str("\x1b[m");
    }

    out
}

/// In raw mode, treat escape sequences as visible characters.
fn truncate_raw(line: &str, start_col: usize, max_width: usize) -> String {
    let mut out = String::with_capacity(max_width);
    let mut col: usize = 0;
    let mut cols_emitted: usize = 0;

    for ch in line.chars() {
        let w = if ch == '\t' {
            8 - (col % 8)
        } else {
            UnicodeWidthChar::width(ch).unwrap_or(1) // In raw mode, control chars take 1 col
        };

        if col + w > start_col && cols_emitted == 0 && col < start_col {
            // Wide char straddles start
            let visible = w - (start_col - col);
            for _ in 0..visible.min(max_width) {
                out.push(' ');
                cols_emitted += 1;
            }
            col += w;
            continue;
        }

        if col >= start_col {
            if cols_emitted + w > max_width {
                break;
            }
            if ch == '\t' {
                for _ in 0..w {
                    if cols_emitted < max_width {
                        out.push(' ');
                        cols_emitted += 1;
                    }
                }
            } else {
                out.push(ch);
                cols_emitted += w;
            }
        }
        col += w;
    }
    out
}

fn is_sgr_sequence(seq: &str) -> bool {
    let bytes = seq.as_bytes();
    bytes.len() >= 3
        && bytes[0] == 0x1b
        && bytes[1] == b'['
        && bytes[bytes.len() - 1] == b'm'
}

fn is_sgr_reset(seq: &str) -> bool {
    seq == "\x1b[m" || seq == "\x1b[0m"
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- strip_ansi ---

    #[test]
    fn strip_plain_text() {
        assert_eq!(strip_ansi("hello world"), "hello world");
    }

    #[test]
    fn strip_empty() {
        assert_eq!(strip_ansi(""), "");
    }

    #[test]
    fn strip_sgr_color() {
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m"), "red");
    }

    #[test]
    fn strip_multiple_sgr() {
        assert_eq!(
            strip_ansi("\x1b[1m\x1b[32mbold green\x1b[m normal"),
            "bold green normal"
        );
    }

    #[test]
    fn strip_256_color() {
        assert_eq!(strip_ansi("\x1b[38;5;196mred\x1b[m"), "red");
    }

    #[test]
    fn strip_truecolor() {
        assert_eq!(strip_ansi("\x1b[38;2;255;0;0mred\x1b[m"), "red");
    }

    #[test]
    fn strip_csi_cursor_movement() {
        assert_eq!(strip_ansi("\x1b[2Jhello\x1b[10;1H"), "hello");
    }

    #[test]
    fn strip_osc_sequence_bel() {
        assert_eq!(strip_ansi("\x1b]0;title\x07text"), "text");
    }

    #[test]
    fn strip_osc_sequence_st() {
        assert_eq!(strip_ansi("\x1b]0;title\x1b\\text"), "text");
    }

    #[test]
    fn strip_preserves_utf8() {
        assert_eq!(strip_ansi("\x1b[31m日本語\x1b[m"), "日本語");
    }

    #[test]
    fn strip_bare_escape_at_end() {
        assert_eq!(strip_ansi("text\x1b"), "text");
    }

    #[test]
    fn strip_unterminated_csi() {
        // ESC [ followed by no final byte — consumed as incomplete sequence
        assert_eq!(strip_ansi("\x1b[31"), "");
    }

    // --- visible_width ---

    #[test]
    fn width_plain_ascii() {
        assert_eq!(visible_width("hello"), 5);
    }

    #[test]
    fn width_empty() {
        assert_eq!(visible_width(""), 0);
    }

    #[test]
    fn width_with_ansi() {
        assert_eq!(visible_width("\x1b[31mhello\x1b[m"), 5);
    }

    #[test]
    fn width_cjk_characters() {
        // Each CJK char is 2 columns wide
        assert_eq!(visible_width("日本"), 4);
    }

    #[test]
    fn width_mixed_cjk_and_ascii() {
        assert_eq!(visible_width("hi日本"), 6);
    }

    #[test]
    fn width_colored_cjk() {
        assert_eq!(visible_width("\x1b[32m日本\x1b[m"), 4);
    }

    // --- truncate_to_width ---

    #[test]
    fn truncate_plain_fits() {
        assert_eq!(truncate_to_width("hello", 0, 10, false), "hello");
    }

    #[test]
    fn truncate_plain_clips() {
        assert_eq!(truncate_to_width("hello world", 0, 5, false), "hello");
    }

    #[test]
    fn truncate_with_offset() {
        assert_eq!(truncate_to_width("hello world", 6, 5, false), "world");
    }

    #[test]
    fn truncate_offset_beyond_content() {
        assert_eq!(truncate_to_width("hi", 10, 5, false), "");
    }

    #[test]
    fn truncate_preserves_color() {
        let result = truncate_to_width("\x1b[31mred text\x1b[m", 0, 8, false);
        assert!(result.starts_with("\x1b[31m"));
        assert!(result.contains("red text"));
        assert!(result.ends_with("\x1b[m"));
    }

    #[test]
    fn truncate_color_replayed_after_offset() {
        // Color set before offset should still be active at start of visible region
        let result = truncate_to_width("\x1b[31mred text\x1b[m", 4, 4, false);
        assert!(result.starts_with("\x1b[31m"));
        assert!(result.contains("text"));
        assert!(result.ends_with("\x1b[m"));
    }

    #[test]
    fn truncate_color_reset_before_offset_clears() {
        // Color set then reset before offset — no color replayed
        let result = truncate_to_width("\x1b[31mred\x1b[m plain", 4, 5, false);
        assert!(!result.contains("\x1b[31m"));
        assert!(result.contains("plain"));
    }

    #[test]
    fn truncate_tab_expansion() {
        let result = truncate_to_width("\thi", 0, 10, false);
        // Tab at col 0 expands to 8 spaces
        assert_eq!(&result[..8], "        ");
        assert!(result.contains("hi"));
    }

    #[test]
    fn truncate_empty() {
        assert_eq!(truncate_to_width("", 0, 80, false), "");
    }

    #[test]
    fn truncate_zero_width() {
        assert_eq!(truncate_to_width("hello", 0, 0, false), "");
    }

    #[test]
    fn truncate_cjk_at_boundary() {
        // Wide char that would straddle the max_width boundary gets replaced with space
        // "日" is 2 cols wide. With max_width=3, "日X" (4 cols) → "日 " (3 cols)
        let result = truncate_to_width("日X", 0, 3, false);
        assert_eq!(visible_width(&strip_ansi(&result)), 3);
    }

    #[test]
    fn truncate_cjk_straddling_offset() {
        // Wide char at col 0-1, start_col=1 → partial wide char → space
        let result = truncate_to_width("日b", 1, 3, false);
        assert!(result.starts_with(' '));
    }

    // --- raw mode truncation ---

    #[test]
    fn truncate_raw_shows_escapes() {
        let result = truncate_to_width("\x1b[31mhi", 0, 20, true);
        // In raw mode, the escape chars are visible, not stripped.
        // The ESC byte is present and the full string is output literally.
        assert_eq!(result, "\x1b[31mhi");
        // The visible width counts ESC and control chars, so it's wider than in normal mode
        assert!(result.len() > "hi".len());
    }

    #[test]
    fn truncate_raw_offset() {
        let result = truncate_to_width("abcdef", 2, 3, true);
        assert_eq!(result, "cde");
    }

    // --- is_sgr helpers ---

    #[test]
    fn sgr_detection() {
        assert!(is_sgr_sequence("\x1b[31m"));
        assert!(is_sgr_sequence("\x1b[0m"));
        assert!(is_sgr_sequence("\x1b[38;5;196m"));
        assert!(!is_sgr_sequence("\x1b[2J")); // clear screen, not SGR
        assert!(!is_sgr_sequence("\x1b[H"));  // cursor, not SGR
    }

    #[test]
    fn sgr_reset_detection() {
        assert!(is_sgr_reset("\x1b[m"));
        assert!(is_sgr_reset("\x1b[0m"));
        assert!(!is_sgr_reset("\x1b[31m"));
    }
}
