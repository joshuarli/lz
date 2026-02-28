# `lz` — Implementation Plan

## Context

Build an ultra-minimal, performant alternative to `less` in Rust with near-zero dependencies. The pager should handle the common 90% of `less` usage with clean code and fast startup.

**Dependencies**: `regex-lite`, `unicode-width` (and nothing else)

## Architecture

```
src/
  main.rs          — arg parsing, entry point, error handling
  terminal.rs      — raw termios, ANSI output, key reading, screen size
  input.rs         — key event types, escape sequence parser (50ms timeout)
  buffer.rs        — lazy line buffer (stream + cache), binary detection, stdin/file abstraction
  pager.rs         — main event loop, state machine, rendering, scroll/search logic
  search.rs        — regex-lite wrapper, smart-case, ANSI-stripping, match positions
  ansi.rs          — ANSI escape parsing/stripping for search, display width calculations
```

## Implementation Steps

### Step 1: Project scaffold
- `cargo init` with edition 2021
- Add `regex-lite` and `unicode-width` to `Cargo.toml`
- Minimal `main.rs` that prints "lz"

### Step 2: CLI arg parsing (`main.rs`)
Hand-rolled arg parser. Flags:
- `--follow` — follow mode
- `--force` — allow binary files
- `-r` — raw mode (don't interpret ANSI)
- `--help` — compact help text
- `--version` — version string
- `--` — end of flags
- `-` — explicit stdin
- Positional: single filename

Exit codes: 0 success, 1 error, 2 usage. Errors to stderr.

### Step 3: Terminal handling (`terminal.rs`)
Raw termios via libc FFI (no crate):
- `enable_raw_mode()` / `disable_raw_mode()` — tcgetattr/tcsetattr
- `enter_alt_screen()` / `leave_alt_screen()` — `\x1b[?1049h` / `\x1b[?1049l`
- `get_terminal_size()` — ioctl TIOCGWINSZ
- `hide_cursor()` / `show_cursor()`
- `move_cursor(row, col)`, `clear_screen()`, `clear_line()`
- Buffered writer — collect all output into a `Vec<u8>`, flush once per frame

Signal handling:
- SIGWINCH → set atomic flag, main loop checks and resizes
- SIGTSTP → restore terminal, raise SIGSTOP
- SIGCONT → re-enter raw mode, redraw
- SIGTERM → restore terminal, exit cleanly

### Step 4: Key input (`input.rs`)
Define `enum Key`:
- `Char(char)`, `Ctrl(char)`, `Escape`
- `Up`, `Down`, `Left`, `Right`
- `PageUp`, `PageDown`, `Home`, `End`
- `Backspace`, `Delete`, `Enter`
- `Unknown`

Read from `/dev/tty` (not stdin, since stdin may be piped).
Escape sequence parser:
- Read byte, if `0x1b`, wait up to 50ms for `[`, then parse CSI sequence
- Map `\x1b[A` → Up, `\x1b[B` → Down, etc.
- `\x1b[5~` → PageUp, `\x1b[6~` → PageDown, `\x1b[H` → Home, `\x1b[F` → End
- If 50ms timeout with no follow-up byte → `Escape`

### Step 5: Line buffer (`buffer.rs`)
```rust
struct LineBuffer {
    lines: Vec<String>,       // cached lines
    source: Source,           // file reader or stdin reader
    finished: bool,           // true when EOF reached
}
enum Source {
    File(BufReader<File>),
    Stdin(BufReader<...>),
}
```

- `get_line(n) -> Option<&str>` — returns line n, reading forward if needed
- `line_count() -> Option<usize>` — None if not fully read yet
- `is_finished() -> bool`
- Binary detection: check first 8KB for null bytes. If binary and `--force` not set, return error before entering pager.
- Empty input: detect immediately, exit 0 without entering pager.

For `--follow` mode: periodically try to read more lines (poll-based in the event loop).

### Step 6: ANSI utilities (`ansi.rs`)
- `strip_ansi(line: &str) -> String` — remove all `\x1b[...m` sequences, return visible text
- `visible_width(line: &str) -> usize` — display width of visible text using `unicode-width`
- `truncate_to_width(line: &str, start_col: usize, max_width: usize) -> String` — slice a line for display, preserving ANSI state. This is the trickiest function:
  - Track active ANSI style as we walk the string
  - Skip characters before `start_col` (for horizontal scroll)
  - Emit characters until `max_width` columns filled
  - Handle wide characters (CJK/emoji taking 2 columns)
  - In `-r` mode: don't interpret escapes, show them as literal text

### Step 7: Search (`search.rs`)
```rust
struct Search {
    pattern: String,
    regex: Regex,            // regex-lite
    smart_case: bool,
}
```

- `Search::new(pattern: &str)` — if pattern has uppercase → case-sensitive, else case-insensitive
- `find_matches(visible_text: &str) -> Vec<(usize, usize)>` — byte ranges of matches in ANSI-stripped text
- `find_next_line(buffer: &LineBuffer, from_line: usize, forward: bool) -> Option<usize>` — scan for next matching line

### Step 8: Pager core (`pager.rs`)
State:
```rust
struct Pager {
    buffer: LineBuffer,
    top_line: usize,          // first visible line
    left_col: usize,          // horizontal scroll offset
    wrap: bool,               // wrap mode (default false)
    search: Option<Search>,
    current_match_line: Option<usize>,
    search_direction_forward: bool,
    mode: Mode,               // Normal, SearchInput, Follow
    search_input: LineEditor,  // for the search prompt
    term_width: u16,
    term_height: u16,         // content area = term_height - 1 (status line)
    follow: bool,
    raw_mode: bool,           // -r flag
    needs_full_redraw: bool,
}

enum Mode {
    Normal,
    SearchInput,
    Follow,
}
```

**Main event loop**:
1. Render current view
2. In Follow mode: poll for new input from file/stdin, poll for keypress (non-blocking with short timeout)
3. In Normal/SearchInput mode: blocking read for next key
4. Dispatch key to handler
5. Loop

**Rendering** (full redraw + buffered flush):
- Build entire screen into a `Vec<u8>` buffer
- For each visible row:
  - Get the line from LineBuffer
  - Apply `truncate_to_width()` for horizontal scroll (or wrap logic)
  - If search active: highlight matches in visible portion (black on yellow: `\x1b[30;43m`)
  - Write line content, clear to end of line (`\x1b[K`)
- Render status line (reverse video `\x1b[7m`):
  - If search active: show `/pattern`
  - Otherwise: show `:` (or filename + percentage on demand)
  - Show "END" when at bottom, "Pattern not found" when applicable
- Flush buffer in one write

**Key dispatch (Normal mode)**:
| Key | Action |
|---|---|
| `j`, `Down` | scroll down 1 line |
| `k`, `Up` | scroll up 1 line |
| `Space`, `PgDn` | page down (screen - 1 line overlap) |
| `b`, `PgUp` | page up (screen - 1 line overlap) |
| `g`, `Home` | go to top |
| `G`, `End` | go to bottom (read all if needed) |
| `Right` | scroll right 8 cols |
| `Left` | scroll left 8 cols (min 0) |
| `/` | enter search mode |
| `n` | next match |
| `N` | previous match |
| `w` | toggle wrap |
| `q`, `Escape`, `Ctrl-C` | quit |

**Key dispatch (SearchInput mode)**:
- Printable chars → insert at cursor
- Backspace → delete before cursor
- Left/Right → move cursor
- Home/End → cursor to start/end
- Enter → compile regex, execute search, switch to Normal
- Escape, Ctrl-C → cancel, switch to Normal

**Search navigation**:
- `n`: from current match line, scan forward for next line with match. If not found, show "Pattern not found (END)".
- `N`: scan backward. If not found, show "Pattern not found (TOP)".
- On jump: set `top_line` so match is visible. In no-wrap mode: adjust `left_col` so first match on line is visible.

**Wrap mode**:
- When wrap is on, a single logical line may span multiple screen rows
- Need a mapping: screen row → (logical line, offset within line)
- `visible_width()` determines how many screen rows a line occupies
- Page up/down counts screen rows, not logical lines

### Step 9: Follow mode
- Entered via `--follow` flag at startup
- `mode` starts as `Mode::Follow`
- Event loop uses non-blocking key read (short poll timeout ~100ms)
- Each iteration: try to read new lines into buffer, scroll to bottom, redraw
- Only `q`/`Escape`/`Ctrl-C` are active (no search, no scroll)
- On EOF for stdin: keep polling (more data may arrive)
- On EOF for file: keep trying to read (file may be appended to)

### Step 10: `main.rs` entry point
1. Parse args
2. If `--help`: print help, exit 0
3. If `--version`: print version, exit 0
4. Open input source (file or stdin). On error → stderr, exit 1.
5. Create `LineBuffer`. Check for binary. Check for empty → exit 0.
6. Set up terminal (raw mode, alt screen, signal handlers)
7. Create `Pager`, run event loop
8. On exit: restore terminal (leave alt screen, disable raw mode, show cursor)
9. Exit 0

Ensure terminal restoration happens even on panic (set a panic hook).

## Line truncation limit

Truncate lines at 64KB. Lines longer than this get a `[truncated]` marker appended.

## Verification

Test manually with:
1. `cargo build && ./target/debug/lz src/main.rs` — basic file viewing
2. `echo "hello world" | ./target/debug/lz` — stdin piping
3. `ls --color=always | ./target/debug/lz` — ANSI pass-through
4. `./target/debug/lz --follow /var/log/system.log` — follow mode
5. `head -c 1000 /dev/urandom | ./target/debug/lz` — binary detection
6. `./target/debug/lz --force /dev/null` — empty input exits immediately
7. Search: open a file, press `/`, type a pattern, verify highlighting and `n`/`N`
8. Horizontal scroll: open a file with long lines, verify Left/Right arrows
9. Wrap toggle: press `w`, verify lines wrap
10. Resize terminal while pager is open — should reflow
11. `Ctrl-Z` to suspend, `fg` to resume — terminal should restore correctly
