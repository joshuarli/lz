use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::ffi::OsString;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

const DEFAULT_HISTORY_SIZE: usize = 100;
const HISTORY_NAME: &str = ".lesshst";
const HEADER: &[u8] = b".less-history-file:";

#[derive(Clone, Copy, PartialEq)]
enum Section { Search, Shell }

struct Record { section: Section, bytes: Vec<u8> }
struct Entry { bytes: Vec<u8>, text: String, modified: bool }

#[derive(Default)]
pub struct SearchHistory {
    entries: Vec<Entry>,
    cursor: usize,
    draft: String,
    dirty: bool,
    position_changed: bool,
}

enum Location { Explicit(PathBuf), Default { home: Option<OsString>, state: Option<OsString>, data: Option<OsString> } }

pub struct HistoryFile { location: Location, limit: usize }

impl HistoryFile {
    pub fn from_environment() -> Option<Self> {
        let setting = |name| std::env::var_os(name).filter(|value| !value.is_empty());
        let location = if let Some(path) = setting("LESSHISTFILE") {
            if path == "-" || path == "/dev/null" { return None; }
            Location::Explicit(path.into())
        } else { Location::Default { home: setting("HOME"), state: setting("XDG_STATE_HOME"), data: setting("XDG_DATA_HOME") } };
        let limit = setting("LESSHISTSIZE").and_then(|value| value.to_str()?.parse::<usize>().ok()).filter(|value| *value > 0).unwrap_or(DEFAULT_HISTORY_SIZE);
        Some(Self { location, limit })
    }

    fn candidates(&self) -> Vec<PathBuf> {
        match &self.location {
            Location::Explicit(path) => vec![path.clone()],
            Location::Default { home, state, data } => {
                let mut paths = Vec::new();
                if let Some(state) = state { paths.push(Path::new(state).join(&HISTORY_NAME[1..])); }
                if let Some(home) = home {
                    let state = Path::new(home).join(".local/state");
                    if File::open(&state).is_ok() { paths.push(state.join(&HISTORY_NAME[1..])); }
                }
                if let Some(data) = data { paths.push(Path::new(data).join(&HISTORY_NAME[1..])); }
                if let Some(home) = home { paths.push(Path::new(home).join(HISTORY_NAME)); }
                paths
            }
        }
    }

    pub fn load(&self) -> SearchHistory {
        let paths = self.candidates();
        paths.iter().find(|path| File::open(path).is_ok()).map(|path| SearchHistory::load(path)).unwrap_or_default()
    }

    pub fn save(&self, history: &mut SearchHistory) {
        if !history.dirty && !history.position_changed { return; }
        let paths = self.candidates();
        if let Some(path) = paths.iter().find(|path| File::open(path).is_ok()).or_else(|| paths.first()) {
            let _ = history.save_to(path, self.limit);
        }
    }
}

fn read_record(reader: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    let mut bytes = Vec::new();
    if reader.take(2047).read_until(b'\n', &mut bytes)? == 0 { return Ok(None); }
    if let Some(end) = bytes.iter().position(|byte| matches!(byte, 0 | b'\r' | b'\n')) { bytes.truncate(end); }
    Ok(Some(bytes))
}

fn read_history(path: &Path) -> io::Result<Vec<Record>> {
    let file = match File::open(path) { Ok(file) => file, Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()), Err(error) => return Err(error) };
    let mut reader = BufReader::new(file);
    if !read_record(&mut reader)?.is_some_and(|line| line.starts_with(HEADER)) { return Ok(Vec::new()); }
    let mut section = None;
    let mut records = Vec::new();
    while let Some(line) = read_record(&mut reader)? {
        match line.as_slice() {
            b".search" => section = Some(Section::Search),
            b".shell" => section = Some(Section::Shell),
            b".mark" => section = None,
            _ if line.first() == Some(&b'"') => if let Some(section) = section { records.push(Record { section, bytes: line[1..].to_vec() }); },
            _ => {}
        }
    }
    Ok(records)
}

struct HistoryTemp { file: File, path: PathBuf }

impl HistoryTemp {
    fn new(destination: &Path) -> io::Result<Self> {
        let mut bytes = destination.as_os_str().as_bytes().to_vec();
        let last = bytes.last_mut().ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty history path"))?;
        *last = if *last == b'Q' { b'Z' } else { b'Q' };
        let path = PathBuf::from(OsString::from_vec(bytes));
        let file = OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path)?;
        let temporary = Self { file, path };
        temporary.file.set_permissions(fs::Permissions::from_mode(0o600))?;
        Ok(temporary)
    }
}

impl Drop for HistoryTemp { fn drop(&mut self) { let _ = fs::remove_file(&self.path); } }

fn write_entry(output: &mut File, bytes: &[u8]) -> io::Result<()> {
    output.write_all(b"\"")?;
    output.write_all(bytes)?;
    output.write_all(b"\n")
}

impl SearchHistory {
    /// Load the less history format; a headerless lz history file is not imported.
    pub fn load(path: &Path) -> Self {
        let mut history = Self::default();
        for record in read_history(path).unwrap_or_default() {
            if record.section == Section::Search { history.add(record.bytes, false); }
        }
        history.cursor = history.entries.len();
        history
    }

    pub fn save(&mut self, path: &Path) { let _ = self.save_to(path, DEFAULT_HISTORY_SIZE); }

    fn save_to(&mut self, path: &Path, limit: usize) -> io::Result<()> {
        if !self.dirty && !self.position_changed { return Ok(()); }
        let records = read_history(path)?;
        let mut temporary = HistoryTemp::new(path)?;
        temporary.file.write_all(HEADER)?;
        temporary.file.write_all(b"\n")?;
        let mut skip_search = self.entries.len().saturating_sub(limit);
        let mut shell_count = 0usize;
        let mut previous_shell: Option<&[u8]> = None;
        for record in records.iter().filter(|record| record.section == Section::Shell && !record.bytes.is_empty()) {
            if previous_shell != Some(record.bytes.as_slice()) { shell_count += 1; }
            previous_shell = Some(&record.bytes);
        }
        let mut skip_shell = shell_count.saturating_sub(limit);
        let mut current_section = None;
        let mut modified = self.dirty;
        for record in records {
            let skip = if record.section == Section::Search { &mut skip_search } else { &mut skip_shell };
            if *skip > 0 { *skip -= 1; continue; }
            if current_section != Some(record.section) {
                if current_section == Some(Section::Search) && modified {
                    for entry in self.entries.iter().filter(|entry| entry.modified) { write_entry(&mut temporary.file, &entry.bytes)?; }
                    modified = false;
                }
                temporary.file.write_all(if record.section == Section::Search { b".search\n" } else { b".shell\n" })?;
                current_section = Some(record.section);
            }
            write_entry(&mut temporary.file, &record.bytes)?;
        }
        if modified {
            temporary.file.write_all(b".search\n")?;
            for entry in self.entries.iter().filter(|entry| entry.modified) { write_entry(&mut temporary.file, &entry.bytes)?; }
        }
        temporary.file.flush()?;
        fs::rename(&temporary.path, path)?;
        for entry in &mut self.entries { entry.modified = false; }
        self.dirty = false;
        self.position_changed = false;
        Ok(())
    }

    fn add(&mut self, bytes: Vec<u8>, modified: bool) {
        if bytes.is_empty() || self.entries.last().is_some_and(|entry| entry.bytes == bytes) { return; }
        let text = String::from_utf8_lossy(&bytes).into_owned();
        self.entries.push(Entry { bytes, text, modified });
    }

    pub(crate) fn note_position(&mut self) { self.position_changed = true; }

    pub fn push(&mut self, pattern: &str) {
        self.dirty = true;
        if pattern.is_empty() { return; }
        self.add(pattern.as_bytes().to_vec(), true);
        self.cursor = self.entries.len();
    }

    pub fn older(&mut self, current_input: &str) -> Option<&str> {
        if self.entries.is_empty() { return None; }
        if self.cursor == self.entries.len() { self.draft = current_input.to_string(); }
        if self.cursor == 0 { return Some(&self.entries[0].text); }
        self.cursor -= 1;
        Some(&self.entries[self.cursor].text)
    }

    pub(crate) fn at_oldest(&self) -> bool { !self.entries.is_empty() && self.cursor == 0 }

    pub fn newer(&mut self) -> Option<&str> {
        if self.cursor >= self.entries.len() { return None; }
        self.cursor += 1;
        if self.cursor >= self.entries.len() { return None; }
        Some(&self.entries[self.cursor].text)
    }

    pub fn reset_cursor(&mut self) { self.cursor = self.entries.len(); self.draft.clear(); }
    pub fn draft(&self) -> &str { &self.draft }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> tempfile::TempDir {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join(".work/tmp");
        fs::create_dir_all(&root).unwrap();
        tempfile::tempdir_in(root).unwrap()
    }

    fn empty_history() -> SearchHistory {
        SearchHistory::default()
    }

    fn history_with(entries: &[&str]) -> SearchHistory {
        let mut history = SearchHistory::default();
        for entry in entries { history.add(entry.as_bytes().to_vec(), false); }
        history.cursor = history.entries.len();
        history
    }

    fn patterns(history: &SearchHistory) -> Vec<&str> { history.entries.iter().map(|entry| entry.text.as_str()).collect() }

    #[test]
    fn push_appends() {
        let mut h = empty_history();
        h.push("foo");
        assert_eq!(patterns(&h), vec!["foo"]);
    }

    #[test]
    fn push_preserves_nonconsecutive_duplicates() {
        let mut h = history_with(&["foo", "bar"]);
        h.push("foo");
        assert_eq!(patterns(&h), vec!["foo", "bar", "foo"]);
    }

    #[test]
    fn new_session_entries_are_not_discarded_before_save() {
        let mut h = empty_history();
        for i in 0..105 {
            h.push(&format!("entry{}", i));
        }
        assert_eq!(h.entries.len(), 105);
        assert_eq!(h.entries[0].text, "entry0");
    }

    #[test]
    fn empty_input_creates_no_entry() {
        let mut h = empty_history();
        h.push("");
        assert!(h.entries.is_empty());
    }

    #[test]
    fn older_cycles_backward() {
        let mut h = history_with(&["a", "b", "c"]);
        assert_eq!(h.older("draft"), Some("c"));
        assert_eq!(h.older("draft"), Some("b"));
        assert_eq!(h.older("draft"), Some("a"));
        // stays at oldest
        assert_eq!(h.older("draft"), Some("a"));
    }

    #[test]
    fn older_saves_draft() {
        let mut h = history_with(&["a"]);
        h.older("my draft");
        assert_eq!(h.draft(), "my draft");
    }

    #[test]
    fn older_empty_returns_none() {
        let mut h = empty_history();
        assert_eq!(h.older("x"), None);
    }

    #[test]
    fn newer_cycles_forward() {
        let mut h = history_with(&["a", "b", "c"]);
        h.older(""); // c, cursor=2
        h.older(""); // b, cursor=1
        assert_eq!(h.newer(), Some("c"));
        // past end returns None
        assert_eq!(h.newer(), None);
    }

    #[test]
    fn newer_at_end_returns_none() {
        let mut h = history_with(&["a"]);
        assert_eq!(h.newer(), None);
    }

    #[test]
    fn reset_cursor_goes_to_end() {
        let mut h = history_with(&["a", "b"]);
        h.older("draft");
        h.reset_cursor();
        assert_eq!(h.cursor, h.entries.len());
        assert_eq!(h.draft(), "");
    }

    #[test]
    fn load_nonexistent_returns_empty() {
        let dir = fixture();
        let h = SearchHistory::load(&dir.path().join("missing"));
        assert!(h.entries.is_empty());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = fixture();
        let path = dir.path().join("history");

        let mut h = empty_history();
        h.push("first");
        h.push("second");
        h.save(&path);

        let h2 = SearchHistory::load(&path);
        assert_eq!(patterns(&h2), vec!["first", "second"]);

    }

    #[test]
    fn save_merges_latest_disk_records_once() {
        let dir = fixture();
        let path = dir.path().join("history");
        fs::write(&path, b".less-history-file:\n.search\n\"old\n.shell\n\"kept\n").unwrap();
        let mut history = SearchHistory::load(&path);
        history.push("mine");
        fs::write(&path, b".less-history-file:\n.search\n\"old\n.shell\n\"kept\n.search\n\"external\n.shell\n\"later\n").unwrap();
        history.save(&path);
        let expected = b".less-history-file:\n.search\n\"old\n\"mine\n.shell\n\"kept\n.search\n\"external\n.shell\n\"later\n";
        assert_eq!(fs::read(&path).unwrap(), expected);
        history.save(&path);
        assert_eq!(fs::read(&path).unwrap(), expected);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn persistence_preserves_raw_bytes_and_rejects_headerless_input() {
        let dir = fixture();
        let path = dir.path().join("history");
        fs::write(&path, b"legacy\nlines\n").unwrap();
        assert!(SearchHistory::load(&path).entries.is_empty());
        assert_eq!(fs::read(&path).unwrap(), b"legacy\nlines\n");
        fs::write(&path, b".less-history-file:\n.search\n\"old\n.shell\n\"\xff\n").unwrap();
        let mut history = SearchHistory::load(&path);
        history.push("mine");
        history.save(&path);
        assert_eq!(fs::read(&path).unwrap(), b".less-history-file:\n.search\n\"old\n\"mine\n.shell\n\"\xff\n");
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn load_skips_empty_lines() {
        let dir = fixture();
        let path = dir.path().join("history");
        fs::write(&path, ".less-history-file:\n.search\n\"a\n\n\"b\n\"\n").unwrap();

        let h = SearchHistory::load(&path);
        assert_eq!(patterns(&h), vec!["a", "b"]);

    }
}
