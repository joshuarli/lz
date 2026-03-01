use crate::ansi::strip_ansi;
use crate::buffer::LineBuffer;
use regex_lite::Regex;

pub struct Search {
    pub pattern: String,
    regex: Regex,
}

impl Search {
    /// Create a new search. Smart-case: if pattern has uppercase → case-sensitive,
    /// otherwise case-insensitive.
    pub fn new(pattern: &str) -> Result<Self, String> {
        if pattern.is_empty() {
            return Err("Empty pattern".to_string());
        }

        let has_upper = pattern.chars().any(|c| c.is_uppercase());
        let regex_pattern = if has_upper {
            pattern.to_string()
        } else {
            format!("(?i){}", pattern)
        };

        let regex = Regex::new(&regex_pattern).map_err(|e| format!("Invalid regex: {}", e))?;

        Ok(Search {
            pattern: pattern.to_string(),
            regex,
        })
    }

    /// Check if a line matches (strips ANSI first).
    pub fn is_match(&self, line: &str) -> bool {
        if !line.contains('\x1b') {
            return self.regex.is_match(line);
        }
        let stripped = strip_ansi(line);
        self.regex.is_match(&stripped)
    }

    /// Find all match byte ranges in ANSI-stripped text.
    #[cfg(test)]
    pub fn find_matches(&self, line: &str) -> Vec<(usize, usize)> {
        let stripped = strip_ansi(line);
        self.find_matches_stripped(&stripped)
    }

    /// Find all match byte ranges in already-stripped text.
    pub fn find_matches_stripped(&self, stripped: &str) -> Vec<(usize, usize)> {
        self.regex
            .find_iter(stripped)
            .map(|m| (m.start(), m.end()))
            .collect()
    }

    /// Find next matching line from `from_line`, searching forward.
    /// Reads more lines from buffer as needed.
    pub fn find_next_line(&self, buffer: &mut LineBuffer, from_line: usize, forward: bool) -> Option<usize> {
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
        assert!(s.find_matches("ab").is_empty());   // but not zero chars
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
