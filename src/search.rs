use crate::ansi::strip_ansi;
use crate::buffer::LineBuffer;
use regex_automata::{Anchored, Input, MatchKind, nfa::thompson::pikevm::{Cache, PikeVM}};
use std::sync::Mutex;

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

fn translate_posix_escapes(pattern: &str) -> String {
    let bytes = pattern.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    let mut in_class = false;
    let mut class_first = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if !in_class && byte == b'\\' && let Some(&next) = bytes.get(index + 1) {
            if cfg!(target_os = "linux") && next == b'b' {
                output.extend_from_slice(br"(?:\A|\b|\z)");
                index += 2;
                continue;
            }
            let literal = next.is_ascii_digit() || (cfg!(target_os = "macos") && (next.is_ascii_alphabetic() || matches!(next, b'<' | b'>')));
            if !literal { output.push(byte); }
            output.push(next);
            index += 2;
            continue;
        }
        if in_class && byte == b'[' && bytes.get(index + 1).is_some_and(|next| matches!(next, b':' | b'.' | b'=')) {
            let delimiter = bytes[index + 1];
            let start = index;
            index += 2;
            while index + 1 < bytes.len() && !(bytes[index] == delimiter && bytes[index + 1] == b']') { index += 1; }
            index = (index + 2).min(bytes.len());
            output.extend_from_slice(&bytes[start..index]);
            class_first = false;
            continue;
        }
        if in_class && matches!(byte, b'&' | b'~') {
            output.extend_from_slice(if byte == b'&' { b"\\x26" } else { b"\\x7e" });
            class_first = false;
            index += 1;
            continue;
        }
        if in_class && byte == b'\\' { output.push(byte); }
        output.push(byte);
        if !in_class && byte == b'[' { in_class = true; class_first = true; }
        else if in_class {
            if byte == b']' && !class_first { in_class = false; }
            else if byte != b'^' || !class_first { class_first = false; }
        }
        index += 1;
    }
    String::from_utf8(output).expect("preserved pattern UTF-8")
}

enum Matcher {
    Literal(Box<Bmh>),
    Regex(Box<RegexMatcher>),
}

struct RegexMatcher {
    first: PikeVM,
    longest: PikeVM,
    caches: Mutex<RegexCaches>,
}

struct RegexCaches { first: Cache, longest: Cache }

impl RegexMatcher {
    fn new(pattern: &str, case_insensitive: bool) -> Result<Self, String> {
        let first = PikeVM::builder()
            .syntax(regex_automata::util::syntax::Config::new().case_insensitive(case_insensitive))
            .build(&translate_posix_escapes(pattern)).map_err(|error| format!("Invalid regex: {error}"))?;
        let longest = PikeVM::builder().configure(PikeVM::config().match_kind(MatchKind::All))
            .build_from_nfa(first.get_nfa().clone()).map_err(|error| format!("Invalid regex: {error}"))?;
        let caches = Mutex::new(RegexCaches { first: first.create_cache(), longest: longest.create_cache() });
        Ok(Self { first, longest, caches })
    }

    fn is_match(&self, text: &str) -> bool {
        let mut caches = self.caches.lock().unwrap_or_else(|error| error.into_inner());
        self.first.is_match(&mut caches.first, text)
    }

    fn find_all(&self, text: &str) -> Vec<(usize, usize)> {
        let mut caches = self.caches.lock().unwrap_or_else(|error| error.into_inner());
        let mut matches = Vec::new();
        let mut from = 0;
        let mut previous_end = None;
        while let Some(first) = self.first.find(&mut caches.first, Input::new(text).range(from..)) {
            // POSIX selects the longest match at the earliest start, independent of alternative order.
            let found = self.longest.find(&mut caches.longest, Input::new(text).range(first.start()..).anchored(Anchored::Yes)).unwrap();
            let (start, end) = (found.start(), found.end());
            if start != end || previous_end != Some(end) {
                matches.push((start, end));
                previous_end = Some(end);
            }
            if start == end {
                let Some(character) = text[end..].chars().next() else { break; };
                from = end + character.len_utf8();
            } else { from = end; }
        }
        matches
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CaseSensitivity {
    #[default]
    Sensitive,
    Smart,
    Insensitive,
}

/// Select where searches start relative to the displayed viewport.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum SearchStart { #[default] Default, AfterScreen, AfterTarget }

pub struct Search {
    pub pattern: String,
    matcher: Matcher,
}

impl Search {
    /// Compile a pattern using the case policy selected by the caller.
    pub fn new(pattern: &str, case: CaseSensitivity) -> Result<Self, String> {
        if pattern.is_empty() {
            return Err("Empty pattern".to_string());
        }

        let case_insensitive = match case {
            CaseSensitivity::Sensitive => false,
            CaseSensitivity::Smart => !pattern.chars().any(|c| c.is_uppercase()),
            CaseSensitivity::Insensitive => true,
        };

        let matcher = if is_literal(pattern) {
            Matcher::Literal(Box::new(Bmh::new(pattern.as_bytes(), case_insensitive)))
        } else {
            Matcher::Regex(Box::new(RegexMatcher::new(pattern, case_insensitive)?))
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
            Matcher::Regex(regex) => regex.find_all(stripped),
        }
    }

    pub(crate) fn find_next_nonmatching_line(&self, buffer: &mut LineBuffer, from_line: usize, forward: bool) -> Option<usize> {
        if forward {
            let mut line = from_line;
            loop {
                if !self.is_match(buffer.get_line(line)?) { return Some(line); }
                line = line.checked_add(1)?;
            }
        } else {
            let mut line = from_line;
            while let Some(previous) = line.checked_sub(1) {
                if !self.is_match(buffer.get_line(previous)?) { return Some(previous); }
                line = previous;
            }
            None
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
                let line = buffer.get_line(line_idx)?;
                if self.is_match(line) {
                    return Some(line_idx);
                }
                line_idx += 1;
            }
        } else {
            if from_line == 0 {
                return None;
            }
            let mut line_idx = from_line - 1;
            loop {
                let line = buffer.get_line(line_idx)?;
                if self.is_match(line) {
                    return Some(line_idx);
                }
                if line_idx == 0 {
                    return None;
                }
                line_idx -= 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posix_bracket_operators_are_literal_members() {
        let ampersand = Search::new("[a&&b]", CaseSensitivity::Sensitive).unwrap();
        assert!(ampersand.is_match("&"));
        assert!(!ampersand.is_match("c"));
        let tilde = Search::new("[a~~b]", CaseSensitivity::Sensitive).unwrap();
        assert!(tilde.is_match("~"));
        assert!(!tilde.is_match("c"));
    }

    #[test]
    fn posix_bracket_backslashes_are_literal_members() {
        let search = Search::new(r"[\d]", CaseSensitivity::Sensitive).unwrap();
        assert!(search.is_match(r"\"));
        assert!(search.is_match("d"));
        assert!(!search.is_match("1"));
    }

    #[test]
    fn escaped_digits_are_literals_in_posix_search() {
        let search = Search::new(r"(a)\1", CaseSensitivity::Sensitive).unwrap();
        assert!(search.is_match("a1"));
        assert!(!search.is_match("aa"));
    }

    #[test]
    fn escaped_boundary_matches_platform_line_edges() {
        let search = Search::new(r"\b", CaseSensitivity::Sensitive).unwrap();
        if cfg!(target_os = "macos") {
            assert!(search.find_matches_stripped("!!!").is_empty());
        } else if cfg!(target_os = "linux") {
            assert_eq!(search.find_matches_stripped("!!!"), vec![(0, 0), (3, 3)]);
        }
    }

    #[test]
    fn escaped_angles_use_platform_regex_semantics() {
        let left = Search::new(r"\<", CaseSensitivity::Sensitive).unwrap();
        let right = Search::new(r"\>", CaseSensitivity::Sensitive).unwrap();
        if cfg!(target_os = "macos") {
            assert_eq!(left.find_matches_stripped("<>a"), vec![(0, 1)]);
            assert_eq!(right.find_matches_stripped("<>a"), vec![(1, 2)]);
        } else if cfg!(target_os = "linux") {
            assert_eq!(left.find_matches_stripped("<>a"), vec![(2, 2)]);
            assert_eq!(right.find_matches_stripped("<>a"), vec![(3, 3)]);
        }
    }

    #[test]
    fn escaped_letter_uses_platform_regex_semantics() {
        let search = Search::new(r"\d", CaseSensitivity::Sensitive).unwrap();
        if cfg!(target_os = "macos") {
            assert!(search.is_match("d"));
            assert!(!search.is_match("1"));
        } else if cfg!(target_os = "linux") {
            assert!(search.is_match("1"));
            assert!(!search.is_match("d"));
        }
    }

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
        let s = Search::new("hello", CaseSensitivity::Smart).unwrap();
        assert!(!s.find_matches("Hello World").is_empty());
        assert!(!s.find_matches("HELLO").is_empty());
    }

    #[test]
    fn new_uppercase_is_case_sensitive() {
        let s = Search::new("Hello", CaseSensitivity::Smart).unwrap();
        assert!(!s.find_matches("Hello").is_empty());
        assert!(s.find_matches("hello").is_empty());
        assert!(s.find_matches("HELLO").is_empty());
    }

    #[test]
    fn new_empty_pattern_errors() {
        assert!(Search::new("", CaseSensitivity::Smart).is_err());
    }

    #[test]
    fn new_invalid_regex_errors() {
        assert!(Search::new("[invalid", CaseSensitivity::Smart).is_err());
    }

    #[test]
    fn new_regex_special_chars() {
        let s = Search::new("a.b", CaseSensitivity::Smart).unwrap();
        assert!(!s.find_matches("axb").is_empty()); // dot matches any
        assert!(s.find_matches("ab").is_empty()); // but not zero chars
    }

    #[test]
    fn new_literal_pattern_uses_bmh() {
        let s = Search::new("0500000", CaseSensitivity::Smart).unwrap();
        assert!(matches!(s.matcher, Matcher::Literal(_)));
    }

    #[test]
    fn new_regex_pattern_uses_regex() {
        let s = Search::new("foo.*bar", CaseSensitivity::Smart).unwrap();
        assert!(matches!(s.matcher, Matcher::Regex(_)));
    }

    // --- find_matches ---

    #[test]
    fn regex_selects_longest_at_leftmost_position() {
        for pattern in ["a|ab", "ab|a", "(a|ab)+"] {
            let search = Search::new(pattern, CaseSensitivity::Sensitive).unwrap();
            assert_eq!(search.find_matches("xab ab"), vec![(1, 3), (4, 6)]);
        }
        let search = Search::new("b|a.*z", CaseSensitivity::Sensitive).unwrap();
        assert_eq!(search.find_matches("abz b"), vec![(0, 3), (4, 5)]);
    }

    #[test]
    fn regex_keeps_anchors_context_and_advances_empty_unicode_matches() {
        let search = Search::new("^a|ab", CaseSensitivity::Sensitive).unwrap();
        assert_eq!(search.find_matches("aa ab"), vec![(0, 1), (3, 5)]);
        let search = Search::new("x*", CaseSensitivity::Sensitive).unwrap();
        assert_eq!(search.find_matches("éx"), vec![(0, 0), (2, 3)]);
    }

    #[test]
    fn matches_plain_text() {
        let s = Search::new("world", CaseSensitivity::Smart).unwrap();
        let m = s.find_matches("hello world");
        assert_eq!(m, vec![(6, 11)]);
    }

    #[test]
    fn matches_multiple() {
        let s = Search::new("a", CaseSensitivity::Smart).unwrap();
        let m = s.find_matches("banana");
        assert_eq!(m, vec![(1, 2), (3, 4), (5, 6)]);
    }

    #[test]
    fn matches_strips_ansi() {
        let s = Search::new("red", CaseSensitivity::Smart).unwrap();
        let m = s.find_matches("\x1b[31mred\x1b[m text");
        assert_eq!(m, vec![(0, 3)]); // positions in stripped text
    }

    #[test]
    fn matches_none() {
        let s = Search::new("xyz", CaseSensitivity::Smart).unwrap();
        assert!(s.find_matches("hello world").is_empty());
    }

    // --- find_next_line ---

    #[test]
    fn find_forward_from_start() {
        let s = Search::new("match", CaseSensitivity::Smart).unwrap();
        let mut buf = make_buffer(&["no", "match here", "also no"]);
        assert_eq!(s.find_next_line(&mut buf, 0, true), Some(1));
    }

    #[test]
    fn find_forward_from_middle() {
        let s = Search::new("target", CaseSensitivity::Smart).unwrap();
        let mut buf = make_buffer(&["target", "no", "target again"]);
        assert_eq!(s.find_next_line(&mut buf, 1, true), Some(2));
    }

    #[test]
    fn find_forward_not_found() {
        let s = Search::new("missing", CaseSensitivity::Smart).unwrap();
        let mut buf = make_buffer(&["a", "b", "c"]);
        assert_eq!(s.find_next_line(&mut buf, 0, true), None);
    }

    #[test]
    fn find_backward_from_end() {
        let s = Search::new("match", CaseSensitivity::Smart).unwrap();
        let mut buf = make_buffer(&["match here", "no", "also no"]);
        assert_eq!(s.find_next_line(&mut buf, 2, false), Some(0));
    }

    #[test]
    fn find_backward_from_start() {
        let s = Search::new("anything", CaseSensitivity::Smart).unwrap();
        let mut buf = make_buffer(&["a", "b"]);
        assert_eq!(s.find_next_line(&mut buf, 0, false), None);
    }

    #[test]
    fn find_backward_stops_at_first_match() {
        let s = Search::new("x", CaseSensitivity::Smart).unwrap();
        let mut buf = make_buffer(&["x1", "x2", "x3", "no"]);
        // Searching backward from line 3 should find line 2
        assert_eq!(s.find_next_line(&mut buf, 3, false), Some(2));
    }

    #[test]
    fn find_with_ansi_content() {
        let s = Search::new("error", CaseSensitivity::Smart).unwrap();
        let mut buf = make_buffer(&["ok", "\x1b[31merror\x1b[m occurred", "ok"]);
        assert_eq!(s.find_next_line(&mut buf, 0, true), Some(1));
    }

    #[test]
    fn smart_case_mixed() {
        let s = Search::new("error", CaseSensitivity::Smart).unwrap();
        let mut buf = make_buffer(&["ERROR found", "no match"]);
        assert_eq!(s.find_next_line(&mut buf, 0, true), Some(0));

        let s2 = Search::new("Error", CaseSensitivity::Smart).unwrap();
        let mut buf2 = make_buffer(&["ERROR found", "Error found"]);
        assert_eq!(s2.find_next_line(&mut buf2, 0, true), Some(1));
    }
}
