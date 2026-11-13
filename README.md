# lz

A minimal, fast pager — like `less`, but smaller.

Two direct dependencies (`regex-automata`, `unicode-width`); the regex parser adds `regex-syntax`. No crossterm, no termion, no clap. Unix only.

## Install

```
cargo install --path .
```

## Usage

```
lz [OPTIONS] [FILE]
cat file.txt | lz
lz +120 file.txt
lz ++20g first.txt second.txt
lz -p pattern file.txt
```

If no file is given and stdin is a pipe, `lz` reads from stdin. Redirected stdout copies input bytes without terminal setup, including binary and invalid UTF-8 content. It concatenates multiple files and explicit `-` stdin entries in order, de-duplicating resolved paths as less does. Interactive `:n`/`:p` switch through a CLI file list, with optional numeric counts, file-number prompts and the next filename at EOF. Boundary errors consume Enter and dispatch other acknowledgment keys. Plain-line positions are remembered across visits, including EOF. `F` or `+F` follows a regular file as lines are appended; Ctrl-C or Ctrl-X returns to navigation, where `q` exits. Custom interrupts, file truncation/replacement, live pipes and continuous-producer cancellation remain unfinished. Startup file commands retain pending input, inline continuation and frame boundaries between separate commands. Their first frame preserves the oracle command text, including with -X and short/empty files. CLI `++COMMAND` runs the last configured command on every file visit, after pending one-time input; positions override saved views. LESS defaults and CLI overrides are supported, with frame boundaries between separate command entries. Missing-file skipping, wrapped and changed-file positions, examine and binary-file transitions remain unfinished.

Binary files ask before alternate-screen entry. Refuse with `n` or Enter, or quit with `Q`. Detection samples the first 256 bytes and prompts after more than five binary units. ASCII and UTF-8 selection also controls filename formatting. SGR escapes are excluded from the sample with `-R`. Color display preserves indexed/RGB SGR across notation, overstrikes and wrapping, and discards unsupported CSI sequences. While paging, `-R` toggles color display and `-r` toggles raw display; the corresponding long name completes from a unique prefix, accepts Enter or `=`, and supports toggle, query, set, reset and quiet forms; Ctrl-P immediately after `-` suppresses the acknowledgment message. `_r` or `_R` reports the current display mode; `-+` resets and `-!` sets either option. Complete long single-row color lines containing printable UTF-8, SGR and ASCII OSC 8 hyperlinks use temporary disk storage without loading the full line into memory when there is no active search or horizontal scroll. Accepted and forced binary bytes display in inverse notation, as do control and malformed UTF-8 bytes. Tab stops participate in wrapping and can be set with `-x` or `--tabs`; multiple stops repeat their final spacing. Caret/hex notation and wide characters move intact across wrap boundaries; combining marks stay with a base at the right edge. Default backspaces replace characters or produce bold/underline overstrikes, including a preceding base with combining marks. Filename controls are also escaped in status prompts. Other charsets, customized definitions, nonstandard UTF-8 forms, processing flags, broader OSC 8 semantics, raw-mode parity and full incremental layout remain unfinished.

Press `h` or `H` to view help. Line, page, half-page and numbered navigation work within it; `q`, `Q`, `:q`, `:Q` or `ZZ` returns to the file. Searching and other help commands remain unfinished. The help text is from less 668, copyright Mark Nudelman, under the [Less License](LESS-LICENSE).

Ordinary in-file marks use `m` plus a letter and `'` plus that letter. Pressing the apostrophe key twice returns to the last large movement; `'^` and `'$` go to file boundaries. Wrapped positions, cross-file marks and persisted marks remain unfinished.

Startup `-t` selects a numeric or literal-prefix ctags address from `tags`; `-T` chooses another tags file. Pattern delimiters, backslash removal, optional end-of-line matching, raw bytes, extended records and the oracle’s secure-startup behavior are covered. Tag matching retains CR bytes while display normalizes CRLF. Duplicate matches, interactive tags, tag error prompts, color conversion, long-running cancellation and GLOBAL helpers remain unfinished.

`-~`/`--tilde` leaves lines after EOF blank instead of displaying tildes.

`-q` uses a visual bell at screen boundaries; `-Q` uses it for other feedback too. `--no-vbell` suppresses visual feedback while preserving any audible feedback selected by `-q`.

Default SIGINT and Ctrl-C ring the bell and continue paging; normal-mode Ctrl-C no longer exits. Interrupts cancel pending input, preserve follow-mode return context and use the same path for signals and keyboard input. Binary and diagnostic prompts also consume SIGINT and restore their acquired state on exit. `-K`/`--quit-on-intr` exits with status 2 on interruption, including pending input and follow mode. Ctrl-X exits follow mode silently under that policy. Binary refusal retains its own status; interrupted diagnostic acknowledgment honors the policy. Pending search/filter drafts are saved to history before exit. LESS defaults and CLI resets are supported. SIGQUIT is ignored during interactive paging and primary prompts. External suspension restores terminal modes and resumes with the acquired screen/keypad options, including `-X` and `--no-keypad`. Repeated suspension and scrolled navigation are covered. Unsolicited SIGCONT preserves the current prompt and saved terminal state. Nonempty `LESSSECURE`, including `0`, ignores external suspension unless `LESSSECURE_ALLOW` grants `stop` (unique `st` prefix accepted). The policy also applies before binary acknowledgment. Ctrl-Z uses the inherited kernel signal-character setting and follows that stop policy. The keyboard quit character is ignored like SIGQUIT. Repeated keyboard suspension and every screen/keypad combination have named cases. Custom interrupt characters and long-operation cancellation remain unfinished.

Normal mode accepts `q`, `Q`, `:q`, `:Q` and `ZZ` to quit; numeric counts are ignored. A pending `Z` or colon command displays its prefix. Backspace, Ctrl-C and invalid second keys cancel the `Z` prefix with the reference bell.

Oversized numeric prefixes show a dismissible error. Enter resumes the pending command without losing the rest of a startup command.

An exact-viewport final line gets bounded source lookahead so its EOF prompt is shown. Empty files keep an interactive EOF view. Initial short-file content appears at the bottom of the alternate screen, with unused rows blank. An explicit startup position uses a full paint.

`-X`/`--no-init` preserves the primary screen, places a short file below the current cursor and clears its prompt on exit. `--no-keypad` independently suppresses application keypad and cursor modes. Both settings accept LESS defaults and CLI resets. Nondefault modes during suspend/resume, resizing, help and complex file transitions still need differential coverage.

`-F` exits after displaying a single file that fits on one screen. With `-R`, long runs of SGR color sequences and single-row printable UTF-8 text use bounded memory; other long layout forms remain unfinished.

Initial display reads bounded line prefixes from disk-backed source storage. An open pipe waits for initial data or EOF before painting its first view; keyboard and signal handling remain active. Streamed input and large line indexes use owned anonymous spill files. Source bytes are retained intact. Whole-line search, wrapped navigation and exact malformed-byte display remain unfinished.

PTY pipe fixtures accept `LZ_PARITY_PRODUCER_DELAY_MS` from 0 through 1000 to delay the owned producer’s first write. The delayed-startup regression compares the first screen against the oracle without changing readiness or output limits.

`LESSOPEN` supports file-return preprocessors with one `%s` filename substitution and literal `%%`. Empty output keeps the original file; the first output line selects a replacement while the pager retains the original display name. Filename arguments are shell-quoted. `-L`/`--no-lessopen` disables execution, and secure mode requires the `lessopen` allowance. Each helper receives temporary foreground tty access and is reaped before paging; TERM cancels its owned process group. Malformed initial file-return templates print the reference warning and await acknowledgment before opening the original file. Quit, interruption, TERM and combined secure diagnostics share the existing prompt policy. A leading `-` in `LESSOPEN` enables execution and validation for stdin. The file-return form keeps stdin as the data source, so helper reads consume stdin; the returned filename is unused. File/stdin switches retain the policy and file-count prompt. Later file diagnostics, live-pipe preprocessors, postprocessors and other helper controls remain unfinished.

History uses `LESSHISTFILE` or the less 668 XDG/home search order, reusing an existing destination before choosing a new one. The old `$XDG_STATE_HOME/lz/history` file is neither read nor migrated. Search and shell records merge at save time with atomic replacement and mode 0600. `LESSHISTSIZE` controls retained older entries. `--no-histdups` moves an accepted search pattern to the newest position for recall; filename and shell history behavior remains unfinished. History obeys the resolved `LESSSECURE`/`LESSSECURE_ALLOW` policy, including unique feature-name prefixes and diagnostics. Marks, concurrent/interrupted writes, temporary-name collisions, other secure feature effects and full command editing remain unfinished.

The less 668 parity campaign is in progress; interactive behavior still differs. Run `make parity-setup` once to build the checksum-pinned POSIX reference, then `make parity-test GROUP=redirected MODE=admit` for strict offline byte, diagnostic and exit comparisons. References and fixture worlds stay under `.work/`; neither reference is installed. `make parity-selftest` checks independent PTY worlds, represented screen/lifecycle state, and deliberate comparison faults. The [coverage register](tests/parity/coverage.md) distinguishes manual features from case outcomes. `make parity-baseline` collects frozen-baseline discrepancies with eight scoped workers; each case retains its own fixture world, deadlines and cleanup, and results retain corpus order. `make parity-inventory-check` validates anchors, upstream dispositions and current full-run fingerprints. `make parity-test MODE=all` includes every current discovery case and fails on any mismatch. All executable admitted cases pass; the raw-byte filename fixture needs a filesystem that can represent it. Most inventory surfaces remain untested. Primary and auxiliary fixtures accept numbered-line or repeated-byte recipes; repeated bytes are generated in blocks capped at 64 KiB. File effects include permission modes; effects over 64 KiB retain a SHA-256 digest and byte length, with a same-length suffix mutation control. The `pty-pipe` adapter keeps the pager in its own foreground group and holds a producer open. Its controls verify byte argv, inherited environment, stdin, status forwarding, kernel termios and child reaping, including timeout escalation. Owned harness descendants inherit zero soft/hard core-file limits. A candidate PTY input closure is a mismatch only after confirmed child exit and successful oracle completion; retained artifacts include the failed input and exit status. SIGQUIT forwards without triggering the TERM/HUP escalation timer. Session-wide cleanup rejects leaked helper groups before the owned supervisor is reaped, with an orphan negative control. Suspend events wait for actual kernel stop/continue records, retaining the stopped screen and termios before resumption; an independent child verifies two cycles. Full kernel flags remain comparable; restoration classification excludes Darwin’s kernel-managed `PENDIN` retyping state. Mode changes preserve queued tty input, including restoration followed by reentry. Strict live-pipe cases pass for quit and TERM on an unterminated line with `-n`, and for quit with default line counting on complete lines. Default counting on an unterminated producer remains unfinished. The optional `trace_check: "plain-text"` obligation rejects terminal controls in query output, with a seeded hidden-control negative test. JSON scenarios reject unknown fields; each differential run retains observations and source/reference fingerprints under `.work/tmp/`.

`make parity-fuzz SEED=0 CASES=50` runs deterministic navigation, invalid-command, mark, forward literal-search, resize, next-file and quiet interactive line-number toggle interactions with selected CLI and `LESS` display options over ASCII, UTF-8 and isolated malformed-byte fixtures through the same PTY comparator. Sessions end with quit or SIGTERM. Each seed selects 40×12 or 80×24 geometry and eight actions, with independent worlds, a 10-second case deadline, a 64 MiB trace limit and a 512 MiB RSS ceiling for PTY pager children. Four scoped workers stop after the first failing batch. Each run records its generator version and source digest. Full observations are retained as gzip JSON; failures retain their seed and receive bounded event, argument, inline-fixture, explicit-environment, auxiliary-file and inline auxiliary-byte and primary repeat-count reduction only while the same mismatch category and oracle completion persist. `make parity-selftest` proves seed determinism, budget rejection, compression round-trip and reduction of an injected attribute fault. Reduction reports include attempts and counts for explicit environment variables, auxiliary files and their inline bytes, and primary repeat counts. Candidate and oracle-only evidence have separate pointers.

The generated grammar covers selected navigation, search, display options, text, resize, next-file and termination interactions. Broader CLI, configuration, stream, signal and file-transition grammars, the full 70/30 boundary mix, reduction of numbered and auxiliary fixture recipes, stdio/helper/producer resource enforcement, commit-derived seeds and holdouts remain unfinished. Passing these generated cases does not establish the full hardening gate.

PTY runs now use the same foreground-process supervisor for files and pipes. It samples the owned pager’s RSS and checks kernel peak RSS at reaping; optional `pty_rss_limit` scenario bytes can lower the 512 MiB ceiling. A candidate-only breach fails as a mismatch, while oracle breaches and measurement failures remain harness errors. Successful results retain both sides’ RSS records separately from screen comparisons. Kernel termios before setup and after exit are compared, and a child’s unrecovered modes fail before supervisor repair can hide them. The controls include a raw-terminal leak with no alternate-screen change. The same supervisor monitors a 2 GiB owned-spill ceiling by sampling writable unlinked regular-file lengths, deduplicated by device/inode. Optional `pty_spill_limit` can lower that ceiling. Deleted read-only inputs are excluded. Spill peaks are sampled, while RSS also has kernel peak accounting; fleeting files between samples are not an established peak-spill proof.

## Options

| Flag | Description |
|---|---|
| `+F` | Start in follow mode (like `tail -f`) |
| `-a`, `--search-skip-screen` | Start searches beyond the displayed screen |
| `-A`, `--SEARCH-SKIP-SCREEN` | Start searches beyond the target line |
| `-xN,...`, `--tabs=N,...` | Set tab stops |
| `-jN`, `--jump-target=N` | Set the screen row for line jumps and searches |
| `-#N`, `--shift=N` | Set the horizontal step in columns or as a screen-width fraction |
| `--line-num-width=N` | Set line-number width from 0 to 16 |
| `-f`, `--force` | View binary files |
| `-i`, `--ignore-case` | Use smart case for ASCII search |
| `-I`, `--IGNORE-CASE` | Ignore ASCII search case |
| `-S`, `--chop-long-lines` | Chop long lines instead of folding |
| `-N` | Show line numbers |
| `-r`, `--raw` | Show ANSI escapes literally |
| `-?`, `--help` | Open help on a terminal or write it to redirected output; `:n` opens a registered file |
| `-V`, `--version` | Show version |

`-j` accepts a screen row or a fraction of terminal height and recalculates fractional rows on resize. In the tested startup cases, negative values resolve to the first row. Wrapped and filtered target positioning remain unfinished. Backward searches start at the bottom of the displayed screen; startup backward searches begin at EOF. Active search highlighting and direction carry to the next file in the tested cases; missing patterns request acknowledgment. `Esc-u` toggles highlighting after a matching search, and a new or repeated search turns it back on; after an unsuccessful search it reports that no previous expression is available. `Esc-U` clears the active pattern and highlighting; a repeat search can recover the last history entry. The search-start options cover the default target line; filtered views and configurable target lines remain unfinished. Search is case-sensitive by default. Regex highlighting selects the longest match at the earliest position for the tested POSIX alternatives. Highlighting preserves malformed-byte notation and the tested SGR colors and inverse attributes. Escaped ASCII letters and angle brackets follow the tested macOS POSIX behavior; escaped digits and bracket-expression backslashes are literals in the tested macOS and musl libc probes. Named bracket cases also treat ampersands and tildes as literals. The named escape and bracket cases also pass in a Linux arm64 container. Full POSIX dialect, native Linux verification, diagnostics and Unicode parity remain open. Simple `-i` and `-I` entries in `LESS` apply before the command line. `-+i` resets the case policy. Unknown and ambiguous CLI options emit diagnostics while redirected file copying continues. The library exposes the shared option catalog and long-name lookup, with distinct unknown and ambiguous errors. Option parsing stops at the first filename. Numeric and string arguments are consumed by one parser, including missing-value and overflow diagnostics. `-h` takes a number. Lines fold by default. `-S` shows an inverse right-edge marker for chopped content and toggles chopping while paging; the setting carries to the next file in the tested case. The public pager exposes that setting. The tested query, set, reset and quiet commands and the long name `--chop-long-lines` also work; other interactions remain unfinished. `-N` displays line numbers and toggles their display while paging; the setting carries to the next file in the tested case. The `-n` and `-N` option state is retained across files and can be queried or changed interactively, including with the long name `--line-numbers`. Effects of disabling line counting beyond the tested display and prompts remain unfinished. The public pager configuration uses `LineNumberMode` for these three states. Full option effects and Unicode case-folding parity remain open.

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
| `Left` / `Right`, `Esc-(` / `Esc-)` | Scroll horizontally by half the screen; a count sets the step |
| `Esc-{` | Return to the first column |
| `m` + letter / `'` + letter | Set / return to an in-file mark |
| `r` / `Ctrl-R` / `Ctrl-L` | Repaint the screen |
| `R` | Reload a regular file and repaint |

Default page movement uses one content viewport; half-page movement uses half the terminal height rounded up. A numeric `d` or `u` sets the default for both directions. A terminal size change resets that default to half the new height. `z` and `w` move forward and backward by a window; a numeric prefix updates the window used by both directions and page commands. These defaults survive file switches. Zero command prefixes retain the current default. `-zN`, `--window=N` and `-N` configure the window at startup. Positive sizes stay fixed; zero uses the terminal height and negative values subtract from it, updating after resize. A nonpositive effective window keeps the position and rings a boundary bell, at most once per wall-clock second. Plain-line aliases include `e`, Ctrl-E/Ctrl-N, `y`, Ctrl-Y/Ctrl-P/Ctrl-K, `f`, Ctrl-V and `<`/`>`. Horizontal movement temporarily chops lines while shifted and restores wrapping at the left edge. Wrapped movement and custom header interactions remain unfinished.

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
| `z` / `w` | Forward / backward window; numeric prefixes set the default |
| `F` | Follow mode (`tail -f`) |
| `h` | Toggle help screen |
| `q` / `Q` / `:q` / `:Q` / `ZZ` | Quit |
