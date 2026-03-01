use std::fs;
use std::path::Path;

const MAX_ENTRIES: usize = 100;

pub struct SearchHistory {
    entries: Vec<String>,
    cursor: usize,
    draft: String,
}

impl SearchHistory {
    pub fn load(path: &Path) -> Self {
        let entries = fs::read_to_string(path)
            .ok()
            .map(|text| {
                text.lines()
                    .filter(|l| !l.is_empty())
                    .map(|l| l.to_string())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let cursor = entries.len();
        SearchHistory {
            entries,
            cursor,
            draft: String::new(),
        }
    }

    pub fn save(&self, path: &Path) {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(path, self.entries.join("\n") + if self.entries.is_empty() { "" } else { "\n" });
    }

    pub fn push(&mut self, pattern: &str) {
        if pattern.is_empty() {
            return;
        }
        self.entries.retain(|e| e != pattern);
        self.entries.push(pattern.to_string());
        if self.entries.len() > MAX_ENTRIES {
            self.entries.drain(..self.entries.len() - MAX_ENTRIES);
        }
        self.cursor = self.entries.len();
    }

    pub fn prev(&mut self, current_input: &str) -> Option<&str> {
        if self.entries.is_empty() {
            return None;
        }
        if self.cursor == self.entries.len() {
            self.draft = current_input.to_string();
        }
        if self.cursor == 0 {
            return Some(&self.entries[0]);
        }
        self.cursor -= 1;
        Some(&self.entries[self.cursor])
    }

    pub fn next(&mut self) -> Option<&str> {
        if self.cursor >= self.entries.len() {
            return None;
        }
        self.cursor += 1;
        if self.cursor >= self.entries.len() {
            return None;
        }
        Some(&self.entries[self.cursor])
    }

    pub fn reset_cursor(&mut self) {
        self.cursor = self.entries.len();
        self.draft.clear();
    }

    pub fn draft(&self) -> &str {
        &self.draft
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn empty_history() -> SearchHistory {
        SearchHistory {
            entries: vec![],
            cursor: 0,
            draft: String::new(),
        }
    }

    fn history_with(entries: &[&str]) -> SearchHistory {
        let entries: Vec<String> = entries.iter().map(|s| s.to_string()).collect();
        let cursor = entries.len();
        SearchHistory {
            entries,
            cursor,
            draft: String::new(),
        }
    }

    #[test]
    fn push_appends() {
        let mut h = empty_history();
        h.push("foo");
        assert_eq!(h.entries, vec!["foo"]);
    }

    #[test]
    fn push_deduplicates() {
        let mut h = history_with(&["foo", "bar"]);
        h.push("foo");
        assert_eq!(h.entries, vec!["bar", "foo"]);
    }

    #[test]
    fn push_caps_at_max() {
        let mut h = empty_history();
        for i in 0..105 {
            h.push(&format!("entry{}", i));
        }
        assert_eq!(h.entries.len(), MAX_ENTRIES);
        assert_eq!(h.entries[0], "entry5");
    }

    #[test]
    fn push_empty_is_noop() {
        let mut h = empty_history();
        h.push("");
        assert!(h.entries.is_empty());
    }

    #[test]
    fn prev_cycles_backward() {
        let mut h = history_with(&["a", "b", "c"]);
        assert_eq!(h.prev("draft"), Some("c"));
        assert_eq!(h.prev("draft"), Some("b"));
        assert_eq!(h.prev("draft"), Some("a"));
        // stays at oldest
        assert_eq!(h.prev("draft"), Some("a"));
    }

    #[test]
    fn prev_saves_draft() {
        let mut h = history_with(&["a"]);
        h.prev("my draft");
        assert_eq!(h.draft(), "my draft");
    }

    #[test]
    fn prev_empty_returns_none() {
        let mut h = empty_history();
        assert_eq!(h.prev("x"), None);
    }

    #[test]
    fn next_cycles_forward() {
        let mut h = history_with(&["a", "b", "c"]);
        h.prev(""); // c, cursor=2
        h.prev(""); // b, cursor=1
        assert_eq!(h.next(), Some("c"));
        // past end returns None
        assert_eq!(h.next(), None);
    }

    #[test]
    fn next_at_end_returns_none() {
        let mut h = history_with(&["a"]);
        assert_eq!(h.next(), None);
    }

    #[test]
    fn reset_cursor_goes_to_end() {
        let mut h = history_with(&["a", "b"]);
        h.prev("draft");
        h.reset_cursor();
        assert_eq!(h.cursor, h.entries.len());
        assert_eq!(h.draft(), "");
    }

    #[test]
    fn load_nonexistent_returns_empty() {
        let h = SearchHistory::load(Path::new("/tmp/lz-test-nonexistent-12345"));
        assert!(h.entries.is_empty());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = PathBuf::from("/tmp/lz-test-history-roundtrip");
        let path = dir.join("history");
        let _ = fs::remove_dir_all(&dir);

        let mut h = empty_history();
        h.push("first");
        h.push("second");
        h.save(&path);

        let h2 = SearchHistory::load(&path);
        assert_eq!(h2.entries, vec!["first", "second"]);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_skips_empty_lines() {
        let dir = PathBuf::from("/tmp/lz-test-history-empty-lines");
        let path = dir.join("history");
        let _ = fs::create_dir_all(&dir);
        fs::write(&path, "a\n\nb\n\n").unwrap();

        let h = SearchHistory::load(&path);
        assert_eq!(h.entries, vec!["a", "b"]);

        let _ = fs::remove_dir_all(&dir);
    }
}
