# lz

A minimal, fast pager — like `less`, but smaller.

Two dependencies (`regex-lite`, `unicode-width`). No crossterm, no termion, no clap. Unix only.

## Install

```
cargo install --path .
```

## Usage

```
lz [OPTIONS] [FILE]
cat file.txt | lz
lz +120 file.txt
lz -p pattern file.txt
```

If no file is given and stdin is a pipe, `lz` reads from stdin. Redirected stdout copies input bytes without terminal setup, including binary and invalid UTF-8 content. It concatenates multiple files and explicit `-` stdin entries in order, de-duplicating resolved paths as less does. Interactive `:n`/`:p` switch through a CLI file list, with file-number prompts and the next filename at EOF. Counts, boundary errors, remembered positions, examine and binary-file transitions remain unfinished.

Binary files ask before alternate-screen entry. Refuse with `n` or Enter, or quit with `Q`. Detection samples the first 256 bytes and prompts after more than five binary units. ASCII and UTF-8 selection also controls filename formatting. SGR escapes are excluded from the sample with `-R`. Color display preserves indexed/RGB SGR across notation, overstrikes and wrapping, and discards unsupported CSI sequences. Accepted and forced binary bytes display in inverse notation, as do control and malformed UTF-8 bytes. Default tab stops participate in wrapping. Caret/hex notation and wide characters move intact across wrap boundaries. Default backspaces replace characters or produce bold/underline overstrikes, including a preceding base with combining marks. Filename controls are also escaped in status prompts. Other charsets, customized definitions, nonstandard UTF-8 forms, processing flags, OSC 8 semantics, raw-mode parity and full incremental layout remain unfinished.

Oversized numeric prefixes show a dismissible error. Enter resumes the pending command without losing the rest of a startup command.

Empty files keep an interactive EOF view. Initial short-file content appears at the bottom, with unused rows blank. An explicit startup position uses a full paint.

Initial display reads bounded line prefixes from disk-backed source storage. Streamed input and large line indexes use owned anonymous spill files. Source bytes are retained intact. Whole-line search, wrapped navigation and exact malformed-byte display remain unfinished.

History uses `LESSHISTFILE` or the less 668 XDG/home search order, reusing an existing destination before choosing a new one. The old `$XDG_STATE_HOME/lz/history` file is neither read nor migrated. Search and shell records merge at save time with atomic replacement and mode 0600. `LESSHISTSIZE` controls retained older entries. Marks, concurrent/interrupted writes, temporary-name collisions, secure allowances and full command editing remain unfinished.

The less 668 parity campaign is in progress; interactive behavior still differs. Run `make parity-setup` once to build the checksum-pinned POSIX reference, then `make parity-test GROUP=redirected MODE=admit` for strict offline byte, diagnostic and exit comparisons. References and fixture worlds stay under `.work/`; neither reference is installed. `make parity-selftest` checks independent PTY worlds, represented screen/lifecycle state, and deliberate comparison faults. The [coverage register](tests/parity/coverage.md) distinguishes manual features from case outcomes. `make parity-baseline` collects frozen-baseline discrepancies with two scoped workers; each case retains its own fixture world, deadlines and cleanup, and results retain corpus order. `make parity-inventory-check` validates anchors, upstream dispositions and current full-run fingerprints. `make parity-test MODE=all` includes every current discovery case and fails on any mismatch. All executable admitted cases pass; the raw-byte filename fixture needs a filesystem that can represent it. Most inventory surfaces remain untested. Fixture recipes generate repeated bytes in blocks capped at 64 KiB. File effects include permission modes; effects over 64 KiB retain a SHA-256 digest and byte length, with a same-length suffix mutation control. The `pty-pipe` adapter keeps the pager in its own foreground group and holds a producer open. Its controls verify byte argv, inherited environment, stdin, status forwarding, kernel termios and child reaping, including timeout escalation. Full kernel flags remain comparable; restoration classification excludes Darwin’s kernel-managed `PENDIN` retyping state. Mode changes preserve queued tty input, including restoration followed by reentry. Strict live-pipe cases pass for quit and TERM on an unterminated line with `-n`, and for quit with default line counting on complete lines. Default counting on an unterminated producer remains unfinished. The optional `trace_check: "plain-text"` obligation rejects terminal controls in query output, with a seeded hidden-control negative test. JSON scenarios reject unknown fields; each differential run retains observations and source/reference fingerprints under `.work/tmp/`.

## Options

| Flag | Description |
|---|---|
| `+F` | Start in follow mode (like `tail -f`) |
| `-f`, `--force` | View binary files |
| `-i`, `--ignore-case` | Use smart case for ASCII search |
| `-I`, `--IGNORE-CASE` | Ignore ASCII search case |
| `-S`, `--chop-long-lines` | Chop long lines instead of folding |
| `-N` | Show line numbers |
| `-r`, `--raw` | Show ANSI escapes literally |
| `-?`, `--help` | Show help |
| `-V`, `--version` | Show version |

Search is case-sensitive by default. Simple `-i` and `-I` entries in `LESS` apply before the command line. `-+i` resets the case policy. Unknown and ambiguous CLI options emit diagnostics while redirected file copying continues. Option parsing stops at the first filename. Numeric and string arguments are consumed by one parser, including missing-value and overflow diagnostics. `-h` takes a number. Lines fold by default. `-S` shows an inverse right-edge marker for chopped content. `-N` displays line numbers. Full option effects and Unicode case-folding parity remain open.

## Keyboard Shortcuts

### Navigation

| Key | Action |
|---|---|
| `j` / `Down` / `Enter` | Scroll down one line |
| `k` / `Up` | Scroll up one line |
| `d` / `Ctrl-D` | Half page down |
| `u` / `Ctrl-U` | Half page up |
| `Space` / `PgDn` | Page down |
| `b` / `PgUp` | Page up |
| `g` / `Home` | Go to top |
| `G` / `End` | Go to bottom |
| `Left` / `Right` | Horizontal scroll |

### Search

| Key | Action |
|---|---|
| `/` | Search forward |
| `?` | Search backward |
| `n` | Next match |
| `N` | Previous match |
| `&` | Filter lines by pattern |

### Other

| Key | Action |
|---|---|
| `w` | Toggle line wrap |
| `F` | Follow mode (`tail -f`) |
| `h` | Toggle help screen |
| `q` / `Esc` | Quit |
