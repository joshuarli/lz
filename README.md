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

If no file is given and stdin is a pipe, `lz` reads from stdin. Redirected stdout copies input bytes without terminal setup, including binary and invalid UTF-8 content. It concatenates multiple files and explicit `-` stdin entries in order, de-duplicating resolved paths as less does. Interactive `:n`/`:p` switch through a CLI file list, with optional numeric counts, file-number prompts and the next filename at EOF. Boundary errors consume Enter and dispatch other acknowledgment keys. The tested numeric command prefix, including leading zeros, appears while entering a count; overflow display remains unfinished. In the tested plain-line views, searches repeat within the active filter and clearing it retains the displayed source position. The tested `!` and Ctrl-N search modifiers find non-matching lines and support forward and reverse repeat. In the tested plain-line views, Ctrl-K highlights a search without moving the view, and Ctrl-W wraps searches at file boundaries; wrap notices require acknowledgment and the setting carries into repeats and file switches. Search matches tested source control bytes when they are displayed, while ignoring the OSC payload in the tested color mode. Tested ASCII backspace overstrikes are normalized for search and mapped to their displayed highlights; non-ASCII mapping remains unfinished. Startup `--PROC-BACKSPACE` displays backspaces as controls in the tested views; `--proc-backspace` restores overstrike display. Search conversion, interactive toggling and the `-u`/`-U` overrides remain unfinished. The public pager and one-screen settings use `BackspaceDisplay`. Tested matches crossing viewport edges keep their visible highlights, including the named clipped wide-glyph cases. Tested ASCII matches reposition chopped and shifted views using the first, middle or last screen. Unicode and special-control positioning and other clipping interactions remain unfinished. A partially visible wide character uses the tested left-edge marker styling, including UI color. `--incsearch` previews the tested plain-line searches as patterns are edited; wrapped, filtered and modifier interactions remain unfinished. Tested C-locale regexes use byte matching even when `LESSCHARSET=utf8` selects UTF-8 display; other locale-specific regex behavior remains unfinished. `J`, `K` and `Y` support the tested forced line movements, including short-file boundaries. In tested plain-line views, ordinary forward movement retains a search position when EOF is already visible; wrapped rows and other boundary interactions remain unfinished. In the tested fully scanned regular-file views, `=`, Ctrl-G and `:f` report the visible line range and byte progress. The short/medium/long prompt selected by `-m`/`-M` and custom `-P` templates expands with the reference's percent escapes, conditionals and line/byte position table; an empty expansion prints a plain colon and a non-empty prompt is emphasized. `-P=` overrides the file-info prefix, `-Pm`/`-PM` the medium and long prompts. `%o` prints `?` without an OSC8 selection, and `-Ph`/`-Pw` are parsed but do not yet replace the fixed help and waiting messages. Files not yet fully scanned, streams, filters, multiple files and wrapped rows remain unfinished. Plain-line positions are remembered across visits, including EOF. In the tested file-return case, growing beyond the previous terminal height while at the first line restores the position saved on entry, as in less; other resize and wrapped-position interactions remain unfinished. `F` or `+F` follows a regular file as lines are appended; Ctrl-C or Ctrl-X returns to navigation, where `q` exits. Custom interrupts, file truncation/replacement, live pipes and continuous-producer cancellation remain unfinished. Startup file commands retain pending input, inline continuation and frame boundaries between separate commands. Their first frame preserves the oracle command text, including with -X and short/empty files. CLI `++COMMAND` runs the last configured command on every file visit, after pending one-time input; positions override saved views. LESS defaults and CLI overrides are supported, with frame boundaries between separate command entries. `E` and `:e` open the tested literal regular-file paths, retain the current view when opening fails, and obey the examine security permission. Filename expansion, changed-file reopen, binary and helper interactions remain unfinished. The public pager configuration exposes the permission, and `PagerExit::Examine` hands the requested name to the caller; `PagerExit` is no longer `Copy`. Missing-file skipping and wrapped and changed-file positions remain unfinished.

Binary files ask before alternate-screen entry. Refuse with `n` or Enter, or quit with `Q`. Detection samples the first 256 bytes and prompts after more than five binary units. ASCII and UTF-8 selection also controls filename formatting; `LESSCHARSET=utf8` selects UTF-8. SGR escapes are excluded from the sample with `-R`. Color display preserves indexed/RGB SGR across notation, overstrikes and wrapping, and discards unsupported CSI sequences. While paging, `-R` toggles color display and `-r` toggles raw display; the corresponding long name completes from a unique prefix, accepts Enter or `=`, and supports toggle, query, set, reset and quiet forms; Ctrl-P immediately after `-` suppresses the acknowledgment message. `_r` or `_R` reports the current display mode; `-+` resets and `-!` sets either option. Complete long single-row color lines containing printable UTF-8, SGR and ASCII OSC 8 hyperlinks use temporary disk storage without loading the full line into memory when there is no active search or horizontal scroll. Accepted binary, control and malformed UTF-8 bytes use visible notation. The tested control and malformed-byte notation uses the default UI color when enabled and inverse video otherwise. Tab stops participate in wrapping and can be set with `-x` or `--tabs`; multiple stops repeat their final spacing. Caret/hex notation and wide characters move intact across wrap boundaries; combining marks stay with a base at the right edge. Default backspaces replace characters or produce bold/underline overstrikes, including a preceding base with combining marks. Filename controls are also escaped in status prompts. Other charsets, customized definitions, nonstandard UTF-8 forms, processing flags, broader OSC 8 semantics, raw-mode parity and full incremental layout remain unfinished.

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

`-j` accepts a screen row or a fraction of terminal height and recalculates fractional rows on resize. In the tested startup cases, negative values resolve to the first row. Wrapped and filtered target positioning remain unfinished. Backward searches start at the bottom of the displayed screen; startup backward searches begin at EOF. Active search highlighting and direction carry to the next file in the tested cases; missing patterns request acknowledgment. `Esc-u` toggles highlighting after a matching search, and a new or repeated search turns it back on; after an unsuccessful search it reports that no previous expression is available. `Esc-U` clears the active pattern and highlighting; a repeat search can recover the last history entry. The search-start options cover the default target line; filtered views and configurable target lines remain unfinished. Search is case-sensitive by default. Regex highlighting selects the longest match at the earliest position for the tested POSIX alternatives. Highlighting preserves malformed-byte notation and the tested SGR colors and inverse attributes. Escaped ASCII letters and angle brackets follow the tested macOS POSIX behavior; escaped digits and bracket-expression backslashes are literals in the tested macOS and musl libc probes. Named bracket cases also treat ampersands and tildes as literals. The named escape and bracket cases also pass in a Linux arm64 container. Full POSIX dialect, native Linux verification, diagnostics and Unicode parity remain open. Simple `-i` and `-I` entries in `LESS` apply before the command line. `-+i` resets the case policy. Unknown and ambiguous CLI options emit diagnostics while redirected file copying continues. The library exposes the shared option catalog and long-name lookup, with distinct unknown and ambiguous errors. Option parsing stops at the first filename. Numeric and string arguments are consumed by one parser, including missing-value and overflow diagnostics. `-h` takes a number. Lines fold by default. `-S` shows an inverse right-edge marker for chopped content and toggles chopping while paging; the setting carries to the next file in the tested case. The public pager exposes that setting. The tested query, set, reset and quiet commands and the long name `--chop-long-lines` also work; other interactions remain unfinished. `-N` displays line numbers and toggles their display while paging; the setting carries to the next file in the tested case. The `-n` and `-N` option state is retained across files and can be queried or changed interactively, including with the long name `--line-numbers`. Effects of disabling line counting beyond the tested display and prompts remain unfinished. The public pager configuration uses `LineNumberMode` for these three states. The tested startup and interactive `--use-color` setting colors pager prompts, notices, line numbers, right-edge markers and tested control-byte notation and carries across files; search matches use the default highlight color, preserving the tested indexed, RGB and inverse source styles. Other escape forms, color classes and custom colors remain unfinished. The public pager and one-screen configurations carry this setting. Full option effects and Unicode case-folding parity remain open.

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

Default page movement uses one content viewport; half-page movement uses half the terminal height rounded up. A numeric `d` or `u` sets the default for both directions. A terminal size change resets that default to half the new height. `z` and `w` move forward and backward by a window; a numeric prefix updates the window used by both directions and page commands. These defaults survive file switches. Zero command prefixes retain the current default. `-zN`, `--window=N` and `-N` configure the window at startup. Positive sizes stay fixed; zero uses the terminal height and negative values subtract from it, updating after resize. A nonpositive effective window keeps the position and rings a boundary bell, at most once per wall-clock second. Plain-line aliases include `e`, Ctrl-E/Ctrl-N, `y`, Ctrl-Y/Ctrl-P/Ctrl-K, `f`, Ctrl-V and `<`/`>`. `--header=L[,C]` keeps the first L lines and C columns fixed and forces chopping in the tested views; the interactive `--header` prompt anchors rows at the current view and takes comma-separated row and column counts, with empty fields keeping the current values. Alternate explicit starts produce a diagnostic. Search targets appear below fixed rows in the tested plain-line cases; filtering, other search interactions and option query/reset remain unfinished. Public pager and one-screen configurations carry header settings, and the pager exposes the shared parser and header dimensions. Horizontal movement temporarily chops lines while shifted and restores wrapping at the left edge. Wrapped movement and custom header interactions remain unfinished.

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
