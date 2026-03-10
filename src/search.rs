use crate::ansi::strip_ansi;
use crate::buffer::LineBuffer;
use regex_lite::Regex;

/// Boyer-Moore-Horspool substring searcher.
///
/// Uses a 256-entry bad-character shift table to skip ahead when a mismatch
/// occurs. For literal patterns this is dramatically faster than regex because
/// it can skip over large chunks of the haystack without examining every byte.
struct Bmh {
    /// The needle bytes (lowercased if case-insensitive).
    needle: Vec<u8>,
    /// Bad-character shift table indexed by raw byte value.
    shift: [usize; 256],
    case_insensitive: bool,
}

impl Bmh {
    fn new(pattern: &[u8], case_insensitive: bool) -> Self {
        let needle: Vec<u8> = if case_insensitive {
            pattern.iter().map(|b| b.to_ascii_lowercase()).collect()
        } else {
            pattern.to_vec()
        };
        let n = needle.len();
        let mut shift = [n; 256];
        // Set shift for every byte in the needle except the last.
        for (i, &b) in needle.iter().enumerate().take(n.saturating_sub(1)) {
            shift[b as usize] = n - 1 - i;
            if case_insensitive {
                shift[b.to_ascii_uppercase() as usize] = n - 1 - i;
            }
        }
        Bmh {
            needle,
            shift,
            case_insensitive,
        }
    }

    /// Find first occurrence starting at `start`. Returns byte offset in haystack.
    fn find_from(&self, haystack: &[u8], start: usize) -> Option<usize> {
        let n = self.needle.len();
        if n == 0 {
            return Some(start);
        }
        if start + n > haystack.len() {
            return None;
        }
        let last = n - 1;
        let mut i = start;
        while i + n <= haystack.len() {
            let mut j = last;
            loop {
                let hb = if self.case_insensitive {
                    haystack[i + j].to_ascii_lowercase()
                } else {
                    haystack[i + j]
                };
                if hb != self.needle[j] {
                    break;
                }
                if j == 0 {
                    return Some(i);
                }
                j -= 1;
            }
            // Shift based on the aligned byte under the last needle position.
            // The shift table has entries for both cases, so raw byte is fine.
            i += self.shift[haystack[i + last] as usize];
        }
        None
    }

    fn is_match(&self, haystack: &[u8]) -> bool {
        self.find_from(haystack, 0).is_some()
    }

    fn find_all(&self, haystack: &[u8]) -> Vec<(usize, usize)> {
        let n = self.needle.len();
        if n == 0 {
            return vec![];
        }
        let mut matches = Vec::new();
        let mut start = 0;
        while let Some(pos) = self.find_from(haystack, start) {
            matches.push((pos, pos + n));
            start = pos + n;
        }
        matches
    }
}

/// Returns true if the pattern contains no regex metacharacters.
fn is_literal(pattern: &str) -> bool {
    !pattern.bytes().any(|b| {
        matches!(
            b,
            b'\\'
                | b'.'
                | b'^'
                | b'$'
                | b'*'
                | b'+'
                | b'?'
                | b'('
                | b')'
                | b'['
                | b']'
                | b'{'
                | b'}'
                | b'|'
        )
    })
}

enum Matcher {
    Literal(Box<Bmh>),
    Regex(Regex),
}

pub struct Search {
    pub pattern: String,
    matcher: Matcher,
}

impl Search {
    /// Create a new search. Smart-case: if pattern has uppercase → case-sensitive,
    /// otherwise case-insensitive.
    pub fn new(pattern: &str) -> Result<Self, String> {
        if pattern.is_empty() {
            return Err("Empty pattern".to_string());
        }

        let has_upper = pattern.chars().any(|c| c.is_uppercase());
        let case_insensitive = !has_upper;

        let matcher = if is_literal(pattern) {
            Matcher::Literal(Box::new(Bmh::new(pattern.as_bytes(), case_insensitive)))
        } else {
            let regex_pattern = if case_insensitive {
                format!("(?i){}", pattern)
            } else {
                pattern.to_string()
            };
            let regex = Regex::new(&regex_pattern).map_err(|e| format!("Invalid regex: {}", e))?;
            Matcher::Regex(regex)
        };

        Ok(Search {
            pattern: pattern.to_string(),
            matcher,
        })
    }

    /// Check if a line matches (strips ANSI first).
    pub fn is_match(&self, line: &str) -> bool {
        if !line.contains('\x1b') {
            return self.is_match_raw(line);
        }
        let stripped = strip_ansi(line);
        self.is_match_raw(&stripped)
    }

    fn is_match_raw(&self, text: &str) -> bool {
        match &self.matcher {
            Matcher::Literal(bmh) => bmh.is_match(text.as_bytes()),
            Matcher::Regex(regex) => regex.is_match(text),
        }
    }

    /// Find all match byte ranges in ANSI-stripped text.
    #[cfg(test)]
    pub fn find_matches(&self, line: &str) -> Vec<(usize, usize)> {
        let stripped = strip_ansi(line);
        self.find_matches_stripped(&stripped)
    }

    /// Find all match byte ranges in already-stripped text.
    pub fn find_matches_stripped(&self, stripped: &str) -> Vec<(usize, usize)> {
        match &self.matcher {
            Matcher::Literal(bmh) => bmh.find_all(stripped.as_bytes()),
            Matcher::Regex(regex) => regex
                .find_iter(stripped)
                .map(|m| (m.start(), m.end()))
                .collect(),
        }
    }

    /// Find next matching line from `from_line`, searching forward.
    /// Reads more lines from buffer as needed.
    pub fn find_next_line(
        &self,
        buffer: &mut LineBuffer,
        from_line: usize,
        forward: bool,
    ) -> Option<usize> {
        if forward {
            let mut line_idx = from_line;
            loop {
                match buffer.get_line(line_idx) {
                    Some(line) => {
                        if self.is_match(line) {
                            return Some(line_idx);
                        }
                        line_idx += 1;
                    }
                    None => return None,
                }
            }
        } else {
            if from_line == 0 {
                return None;
            }
            let mut line_idx = from_line - 1;
            loop {
                match buffer.get_line(line_idx) {
                    Some(line) => {
                        if self.is_match(line) {
                            return Some(line_idx);
                        }
                        if line_idx == 0 {
                            return None;
                        }
                        line_idx -= 1;
                    }
                    None => return None,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_buffer(lines: &[&str]) -> LineBuffer {
        LineBuffer::from_lines(lines.iter().map(|s| s.to_string()).collect())
    }

    // --- is_literal ---

    #[test]
    fn literal_detection() {
        assert!(is_literal("hello"));
        assert!(is_literal("0500000"));
        assert!(is_literal("foo bar"));
        assert!(!is_literal("foo.bar"));
        assert!(!is_literal("foo*"));
        assert!(!is_literal("^start"));
        assert!(!is_literal("end$"));
        assert!(!is_literal("[abc]"));
        assert!(!is_literal("a|b"));
        assert!(!is_literal("foo\\d"));
    }

    // --- Bmh ---

    #[test]
    fn bmh_basic_find() {
        let bmh = Bmh::new(b"fox", false);
        assert_eq!(bmh.find_from(b"the quick brown fox", 0), Some(16));
    }

    #[test]
    fn bmh_no_match() {
        let bmh = Bmh::new(b"xyz", false);
        assert_eq!(bmh.find_from(b"hello world", 0), None);
    }

    #[test]
    fn bmh_at_start() {
        let bmh = Bmh::new(b"the", false);
        assert_eq!(bmh.find_from(b"the quick brown fox", 0), Some(0));
    }

    #[test]
    fn bmh_at_end() {
        let bmh = Bmh::new(b"fox", false);
        assert_eq!(bmh.find_from(b"fox", 0), Some(0));
    }

    #[test]
    fn bmh_case_insensitive() {
        let bmh = Bmh::new(b"hello", true);
        assert!(bmh.is_match(b"HELLO WORLD"));
        assert!(bmh.is_match(b"Hello World"));
        assert!(bmh.is_match(b"hello world"));
    }

    #[test]
    fn bmh_case_sensitive() {
        let bmh = Bmh::new(b"Hello", false);
        assert!(bmh.is_match(b"Hello World"));
        assert!(!bmh.is_match(b"hello world"));
        assert!(!bmh.is_match(b"HELLO WORLD"));
    }

    #[test]
    fn bmh_find_all() {
        let bmh = Bmh::new(b"ab", false);
        assert_eq!(
            bmh.find_all(b"ab cd ab ef ab"),
            vec![(0, 2), (6, 8), (12, 14)]
        );
    }

    #[test]
    fn bmh_find_all_no_overlap() {
        let bmh = Bmh::new(b"aa", false);
        assert_eq!(bmh.find_all(b"aaaa"), vec![(0, 2), (2, 4)]);
    }

    #[test]
    fn bmh_single_byte() {
        let bmh = Bmh::new(b"x", false);
        assert_eq!(bmh.find_from(b"abcxdef", 0), Some(3));
    }

    #[test]
    fn bmh_empty_needle() {
        let bmh = Bmh::new(b"", false);
        assert_eq!(bmh.find_from(b"anything", 0), Some(0));
    }

    #[test]
    fn bmh_haystack_shorter_than_needle() {
        let bmh = Bmh::new(b"longpattern", false);
        assert_eq!(bmh.find_from(b"short", 0), None);
    }

    #[test]
    fn bmh_find_from_offset() {
        let bmh = Bmh::new(b"ab", false);
        assert_eq!(bmh.find_from(b"ab ab ab", 1), Some(3));
        assert_eq!(bmh.find_from(b"ab ab ab", 4), Some(6));
    }

    // --- Search::new ---

    #[test]
    fn new_lowercase_is_case_insensitive() {
        let s = Search::new("hello").unwrap();
        assert!(!s.find_matches("Hello World").is_empty());
        assert!(!s.find_matches("HELLO").is_empty());
    }

    #[test]
    fn new_uppercase_is_case_sensitive() {
        let s = Search::new("Hello").unwrap();
        assert!(!s.find_matches("Hello").is_empty());
        assert!(s.find_matches("hello").is_empty());
        assert!(s.find_matches("HELLO").is_empty());
    }

    #[test]
    fn new_empty_pattern_errors() {
        assert!(Search::new("").is_err());
    }

    #[test]
    fn new_invalid_regex_errors() {
        assert!(Search::new("[invalid").is_err());
    }

    #[test]
    fn new_regex_special_chars() {
        let s = Search::new("a.b").unwrap();
        assert!(!s.find_matches("axb").is_empty()); // dot matches any
        assert!(s.find_matches("ab").is_empty()); // but not zero chars
    }

    #[test]
    fn new_literal_pattern_uses_bmh() {
        let s = Search::new("0500000").unwrap();
        assert!(matches!(s.matcher, Matcher::Literal(_)));
    }

    #[test]
    fn new_regex_pattern_uses_regex() {
        let s = Search::new("foo.*bar").unwrap();
        assert!(matches!(s.matcher, Matcher::Regex(_)));
    }

    // --- find_matches ---

    #[test]
    fn matches_plain_text() {
        let s = Search::new("world").unwrap();
        let m = s.find_matches("hello world");
        assert_eq!(m, vec![(6, 11)]);
    }

    #[test]
    fn matches_multiple() {
        let s = Search::new("a").unwrap();
        let m = s.find_matches("banana");
        assert_eq!(m, vec![(1, 2), (3, 4), (5, 6)]);
    }

    #[test]
    fn matches_strips_ansi() {
        let s = Search::new("red").unwrap();
        let m = s.find_matches("\x1b[31mred\x1b[m text");
        assert_eq!(m, vec![(0, 3)]); // positions in stripped text
    }

    #[test]
    fn matches_none() {
        let s = Search::new("xyz").unwrap();
        assert!(s.find_matches("hello world").is_empty());
    }

    // --- find_next_line ---

    #[test]
    fn find_forward_from_start() {
        let s = Search::new("match").unwrap();
        let mut buf = make_buffer(&["no", "match here", "also no"]);
        assert_eq!(s.find_next_line(&mut buf, 0, true), Some(1));
    }

    #[test]
    fn find_forward_from_middle() {
        let s = Search::new("target").unwrap();
        let mut buf = make_buffer(&["target", "no", "target again"]);
        assert_eq!(s.find_next_line(&mut buf, 1, true), Some(2));
    }

    #[test]
    fn find_forward_not_found() {
        let s = Search::new("missing").unwrap();
        let mut buf = make_buffer(&["a", "b", "c"]);
        assert_eq!(s.find_next_line(&mut buf, 0, true), None);
    }

    #[test]
    fn find_backward_from_end() {
        let s = Search::new("match").unwrap();
        let mut buf = make_buffer(&["match here", "no", "also no"]);
        assert_eq!(s.find_next_line(&mut buf, 2, false), Some(0));
    }

    #[test]
    fn find_backward_from_start() {
        let s = Search::new("anything").unwrap();
        let mut buf = make_buffer(&["a", "b"]);
        assert_eq!(s.find_next_line(&mut buf, 0, false), None);
    }

    #[test]
    fn find_backward_stops_at_first_match() {
        let s = Search::new("x").unwrap();
        let mut buf = make_buffer(&["x1", "x2", "x3", "no"]);
        // Searching backward from line 3 should find line 2
        assert_eq!(s.find_next_line(&mut buf, 3, false), Some(2));
    }

    #[test]
    fn find_with_ansi_content() {
        let s = Search::new("error").unwrap();
        let mut buf = make_buffer(&["ok", "\x1b[31merror\x1b[m occurred", "ok"]);
        assert_eq!(s.find_next_line(&mut buf, 0, true), Some(1));
    }

    #[test]
    fn smart_case_mixed() {
        let s = Search::new("error").unwrap(); // lowercase → case insensitive
        let mut buf = make_buffer(&["ERROR found", "no match"]);
        assert_eq!(s.find_next_line(&mut buf, 0, true), Some(0));

        let s2 = Search::new("Error").unwrap(); // has uppercase → case sensitive
        let mut buf2 = make_buffer(&["ERROR found", "Error found"]);
        assert_eq!(s2.find_next_line(&mut buf2, 0, true), Some(1));
    }
}
