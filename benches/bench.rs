//! Benchmark harness for lz.
//!
//! Tracks wall time and allocations for user-visible hot paths.
//! Run: `cargo bench --bench bench`.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rustybench::{AllocProfiler, Bencher, black_box};

#[global_allocator]
static ALLOC: AllocProfiler = AllocProfiler::system();

use lz::ansi::{strip_ansi, truncate_to_width, visible_width};
use lz::buffer::LineBuffer;
use lz::search::{CaseSensitivity, Search};

const LINES: usize = 100_000;
const LINE: &str = "the quick brown fox jumps over the lazy dog abcdef padding padding padding";

struct BenchDir(PathBuf);

impl BenchDir {
    fn new(label: &str) -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "lz-bench-{label}-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for BenchDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Write the 100k-line fixture once and return its path; callers pass the
/// bytes to LineBuffer via from_file.
fn make_fixture(label: &str) -> (BenchDir, PathBuf) {
    let fixture = BenchDir::new(label);
    let path = fixture.path().join("data.txt");
    let mut file = fs::File::create(&path).unwrap();
    for i in 1..=LINES {
        writeln!(file, "{i:07} | {LINE}").unwrap();
    }
    (fixture, path)
}

fn line_buffer(path: &Path) -> LineBuffer {
    LineBuffer::from_file(fs::File::open(path).unwrap())
}

#[rustybench::bench]
fn buffer_read_all_100k_lines(bencher: Bencher) {
    let (_fixture, path) = make_fixture("read-all");
    bencher.bench_local(|| {
        let mut buffer = line_buffer(&path);
        buffer.read_all();
        black_box(buffer.len());
    });
}

#[rustybench::bench]
fn search_hit_literal(bencher: Bencher) {
    let (_fixture, path) = make_fixture("search-hit");
    let search = Search::new("00050000", CaseSensitivity::Sensitive).unwrap();
    bencher.bench_local(|| {
        let mut buffer = line_buffer(&path);
        black_box(search.find_next_line(&mut buffer, 0, true));
    });
}

#[rustybench::bench]
fn search_miss_literal(bencher: Bencher) {
    let (_fixture, path) = make_fixture("search-miss");
    let search = Search::new("zznomatch", CaseSensitivity::Sensitive).unwrap();
    bencher.bench_local(|| {
        let mut buffer = line_buffer(&path);
        black_box(search.find_next_line(&mut buffer, 0, true));
    });
}

#[rustybench::bench]
fn search_hit_regex(bencher: Bencher) {
    let (_fixture, path) = make_fixture("search-regex");
    let search = Search::new("0001000.*fox", CaseSensitivity::Sensitive).unwrap();
    bencher.bench_local(|| {
        let mut buffer = line_buffer(&path);
        black_box(search.find_next_line(&mut buffer, 0, true));
    });
}

#[rustybench::bench]
fn search_filter_scan_cached(bencher: Bencher) {
    let (_fixture, path) = make_fixture("filter-scan");
    let search = Search::new("lazy dog", CaseSensitivity::Sensitive).unwrap();
    let mut buffer = line_buffer(&path);
    buffer.read_all();
    bencher.bench_local(|| {
        let mut count = 0;
        for i in 0..buffer.len() {
            if let Some(line) = buffer.get_line(i)
                && search.is_match(line)
            {
                count += 1;
            }
        }
        black_box(count);
    });
}

#[rustybench::bench]
fn strip_ansi_colored_line(bencher: Bencher) {
    let line = "\x1b[32m0001 | \x1b[1mthe quick brown fox\x1b[0m jumps \x1b[31mover\x1b[0m the lazy dog";
    bencher.bench_local(|| black_box(strip_ansi(black_box(line))));
}

#[rustybench::bench]
fn visible_width_unicode_line(bencher: Bencher) {
    let line = "\x1b[33m0001 | 日本語テキスト mixed with ascii and some more padding here\x1b[0m";
    bencher.bench_local(|| black_box(visible_width(black_box(line))));
}

#[rustybench::bench]
fn truncate_to_width_colored_line(bencher: Bencher) {
    let line = "\x1b[32m0001 | \x1b[1mthe quick brown fox jumps over the lazy dog abcdef padding padding padding\x1b[0m";
    bencher.bench_local(|| black_box(truncate_to_width(black_box(line), 0, 80, false)));
}

fn main() {
    rustybench::main();
}
