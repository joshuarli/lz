# AGENTS.md — lz Codebase Guide

This document is written for AI agents working on the lz codebase. It covers architecture, module responsibilities, design decisions, and how to build and test.

---

## What lz Is

lz is a minimal, fast pager (like `less`) written in Rust. The active parity campaign targets the documented less 668 surface; see `plan.md` for its scope and remaining proof. It is Unix-only and has two direct runtime dependencies: `regex-automata` and `unicode-width`; `regex-syntax` is the regex parser. Everything else — terminal control, key input, signal handling — is done via direct libc FFI or standard library primitives.

The design philosophy is small binary, simple code, zero unnecessary dependencies. `plan.md` now authorizes a less 668 parity campaign that supersedes the intentional omissions below; those sections describe the current interactive implementation, not the target contract. Redirected stdout already copies arbitrary input bytes before any terminal or binary detection. `make parity-setup` builds isolated ordinary/instrumented references; `make parity-test GROUP=redirected MODE=admit` compares redirected behavior offline.

---

## Source Layout

All source lives in `src/`:

```
src/
  lib.rs        Library root (modules below) and binary test suite
  main.rs       CLI entry point, arg parsing, panic hook
  tags.rs       Byte-preserving numeric and literal-prefix ctags startup selection
  security.rs   Secure feature-name parsing, permission mask and diagnostics
  options.rs    Binary-only CLI option-name diagnostics
  pager.rs      Main event loop and rendering
  buffer.rs     Byte source access, line indexing and display fragments
  charset.rs    ASCII/UTF-8 policy for binary samples and byte presentation
  layout.rs     Bounded byte-prefix mapping for standard/color display and notation
  storage.rs    Anonymous spill files and bounded line indexes
  readiness.rs  Platform descriptor readiness
  input.rs      Key reading and escape sequence parsing
  terminal.rs   Raw termios, alt screen, signals, output helpers
  ansi.rs       ANSI stripping, visible width, color-preserving truncation
  search.rs     Regex/BMH search with smart-case
  history.rs    Persistent search history (Up/Down in search prompt)

benches/
  bench.rs      rustybench harness (buffer scan, search, ANSI/width hot paths)
  baseline.json Baseline for `make bench`
```

---

## Module Dependency Graph

```
main.rs
  └── lz (the library)

lib.rs
  ├── pager.rs
  │     ├── buffer.rs
  │     ├── history.rs
  │     ├── input.rs
  │     ├── search.rs
  │     │     ├── ansi.rs
  │     │     └── buffer.rs
  │     ├── terminal.rs
  │     ├── ansi.rs
  │     └── unicode-width (crate)
  └── history.rs

ansi.rs
  └── unicode-width (crate)
```

`main.rs` is a thin binary that links `lib.rs`; `benches/bench.rs` also links the library. `pager.rs` owns the event loop and pulls in everything else. `search.rs` and `ansi.rs` are utility modules used by multiple callers. `terminal.rs` and `input.rs` are low-level I/O modules with no upward dependencies. `history.rs` is a standalone utility used by both `main.rs` (load/save) and `pager.rs` (cycling).

---

## Module Responsibilities

### `main.rs`

`options.rs` owns one byte-preserving parser for option names, numeric and string
values, LESS defaults and the first-filename boundary. Missing values stop output
with status 0; other diagnosed option errors allow redirected file copying. `-h`
takes a number and `-?` requests help. Most option effects remain unimplemented;
parsing a documented name does not establish full CLI parity.

Entry point and CLI argument parsing.

- Hand-rolled argument parsing (no clap). Handles `--follow`, `--force`, `-r`/`--raw`, `-h`/`--help`, `-V`/`--version`, `--`, `-` (explicit stdin), and positional file arguments.
- Installs a panic hook that restores the terminal (exits alt screen, disables raw mode) before printing the panic message, so a crash does not leave the terminal broken.
- Calls into `pager::run()` with a constructed config.

Startup `-t` resolves a numeric or literal-prefix ctags address; `-T` changes the tags file. `tags::Selection` retains filename bytes and an `Address` containing a nonzero line number or literal prefix with optional end matching. Pattern matching uses bounded `LineBuffer::raw_byte_prefix` reads without changing search state. Raw prefixes exclude LF but retain CR and arbitrary bytes; the display adapter separately normalizes CRLF. Records follow the reference’s 1024-byte input-buffer boundary. Duplicates, error prompts, color conversion, long-running cancellation, interactive tags and GLOBAL helpers remain open.

Key types: none (thin glue layer).

### `terminal.rs`

`Session::start` owns the `KeyReader` tty and acquired modes. Use `keys_mut` for
input and `finish` to observe restoration errors; Drop also unwinds partial setup.
Raw-mode helpers and signal installation are private. `TERMINATION_SIGNAL`
replaces the old boolean TERM flag and preserves the first TERM/HUP request.
TERM exits 15 after cleanup, matching the pinned oracle. HUP restores represented
modes and re-raises HUP; this is an explicit safety divergence from the oracle's
terminal leak, parked in the parity register. Startup enables application
cursor/keypad modes and leaves the prompt cursor visible. `LESSHISTFILE=-` disables
history reads and writes. `HistoryFile` now resolves the 668 default/explicit path and format; legacy lz history stays unread and untouched.

All raw terminal interaction via libc FFI.

- Sets and restores raw termios mode through direct `tcgetattr` and `tcsetattr` FFI. Mode changes drain output and preserve queued input. No crossterm or termion.
- Manages the alternate screen (`\x1b[?1049h` / `\x1b[?1049l`).
- Cursor control: hide/show, absolute positioning.
- Signal handling: `SIGWINCH` (terminal resize), `SIGTSTP` (Ctrl-Z suspend), `SIGCONT` (resume after suspend), `SIGTERM` (clean exit). Signals are caught via `signal()` or `sigaction` FFI calls; handlers set atomic flags that the event loop checks.
- Buffered output helpers: wraps `stdout` writes so that each rendered frame is assembled in memory and flushed with a single `write()` syscall.

Key types: `Session` owns keyboard input and signal dispositions. `Session::start(Screen::Primary)` acquires raw input without alternate or application modes. `enter_interactive(InteractiveOptions)` acquires an interactive display once, with independent `ScreenMode` and `KeypadMode` policies. `Screen::Interactive` applies those policies during startup. Primary-screen rendering retains a relative `DisplayAnchor`; exit clears its current prompt. Nondefault suspend/resume remains untested.

### `input.rs`

Key event reading from `/dev/tty`.

- Opens `/dev/tty` directly (not stdin) so that key input works correctly when stdin is a pipe feeding file content.
- Defines the `Key` enum covering all keys the pager acts on: arrow keys, page up/down, Home/End, printable characters, Ctrl combinations, Escape.
- Internal 64-byte read buffer: reads up to 64 bytes at once. This is critical for escape sequences — arrow keys send `\x1b[A` as three bytes. Buffering the read means all three bytes arrive in one call, avoiding timing-dependent races where only `\x1b` is read and the rest is lost.
- Read readiness uses `select` on macOS (`poll` rejects `/dev/tty` with `POLLNVAL`) and `poll` on Linux. Idle key waits allow the pager to observe signal flags without repainting.
- Escape sequence parser with a 50ms timeout: after seeing `\x1b`, waits up to 50ms for the rest of the sequence before treating it as a bare Escape keypress.
- `Key::is_prompt_acknowledgement` defines which dismissal keys are consumed; other keys resume normal dispatch.
- `unread_key` retains one decoded key before buffered input and rejects overwriting a pending key. Startup diagnostics use it to preserve a non-Return command.
- Raw mode for the tty fd is set here (separate from the stdout terminal) so that escape sequences arrive as raw bytes even when stdin is redirected.

Key types: `Key` enum, `InputReader` struct.

### `buffer.rs`

Byte source access and logical line indexing.

- Regular files remain seekable. Nonseekable stdin is polled before each read and retained in an unlinked, mode-0600 spill file.
- Scanning uses 64 KiB blocks. Line offsets spill after 8,192 entries, so source retention does not require an in-memory copy of every line.
- `line_prefix` reads a bounded display fragment without waiting for the end of a logical line. `get_line` explicitly materializes a complete line for search and some navigation paths. Those operations still need large-line bounds and cancellation work.
- `SourceState` distinguishes ready data, temporary waiting, EOF and read failure. Read failures reach the pager instead of becoming EOF.
- Source bytes stay intact in the file or spool. Display fragments currently decode with replacement for malformed UTF-8. Exact malformed-byte presentation remains unfinished.
- File binary detection samples 256 bytes without changing the file position. More than five binary units trigger a question. Detection and filename questions share `Charset`; `ControlDisplay::Color` excludes SGR spans from the sample, preserving the reference's distinct OSC-prefix handling during binary sampling. Accepted/forced bytes use inverse caret/hex notation. Other charsets remain unfinished.

Key types: `LineBuffer`, `LineFragment`, `SourceState`. Mutable line injection and the old prepopulation interface are retired.

### `ansi.rs`

ANSI escape code utilities.

- `strip_ansi(s)`: removes all ANSI escape sequences from a string, returning plain text.
- `visible_width(s)`: computes the display width of a string (after stripping ANSI), accounting for wide Unicode characters (CJK, etc.) via `unicode-width`.
- `truncate_to_width(s, max_width)`: truncates a string to fit within `max_width` terminal columns, but preserves ANSI color state. If a color escape was active before the truncation point, the returned string still starts in that color. This is necessary for correct horizontal scrolling — if you slice a colored line mid-segment, the color must carry through.

Standard and color rendering use `layout.rs`: byte ranges map to columns, atomic wrap boundaries, printable notation and overstrike attributes. Color spans preserve source SGR state across generated notation and resets. Raw rendering and whole-line search/navigation still use the earlier string utilities.

Key types: `ControlDisplay`; the remaining ANSI functions also serve search and benchmarks.

### `history.rs`

Persistent search history in the less format.

- `HistoryFile` captures path/size policy, searches existing XDG/home destinations, and resolves the save destination again at exit. `LESSHISTFILE=-` or `/dev/null` disables persistence. Main applies the shared secure feature policy before constructing it; path/size resolution does not duplicate permission checks.
- Search entries suppress consecutive duplicates. Raw stored bytes survive merge, including shell records. Full command editing and malformed-byte recall remain unfinished.
- Closing a painted file records a position change separately from accepted search input. Saving rereads the current destination, trims old entries by `LESSHISTSIZE`, writes a private file and renames it atomically. I/O failures are silent.
- The old `$XDG_STATE_HOME/lz/history` path is not read or migrated. Parent directories are not created for a history destination.
- Marks, concurrent/interrupted writers and temporary-name collisions still need parity proof.

Key types: `SearchHistory` owns input history; `HistoryFile` owns persistence policy. `save` mutates saved-entry flags, and `Pager::into_history` notes the last displayed position.

### `search.rs`

Search over file content with a fast literal path.

- Two-tier matching via the `Matcher` enum: `Literal(BMH)` for patterns with no regex metacharacters, `Regex(Box<RegexMatcher>)` for everything else. The `is_literal()` function detects which path to use.
- `BMH` struct implements Boyer-Moore-Horspool substring search with a 256-entry bad-character shift table. For literal patterns this skips large chunks of the haystack without examining every byte — dramatically faster than regex for common search terms.
- Case policy is explicit: sensitive by default, smart case with `-i`, and insensitive with `-I`. `Search::new(pattern, CaseSensitivity)` receives the resolved policy. The literal folding path is ASCII-only; Unicode folding parity is unproved.
- Searches on ANSI-stripped text (delegates to `ansi.rs`) so that color codes in the input do not interfere with pattern matching.
- Returns match byte ranges on the stripped text; `pager.rs` uses these to inject inverse highlighting escapes when rendering.
- Integrates with `buffer.rs` to scan forward/backward through lines for `n`/`N` navigation.

Key types: `Search` struct, `Matcher` enum (`Literal`, `Regex`), `BMH` struct.

### `pager.rs`

The main event loop and renderer. This is the largest and most central module.

- Modes via the `Mode` enum: `Normal`, `SearchInput` (typing a search pattern), `FilterInput` (typing a filter pattern), `Follow` (like `tail -f`), `Help` (overlay showing keybindings), and `CommandError` (a pending command paused for acknowledgment).
- `SetMark` and `JumpMark` modes own ordinary in-file marks. Marks currently store logical line indices; wrapped, cross-file and persisted positions remain open. Large jumps update the previous-position mark.
- `SearchDir` enum (`Forward`, `Backward`) tracks whether search was initiated with `/` (forward) or `?` (backward). `n` repeats in the same direction, `N` in the opposite direction.
- Line filtering: `&` enters `FilterInput` mode. On Enter, builds a `Search` from the pattern and populates `filtered_lines: Vec<usize>` with matching line indices. All rendering and navigation then operate on this filtered subset. Empty pattern clears the filter.
- Help mode: `h` toggles an overlay displaying all keybindings. `render_help()` draws static help text instead of file content. `h`, `q`, or Escape returns to Normal.
- A complete line at the bottom of the viewport probes for the next source line with a zero-byte prefix request; this detects exact-viewport EOF without reading a whole following line.
- Initial short-file content is placed at the bottom with blank unused rows. Empty files keep an interactive EOF view. Explicit startup positioning uses a full paint.
- Rendering: full redraw every frame. Each frame: clear screen, render visible lines (or help screen) top-to-bottom, render inverse-video status bar at the bottom showing context on the left (filename, filter, wrap state, or input prompt) and position info on the right (line range, total, percentage/END).
- Horizontal scroll: tracks a column offset, increments/decrements in 8-column steps.
- Wrap mode: enabled by default and disabled with `-S`. When enabled, long lines are wrapped using `unicode-width` for correct column calculation (this is why `pager.rs` depends on the crate directly).
- Search highlighting: when a search is active, matching spans on each visible line are wrapped in `\x1b[7m` (inverse video) and reset afterward.
- Line editor: the search/filter prompt at the bottom is a simple line editor supporting character input, backspace, cursor movement, and Escape/Enter. Up/Down cycle through search history (search mode only).
- Default page movement uses the content height, and half-page movement uses the terminal height rounded up to the next half. Numeric d/u set the shared half-page default; numeric z/w set the shared forward/backward window, including page commands. Main carries opaque `ScrollDefaults` across file visits. Zero command prefixes retain defaults while leading zeros remain valid digits. CLI -z/--window and the numeric alias accept fixed sizes or nonpositive screen-relative settings; resize recalculates relative windows. Effective nonpositive windows keep position and request the once-per-wall-clock-second boundary bell. Plain-line aliases e/y/f, Ctrl-E/N/Y/P/K/V and angle-bracket jumps share their canonical dispatch paths. Wrapped movement and custom headers remain open.
- Key dispatch table: maps `Key` variants to actions in Normal mode (scroll, search, backward search, filter, quit, enter follow mode, help, etc.).
- Signal integration: checks the atomic flags set by signal handlers in `terminal.rs` on each event loop iteration. Handles resize by re-querying terminal dimensions and redrawing; handles TSTP by suspending the process after restoring the terminal.

`Pager::new` takes a named `Config` containing startup command bytes, charset and control-display policy,
chopping, line-number display, search case policy, validated `FileStatus`, retained `DisplayAnchor` and `ScrollDefaults`, and an explicit `InitialDisplay` placement. `InitialDisplay::FileSwitch` carries an optional opaque `SavedPosition`, obtained with `saved_position` before transferring history. Main retains one saved logical-line position per CLI file; wrapped-row and horizontal offsets remain open. `run` returns `PagerExit`; main opens the requested next/previous file under the same terminal session. `into_history` transfers search history to the next pager. Startup commands use the same key decoder and dispatcher
as tty input, before the first frame. Numeric startup positions and `-p` searches
are supported. A position-dependent startup movement before any jump/search rings
a bell and discards the remaining command, matching the reference.

Key types: `Pager` struct, `Mode` enum (`Normal`, `SearchInput`, `FilterInput`, `Follow`, `Help`), `SearchDir` enum (`Forward`, `Backward`), `LineEditor` struct.

---

## How lz Differs from `less`

### What lz Does

- View files and piped stdin
- Line scroll, half-page scroll, full-page scroll, jump to top/bottom
- Horizontal scroll in 8-column increments
- Regex search with smart-case, forward (`/`) and backward (`?`) search, `n`/`N` navigation, inverse highlighting, persistent search history (Up/Down in search prompt)
- Line filtering (`&`) — show only lines matching a pattern
- Interactive help screen (`h` key)
- Default wrapping, with `-S` chopping; `z`/`w` window movement
- Regular-file follow (`+F` or `F`), including appended lines and return to navigation with Ctrl-C/Ctrl-X. The default waiting message is emphasized. Ctrl-C rings a bell; Ctrl-X does not. The mode owns follow state without a duplicate flag. Custom interrupts, truncation/replacement and continuous/live sources remain open.
- ANSI color pass-through (colors in input are preserved in output)
- Raw escape mode (`-r` flag, shows escape sequences literally)
- Binary file detection (refuses to display binary content)
- Signal handling: Ctrl-Z suspend/resume, terminal resize (SIGWINCH), clean exit on SIGTERM

### What lz Intentionally Does Not Do

- Ordinary in-file marks are implemented; wrapped, cross-file and persisted marks remain unfinished
- Line-number options beyond the implemented -N gutter remain incomplete
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

**Search case policy.**
Search is case-sensitive by default. `-i` enables smart case, and `-I` always ignores case. Simple case flags in `LESS` apply before CLI overrides. Unicode case parity remains unproved.

**Boyer-Moore-Horspool fast path for literal searches.**
Most real-world searches are literal strings, not regexes. BMH with a 256-entry shift table skips large portions of each line without examining every byte, giving a significant speedup over regex for the common case. The `is_literal()` check detects regex metacharacters; if none are present, the search is routed to BMH instead of the regex automaton. This is why `search.rs` has the `Matcher` enum dispatching between `Literal` and `Regex`.

**Lazy line loading.**
Display reads bounded prefixes from seekable files or retained stream bytes. Line indexes spill to disk after a fixed in-memory allowance.

**Long lines.**
Source bytes are preserved in full. Initial display does not allocate a complete
logical line. Whole-line search and wrapped movement still materialize lines.

---

## Build and Test

- `make build` — debug build; `make test` — quiet test run (`make test-ci` requires a dist binary from `make release`).
- `make parity-fuzz SEED=0 CASES=50` — deterministic navigation/mark cases; `tests/parity/fuzz.rs` owns generation and bounded event reduction. It shares the existing comparator and scoped worker cleanup. Full observations use gzip JSON; `gzip` must be available. Candidate and oracle-only proof pointers are distinct. PTY pager RSS has a 512 MiB ceiling with live sampling and mandatory kernel peak accounting. Successful records remain outside screen comparisons; kernel termios before/after exit is compared and asserted before repair. Optional scenario `pty_rss_limit` can lower the ceiling. A 2 GiB sampled owned-spill ceiling counts writable unlinked regular-file lengths once per device/inode; `pty_spill_limit` may lower it. The Darwin descriptor ABI was checked against the SDK and live alias/read-only/write-only controls. Grammar expansion, full minimization, stdio/helper/producer resource coverage, transient-spill peak proof and hardening remain open.
- `make lint` — `cargo clippy --workspace --all-targets -- -D warnings`.
- `make release` — static release binary at `target/<target>/dist/lz` using the `dist` profile (fat LTO, stripped; builds `std` from source, for musl static by default on Linux). `make verify-release` checks the binary is static and stripped on Linux. Linux CI builds run in the Alpine-based `Dockerfile`.
- CI is `.github/workflows/ci.yml` (macOS + Linux musl), `lint.yml`, and `release.yml` (dispatch-only pre-release with zips and SHA256SUMS; releases are tagged `release-<sha>`).

Tests live in the standard Rust `#[cfg(test)]` blocks within each module.

`cargo bench --bench bench` runs the rustybench harness in `benches/bench.rs` (buffer scanning, literal/regex search, and ANSI/width hot paths). `make bench` re-runs it against `benches/baseline.json` and rewrites that file; `make bench-syscalls` reports syscall counts on Linux.

The release binary is small by design. Do not add dependencies without a strong reason. Before adding any crate, check whether the functionality can be implemented directly in a reasonable number of lines.
