# less 668 parity

lz tracks the documented less 668 surface, checked against a pinned reference build. This document records the verdict, the parked set, the needs-environment debt, and how to reproduce the evidence. The parity campaign that built this infrastructure is closed; remaining gaps are documented and are not in scope to close.

## Verdict

Full tested parity is **not** claimed.

At frozen source, `make parity-test MODE=all` passes all 1089 admitted cases. The only non-passes are the parked set and one needs-environment case below. `make parity-baseline` (frozen `2ae5660`) passes 3 of 1093, with 1088 mismatches, 1 harness error and 1 needs-environment case. `make parity-inventory-check` validates anchors, upstream dispositions and full-run fingerprints; 159 Rust tests and all-target Clippy pass.

The register is the [inventory](../tests/parity/inventory.json): 385 entries — 68 implemented, 123 partial, 194 unimplemented; evidence 141 compatible in the named tested scenarios, 6 partially compatible, 237 not tested, 1 known incompatible. [Coverage](../tests/parity/coverage.md) distinguishes named-case results from complete features. The inventory's counts are the authority for "compatible": only an entry citing admitted cases that pass on the frozen source is compatible.

## How to reproduce

Run `make parity-setup` once to build the checksum-pinned POSIX reference, then `make parity-test GROUP=redirected MODE=admit` for strict offline byte, diagnostic and exit comparisons. Interactive cases set `LESSECHO` to the helper built alongside that reference unless the case overrides it. References and fixture worlds stay under `.work/`; neither reference is installed. `make parity-selftest` checks independent PTY worlds, represented screen/lifecycle state, and deliberate comparison faults. The focused `cargo test --test parity -- --checkpoint-selftest` checks checkpoint handling; paired `bell-window-start` and `bell-window-end` events require their enclosed commands to complete within one wall-clock second. `make parity-baseline` collects frozen-baseline discrepancies with eight scoped workers; each case retains its own fixture world, deadlines and cleanup, and results retain corpus order. `make parity-inventory-check` validates anchors, upstream dispositions and current full-run fingerprints. `make parity-test MODE=all` includes every current discovery case and fails on any mismatch.

Full runs fingerprint candidate and oracle binary digests plus the digests of `src/**`, `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml` and `tests/parity/**`. Results print at the end of a run; the sources must be frozen before launching a full run. Candidate and oracle-only evidence have separate pointers (`.work/parity-current`, `.work/parity-baseline`). The baseline is the frozen `2ae5660` archive under `.work/tools/` (extracted without `.git`), reused, never updated in place of the candidate.

Primary and auxiliary fixtures accept numbered-line or repeated-byte recipes; repeated bytes are generated in blocks capped at 64 KiB. File effects include permission modes; effects over 64 KiB retain a SHA-256 digest and byte length, with a same-length suffix mutation control. The `pty-pipe` adapter keeps the pager in its own foreground group and holds a producer open. Its controls verify byte argv, inherited environment, stdin, status forwarding, kernel termios and child reaping, including timeout escalation. Owned harness descendants inherit zero soft/hard core-file limits. A candidate PTY input closure is a mismatch only after confirmed child exit and successful oracle completion; retained artifacts include the failed input and exit status. SIGQUIT forwards without triggering the TERM/HUP escalation timer. Session-wide cleanup rejects leaked helper groups before the owned supervisor is reaped, with an orphan negative control. Suspend events wait for actual kernel stop/continue records, retaining the stopped screen and termios before resumption; an independent child verifies two cycles. Full kernel flags remain comparable; restoration classification excludes Darwin’s kernel-managed `PENDIN` retyping state. Mode changes preserve queued tty input, including restoration followed by reentry. Strict live-pipe cases pass for quit and TERM on an unterminated line with `-n`, and for quit with default line counting on complete lines. Default counting on an unterminated producer remains unfinished. The optional `trace_check: "plain-text"` obligation rejects terminal controls in query output, with a seeded hidden-control negative test. JSON scenarios reject unknown fields; each differential run retains observations and source/reference fingerprints under `.work/tmp/`.

`make parity-fuzz SEED=0 CASES=50` runs deterministic navigation, invalid-command, mark, forward and backward literal-search and repeat, missing-search acknowledgment, selective filtering and clearing, resize, repeated next/previous-file and quiet interactive line-number toggle interactions with selected CLI and `LESS` display options over ASCII, UTF-8 and isolated malformed-byte fixtures through the same PTY comparator. Sessions end with quit or SIGTERM. Each seed selects 40×12 or 80×24 geometry and eight actions, with independent worlds, a 10-second case deadline, a 64 MiB trace limit and a 512 MiB RSS ceiling for PTY pager children. Four scoped workers stop after the first failing batch. Each run records its generator version and source digest. Full observations are retained as gzip JSON; failures retain their seed and receive bounded reduction of events, arguments, environment entries, inline bytes, auxiliary files and repeat or numbered recipes only while the same mismatch category and oracle completion persist. Run the fuzz controls with `cargo test --test parity -- --fuzz-selftest`, or only the deterministic generation controls with `cargo test --test parity -- --generator-selftest`. Seeds 0–999 pass; the generated grammar does not yet cover every event family, and passing these generated cases does not establish the full hardening gate.

PTY runs use the same foreground-process supervisor for files and pipes. It samples the owned pager’s RSS and checks kernel peak RSS at reaping; optional `pty_rss_limit` scenario bytes can lower the 512 MiB ceiling. A candidate-only breach fails as a mismatch, while oracle breaches and measurement failures remain harness errors. Successful results retain both sides’ RSS records separately from screen comparisons. Kernel termios before setup and after exit are compared, and a child’s unrecovered modes fail before supervisor repair can hide them. The controls include a raw-terminal leak with no alternate-screen change. The same supervisor monitors a 2 GiB owned-spill ceiling by sampling writable unlinked regular-file lengths, deduplicated by device/inode. Optional `pty_spill_limit` can lower that ceiling. Deleted read-only inputs are excluded. Spill peaks are sampled, while RSS also has kernel peak accounting; fleeting files between samples are not an established peak-spill proof.

Linux arm64 container for iteration, not native proof:

```sh
docker run --rm --platform linux/arm64 -v "$PWD:/workspace" \
  -v "$HOME/.cargo/git:/root/.cargo/git" -v "$HOME/.cargo/registry:/root/.cargo/registry" \
  -w /workspace -e CARGO_TARGET_DIR=/workspace/.work/linux-target -e CARGO_NET_OFFLINE=true \
  rimage-linux:latest sh -c 'apk add --no-cache python3 musl-locales && make parity-test CASE=<id> MODE=all'
```

## Parked cases and known divergences

- `lifecycle-hup` — deliberate divergence. The oracle leaves represented terminal modes unrestored on HUP; lz restores represented modes and re-raises HUP. Never reproduce the leak; this is the inventory's one `known incompatible` entry.
- `sessions-search-toggle-no-match-next-file` and `sessions-search-toggle-match-far-below` — parked as S08q; admitted `false`; expected failures.
- `redirected-nonutf8` — needs a byte-preserving Unix filesystem (EILSEQ on this host's fixture names). Not counted against the implementation.
- The frozen baseline aborts one discovery case during its run; the harness error is recorded in [Coverage](../tests/parity/coverage.md).

## Needs-environment debt

- Native Linux x86_64 and arm64; another macOS architecture. Linux arm64 Docker is iteration evidence, not native proof.
- Quiet-host perf gates. `perf-gate-ready` exits 0 on this host, but the S14 `make parity-perf` and `make parity-close` targets do not exist, so no gate has run and the functional prerequisite is unmet.
- `redirected-nonutf8` on a byte-preserving filesystem.
- Remote CI only after a separately authorized push.

## Outstanding decisions

- S13 — `.github/workflows/ci.yml` is untouched; editing it needs an explicit owner go-ahead.
- S14 — not started.
- S15 — partial: dead-code cleanup landed; `ultrareview` was unavailable in the harness.
- Release — zips include `LESS-LICENSE` and were verified locally; nothing has been tagged or published. Publishing is owner-gated.

## Notable known gaps

Unfinished behavior is recorded per area as "remain unfinished" notes in the README, and every unimplemented inventory entry carries its cause; neither is restated here. Examples, not an exhaustive list: full help search; `-Ph`/`-Pw` are parsed but do not replace the fixed help and waiting messages; `%o` always prints `?`; ANSI-in-line prompt emphasis is not tracked; wrapped and persisted marks; multi-file/tag `%i`/`%m`/`%x` substitution; live-pipe line counting on unterminated producers; file position restoration across some resize and wrapped-row transitions.
