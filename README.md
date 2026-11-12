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
```

If no file is given and stdin is a pipe, `lz` reads from stdin. Redirected stdout copies input bytes without terminal setup, including binary and invalid UTF-8 content.

The less 668 parity campaign is in progress; interactive behavior still differs. Run `make parity-setup` once to build the checksum-pinned POSIX reference, then `make parity-test GROUP=redirected MODE=admit` for strict offline byte, diagnostic and exit comparisons. References and fixture worlds stay under `.work/`; neither reference is installed. `make parity-selftest` checks independent PTY worlds, represented screen/lifecycle state, and deliberate comparison faults. The [coverage register](tests/parity/coverage.md) distinguishes manual features from case outcomes. `make parity-baseline` collects frozen-baseline discrepancies; `make parity-inventory-check` validates anchors, upstream dispositions and current full-run fingerprints. `make parity-test MODE=all` includes every current discovery case and fails on any mismatch. The 16-case pilot passes; most inventory surfaces remain untested. JSON scenarios reject unknown fields; each differential run retains observations and source/reference fingerprints under `.work/tmp/`.

## Options

| Flag | Description |
|---|---|
| `--follow` | Follow mode (like `tail -f`) |
| `--force` | View binary files |
| `-r`, `--raw` | Show ANSI escapes literally |
| `-h`, `--help` | Show help |
| `-V`, `--version` | Show version |

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
