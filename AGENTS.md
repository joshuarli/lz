# AGENTS.md — lz Codebase Guide

This document is written for AI agents working on the lz codebase. It covers architecture, module responsibilities, design decisions, and how to build and test.

---

## What lz Is

lz is a minimal, fast pager (like `less`) written in Rust. It targets the common 90% of `less` usage without the full complexity. It is Unix-only and has exactly two dependencies: `regex-lite` and `unicode-width`. Everything else — terminal control, key input, signal handling — is done via direct libc FFI or standard library primitives.

The design philosophy is small binary, simple code, zero unnecessary dependencies.

---

## Source Layout

All source lives in `src/`:

```
src/
  main.rs       CLI entry point, arg parsing, panic hook
  pager.rs      Main event loop and rendering
  buffer.rs     Lazy streaming line buffer
  input.rs      Key reading and escape sequence parsing
  terminal.rs   Raw termios, alt screen, signals, output helpers
  ansi.rs       ANSI stripping, visible width, color-preserving truncation
  search.rs     Regex/BMH search with smart-case
  history.rs    Persistent search history (Up/Down in search prompt)

bench/
  run.sh        Benchmark script (lz vs less, uses expect + hyperfine)
```

---

## Module Dependency Graph

```
main.rs
  ├── history.rs
  └── pager.rs
        ├── buffer.rs
        ├── history.rs
        ├── input.rs
        ├── search.rs
        │     ├── ansi.rs
        │     └── buffer.rs
        ├── terminal.rs
        ├── ansi.rs
        └── unicode-width (crate)

ansi.rs
  └── unicode-width (crate)
```

`main.rs` is the only binary entry point. `pager.rs` owns the event loop and pulls in everything else. `search.rs` and `ansi.rs` are utility modules used by multiple callers. `terminal.rs` and `input.rs` are low-level I/O modules with no upward dependencies. `history.rs` is a standalone utility used by both `main.rs` (load/save) and `pager.rs` (cycling).

---

## Module Responsibilities

### `main.rs`

Entry point and CLI argument parsing.

- Hand-rolled argument parsing (no clap). Handles `--follow`, `--force`, `-r`/`--raw`, `-h`/`--help`, `-V`/`--version`, `--`, `-` (explicit stdin), and a positional file argument.
- Installs a panic hook that restores the terminal (exits alt screen, disables raw mode) before printing the panic message, so a crash does not leave the terminal broken.
- Calls into `pager::run()` with a constructed config.

Key types: none (thin glue layer).

### `terminal.rs`

All raw terminal interaction via libc FFI.

- Sets and restores raw termios mode by calling `tcgetattr` / `tcsetattr` directly through libc. No crossterm, no termion.
- Manages the alternate screen (`\x1b[?1049h` / `\x1b[?1049l`).
- Cursor control: hide/show, absolute positioning.
- Signal handling: `SIGWINCH` (terminal resize), `SIGTSTP` (Ctrl-Z suspend), `SIGCONT` (resume after suspend), `SIGTERM` (clean exit). Signals are caught via `signal()` or `sigaction` FFI calls; handlers set atomic flags that the event loop checks.
- Buffered output helpers: wraps `stdout` writes so that each rendered frame is assembled in memory and flushed with a single `write()` syscall.

Key types: `Terminal` struct (owns the termios state and the write buffer).

### `input.rs`

Key event reading from `/dev/tty`.

- Opens `/dev/tty` directly (not stdin) so that key input works correctly when stdin is a pipe feeding file content.
- Defines the `Key` enum covering all keys the pager acts on: arrow keys, page up/down, Home/End, printable characters, Ctrl combinations, Escape.
- Internal 64-byte read buffer: reads up to 64 bytes at once. This is critical for escape sequences — arrow keys send `\x1b[A` as three bytes. Buffering the read means all three bytes arrive in one call, avoiding timing-dependent races where only `\x1b` is read and the rest is lost.
- Escape sequence parser with a 50ms timeout: after seeing `\x1b`, waits up to 50ms for the rest of the sequence before treating it as a bare Escape keypress.
- Raw mode for the tty fd is set here (separate from the stdout terminal) so that escape sequences arrive as raw bytes even when stdin is redirected.

Key types: `Key` enum, `InputReader` struct.

### `buffer.rs`

Lazy streaming line buffer for file content.

- Reads lines from the input source on demand rather than loading the entire file upfront. This gives instant startup even on very large files.
- Binary detection: reads the first 8KB and checks for null bytes. If found, refuses to display and reports the file as binary.
- Line truncation at 64KB: any single line longer than 64KB is hard-truncated. Prevents memory explosion on pathological input (e.g., a file with no newlines).
- Follow mode: when enabled (equivalent to `tail -f`), polls the input source for new data after reaching EOF rather than stopping.
- Exposes a random-access-by-line-index interface: `pager.rs` asks for line N and the buffer reads forward as needed, caching lines already read.

Key types: `LineBuffer` struct, methods `get(n)`, `len()`, `is_binary()`.

### `ansi.rs`

ANSI escape code utilities.

- `strip_ansi(s)`: removes all ANSI escape sequences from a string, returning plain text.
- `visible_width(s)`: computes the display width of a string (after stripping ANSI), accounting for wide Unicode characters (CJK, etc.) via `unicode-width`.
- `truncate_to_width(s, max_width)`: truncates a string to fit within `max_width` terminal columns, but preserves ANSI color state. If a color escape was active before the truncation point, the returned string still starts in that color. This is necessary for correct horizontal scrolling — if you slice a colored line mid-segment, the color must carry through.

Key types: none (free functions).

### `history.rs`

Persistent search history.

- Stores up to 100 search patterns in `$XDG_STATE_HOME/lz/history` (default `~/.local/state/lz/history`), one pattern per line.
- Loaded at startup, saved on exit. Failures are silent (missing file = empty history, write failure = no persistence).
- Deduplicates on push: if a pattern already exists, it is moved to the end.
- Cursor-based cycling: Up/Down in search input mode walk backward/forward through history. The in-progress input is saved as a "draft" on first Up press and restored when cycling past the newest entry.
- `main.rs` owns the file path resolution and calls `load`/`save`. `pager.rs` owns the `SearchHistory` instance and calls `push`/`prev`/`next`/`reset_cursor` during search input handling.

Key types: `SearchHistory` struct.

### `search.rs`

Search over file content with a fast literal path.

- Two-tier matching via the `Matcher` enum: `Literal(BMH)` for patterns with no regex metacharacters, `Regex(Regex)` for everything else. The `is_literal()` function detects which path to use.
- `BMH` struct implements Boyer-Moore-Horspool substring search with a 256-entry bad-character shift table. For literal patterns this skips large chunks of the haystack without examining every byte — dramatically faster than regex for common search terms.
- Smart-case logic: if the search pattern contains any uppercase letter, the search is case-sensitive; if the pattern is all lowercase, it is case-insensitive. This matches vim and less behavior.
- Searches on ANSI-stripped text (delegates to `ansi.rs`) so that color codes in the input do not interfere with pattern matching.
- Returns match byte ranges on the stripped text; `pager.rs` uses these to inject yellow highlighting escapes when rendering.
- Integrates with `buffer.rs` to scan forward/backward through lines for `n`/`N` navigation.

Key types: `Search` struct, `Matcher` enum (`Literal`, `Regex`), `BMH` struct.

### `pager.rs`

The main event loop and renderer. This is the largest and most central module.

- Five modes via the `Mode` enum: `Normal`, `SearchInput` (typing a search pattern), `FilterInput` (typing a filter pattern), `Follow` (like `tail -f`), and `Help` (overlay showing keybindings).
- `SearchDir` enum (`Forward`, `Backward`) tracks whether search was initiated with `/` (forward) or `?` (backward). `n` repeats in the same direction, `N` in the opposite direction.
- Line filtering: `&` enters `FilterInput` mode. On Enter, builds a `Search` from the pattern and populates `filtered_lines: Vec<usize>` with matching line indices. All rendering and navigation then operate on this filtered subset. Empty pattern clears the filter.
- Help mode: `h` toggles an overlay displaying all keybindings. `render_help()` draws static help text instead of file content. `h`, `q`, or Escape returns to Normal.
- Rendering: full redraw every frame. Each frame: clear screen, render visible lines (or help screen) top-to-bottom, render inverse-video status bar at the bottom showing context on the left (filename, filter, wrap state, or input prompt) and position info on the right (line range, total, percentage/END).
- Horizontal scroll: tracks a column offset, increments/decrements in 8-column steps.
- Wrap mode: toggled with `w`. When enabled, long lines are wrapped using `unicode-width` for correct column calculation (this is why `pager.rs` depends on the crate directly).
- Search highlighting: when a search is active, matching spans on each visible line are wrapped in `\x1b[30;43m` (black text on yellow background) and reset afterward.
- Line editor: the search/filter prompt at the bottom is a simple line editor supporting character input, backspace, cursor movement, and Escape/Enter. Up/Down cycle through search history (search mode only).
- Key dispatch table: maps `Key` variants to actions in Normal mode (scroll, search, backward search, filter, quit, toggle wrap, enter follow mode, help, etc.).
- Signal integration: checks the atomic flags set by signal handlers in `terminal.rs` on each event loop iteration. Handles resize by re-querying terminal dimensions and redrawing; handles TSTP by suspending the process after restoring the terminal.

Key types: `Pager` struct, `Mode` enum (`Normal`, `SearchInput`, `FilterInput`, `Follow`, `Help`), `SearchDir` enum (`Forward`, `Backward`), `LineEditor` struct.

---

## How lz Differs from `less`

### What lz Does

- View files and piped stdin
- Line scroll, half-page scroll, full-page scroll, jump to top/bottom
- Horizontal scroll in 8-column increments
- Regex search with smart-case, forward (`/`) and backward (`?`) search, `n`/`N` navigation, yellow highlighting, persistent search history (Up/Down in search prompt)
- Line filtering (`&`) — show only lines matching a pattern
- Interactive help screen (`h` key)
- Wrap toggle (`w` key)
- Follow mode (`--follow` flag or `F` key, equivalent to `tail -f`)
- ANSI color pass-through (colors in input are preserved in output)
- Raw escape mode (`-r` flag, shows escape sequences literally)
- Binary file detection (refuses to display binary content)
- Signal handling: Ctrl-Z suspend/resume, terminal resize (SIGWINCH), clean exit on SIGTERM

### What lz Intentionally Does Not Do

- No marks or bookmarks
- No multiple file arguments
- No line number display
- No pipe commands (no `|` prompt)
- No custom keybindings
- No mouse support
- No Windows support
- No configuration file (no `.lessrc` equivalent)
- No command-line options beyond the small set actually implemented

The "does not do" list is a feature, not a gap. These omissions keep the code simple and the binary small.

---

## Key Design Decisions

**Full redraw per frame, not diff-based rendering.**
Every keypress triggers a full repaint: clear screen, write all visible lines, flush. This is simpler than tracking what changed and surgically updating the terminal. It avoids an entire class of cursor-tracking bugs. With a single `write()` syscall per frame, it is fast enough in practice.

**Raw termios via libc FFI, not crossterm or termion.**
Using crossterm or termion would be convenient but would add transitive dependencies and increase binary size. Since lz only needs a small, fixed set of terminal operations, direct FFI is feasible and keeps the dependency count at two.

**Hand-rolled argument parsing, not clap.**
lz has approximately 8 flags. Pulling in clap for 8 flags would add significant compile time and binary size. The hand-rolled parser is short and straightforward.

**Key input from `/dev/tty`, not stdin.**
When lz is invoked as `something | lz`, stdin is the pipe carrying file content. Key input must come from a separate fd connected to the terminal. Opening `/dev/tty` explicitly is the correct Unix approach. Raw mode is also set on this fd so escape sequences arrive as raw bytes.

**64-byte internal read buffer for key input.**
Arrow keys and other special keys send multi-byte escape sequences (e.g., `\x1b[A` for up arrow). If you read one byte at a time and the read returns only `\x1b`, you cannot tell whether it is a bare Escape or the start of a sequence. Reading up to 64 bytes at once means that in practice the entire sequence arrives in one read. The 50ms timeout handles the rare case where `\x1b` arrives alone.

**Smart-case search.**
If the search pattern is all lowercase, the search is case-insensitive. If it contains any uppercase letter, it is case-sensitive. This matches vim and less behavior and is the expected default for most users.

**Boyer-Moore-Horspool fast path for literal searches.**
Most real-world searches are literal strings, not regexes. BMH with a 256-entry shift table skips large portions of each line without examining every byte, giving a significant speedup over regex for the common case. The `is_literal()` check detects regex metacharacters; if none are present, the search is routed to BMH instead of `regex-lite`. This is why `search.rs` has the `Matcher` enum dispatching between `Literal` and `Regex`.

**Lazy line loading.**
Lines are read from the source on demand and cached. The pager does not read the entire file on startup. This gives instant startup for large files and makes stdin streaming natural.

**64KB line truncation.**
A line longer than 64KB is truncated. This prevents a single pathological line from consuming unbounded memory. The truncation point is chosen to be far larger than any reasonable line while still being a hard cap.

---

## Build and Test

Tests live in the standard Rust `#[cfg(test)]` blocks within each module.

`bench/run.sh` runs comparative benchmarks against `less` using `expect` and `hyperfine`. It generates a 1M-line test file and measures startup, jump-to-end, search (hit/miss), and page-through scenarios. Requires `expect` and `hyperfine` to be installed; run `cargo build --release` first.

The release binary is small by design. Do not add dependencies without a strong reason. Before adding any crate, check whether the functionality can be implemented directly in a reasonable number of lines.
