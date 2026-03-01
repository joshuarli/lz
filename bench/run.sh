#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
LZ="$PROJECT_ROOT/target/release/lz"
LESS="$(command -v less)"
TMPDIR_BASE="${TMPDIR:-/tmp}"
WORKDIR="$(mktemp -d "$TMPDIR_BASE/lz-bench.XXXXXX")"
LINES=${1:-1000000}
RUNS=5
WARMUP=2

# Pagers set raw mode with TCSAFLUSH which discards pending input.
# We need a brief sleep after spawn so the pager finishes setup before
# we send any keystrokes. This constant overhead applies equally to both
# pagers and cancels out in comparison.
INIT_DELAY=0.3

cleanup() { rm -rf "$WORKDIR"; }
trap cleanup EXIT

# --- pre-flight checks ---
for cmd in expect hyperfine; do
  command -v "$cmd" >/dev/null || { echo "error: $cmd not found"; exit 1; }
done
[[ -x "$LZ" ]] || { echo "error: $LZ not found — run: cargo build --release"; exit 1; }

# --- generate test data ---
echo "generating $LINES-line test file…"
awk -v n="$LINES" 'BEGIN {
  for (i = 1; i <= n; i++)
    printf "%07d | the quick brown fox jumps over the lazy dog abcdef\n", i
}' > "$WORKDIR/data.txt"
echo "  $(wc -c < "$WORKDIR/data.txt" | tr -d ' ') bytes"

# --- write expect scripts ---
DATAFILE="$WORKDIR/data.txt"

# Scenario 1: startup + quit
cat > "$WORKDIR/lz_quit.exp" <<EOF
set timeout 30
spawn $LZ $DATAFILE
sleep $INIT_DELAY
send "q"
expect eof
EOF

cat > "$WORKDIR/less_quit.exp" <<EOF
set timeout 30
spawn $LESS $DATAFILE
sleep $INIT_DELAY
send "q"
expect eof
EOF

# Scenario 2: jump to end
cat > "$WORKDIR/lz_jump_end.exp" <<EOF
set timeout 60
spawn $LZ $DATAFILE
sleep $INIT_DELAY
send "G"
sleep 1
send "q"
expect eof
EOF

cat > "$WORKDIR/less_jump_end.exp" <<EOF
set timeout 60
spawn $LESS $DATAFILE
sleep $INIT_DELAY
send "G"
sleep 1
send "q"
expect eof
EOF

# Scenario 3: search (hit) — pattern near middle of file
cat > "$WORKDIR/lz_search_hit.exp" <<EOF
set timeout 60
spawn $LZ $DATAFILE
sleep $INIT_DELAY
send "/0500000\r"
sleep 1
send "q"
expect eof
EOF

cat > "$WORKDIR/less_search_hit.exp" <<EOF
set timeout 60
spawn $LESS $DATAFILE
sleep $INIT_DELAY
send "/0500000\n"
sleep 1
send "q"
expect eof
EOF

# Scenario 4: search (miss) — pattern does not exist
cat > "$WORKDIR/lz_search_miss.exp" <<EOF
set timeout 120
spawn $LZ $DATAFILE
sleep $INIT_DELAY
send "/zzznomatch\r"
sleep 2
send "q"
expect eof
EOF

cat > "$WORKDIR/less_search_miss.exp" <<EOF
set timeout 120
spawn $LESS $DATAFILE
sleep $INIT_DELAY
send "/zzznomatch\n"
sleep 2
send "q"
expect eof
EOF

# Scenario 5: page-through 1000 pages
# Send space in batches of 50 with small delays to avoid pty buffer overflow.
PAGE_KEYS=""
for ((i = 0; i < 20; i++)); do
  PAGE_KEYS+='send "                                                  "
after 50
'
done

cat > "$WORKDIR/lz_page_1000.exp" <<EOF
set timeout 120
spawn $LZ $DATAFILE
sleep $INIT_DELAY
${PAGE_KEYS}sleep 1
send "q"
expect eof
EOF

cat > "$WORKDIR/less_page_1000.exp" <<EOF
set timeout 120
spawn $LESS $DATAFILE
sleep $INIT_DELAY
${PAGE_KEYS}sleep 1
send "q"
expect eof
EOF

# --- run benchmarks ---
echo ""
echo "=== lz vs less benchmarks ($LINES lines) ==="
echo ""

scenarios=("quit" "jump_end" "search_hit" "search_miss" "page_1000")
labels=(
  "Startup + quit"
  "Jump to end (G)"
  "Search (hit)"
  "Search (miss)"
  "Page-through 1000 pages"
)

for i in "${!scenarios[@]}"; do
  s="${scenarios[$i]}"
  label="${labels[$i]}"
  echo "--- $label ---"
  hyperfine \
    --warmup "$WARMUP" \
    --runs "$RUNS" \
    --command-name "lz" "expect $WORKDIR/lz_${s}.exp" \
    --command-name "less" "expect $WORKDIR/less_${s}.exp" \
    2>&1
  echo ""
done

echo "done."
