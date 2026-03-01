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

If no file is given and stdin is a pipe, `lz` reads from stdin.

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
