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

If no file is given and stdin is a pipe, `lz` reads from stdin. Redirected stdout copies input bytes without terminal setup, including binary and invalid UTF-8 content. It concatenates multiple files and explicit `-` stdin entries in order, de-duplicating resolved paths as less does. Interactive multiple-file sessions remain unimplemented.

The less 668 parity campaign is in progress; interactive behavior still differs. Run `make parity-setup` once to build the checksum-pinned POSIX reference, then `make parity-test GROUP=redirected MODE=admit` for strict offline byte, diagnostic and exit comparisons. References and fixture worlds stay under `.work/`; neither reference is installed. `make parity-selftest` checks independent PTY worlds, represented screen/lifecycle state, and deliberate comparison faults. The [coverage register](tests/parity/coverage.md) distinguishes manual features from case outcomes. `make parity-baseline` collects frozen-baseline discrepancies; `make parity-inventory-check` validates anchors, upstream dispositions and current full-run fingerprints. `make parity-test MODE=all` includes every current discovery case and fails on any mismatch. All executable admitted cases pass; the raw-byte filename fixture needs a filesystem that can represent it. Most inventory surfaces remain untested. JSON scenarios reject unknown fields; each differential run retains observations and source/reference fingerprints under `.work/tmp/`.

## Options

| Flag | Description |
|---|---|
| `+F` | Start in follow mode (like `tail -f`) |
| `-f`, `--force` | View binary files |
| `-i`, `--ignore-case` | Use smart case for ASCII search |
| `-I`, `--IGNORE-CASE` | Ignore ASCII search case |
| `-r`, `--raw` | Show ANSI escapes literally |
| `-?`, `--help` | Show help |
| `-V`, `--version` | Show version |

Search is case-sensitive by default. Simple `-i` and `-I` entries in `LESS` apply before the command line. `-+i` resets the case policy. Unknown and ambiguous CLI options emit diagnostics while redirected file copying continues. Option parsing stops at the first filename. Numeric and string arguments are consumed by one parser, including missing-value and overflow diagnostics. `-h` takes a number. Full option effects and Unicode case-folding parity remain open.

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
