Prefix: DSLP
Next: 15

Deslop campaign backlog ([plan](https://github.com/laputa-systems/stuff/blob/main/docs/campaigns/deslop.md), tier B). One agent works this file with the `deslop` skill: take the lowest-numbered open ticket whose area does not start with `Needs owner ·`. After DSLP-5, work the open P1 and P2 findings, then fill in `## Report` below.

## Report

DSLP-1 named `2c02347` as its base revision. Ground measured the later
revision `b76d8b11869193dbea1b61ad17fef444a14122f9` on macOS ARM64
with nightly 2026-11-09. The campaign runs from seed [#4](https://github.com/joshuarli/lz/pull/4)
through Bench [#15](https://github.com/joshuarli/lz/pull/15), the latest
merged PR labeled deslop. The remaining tickets are DSLP-12, DSLP-13 and
DSLP-14, all P3. The step records below retain the measurements and limits.

**Scorecard**

| Measure | Before | After / disposition |
| --- | --- | --- |
| Tracked lines in `src` and `tests` | Ground revision: 9,560 and 615,275 | #15: 9,595 and 615,350. Counts include all tracked files and fixtures, not just Rust. Counted with `git ls-tree` and `git show` at each revision. |
| Warm `make test` | 2.47 s, 169 cases at Ground | 1.26 s, 170 cases at Tests. Host load and caching affect timings; no comparative performance verdict. |
| Coverage | 40.33% lines, 21.90% branches | 40.77% lines, 22.28% branches. Library and CLI only; parity is excluded. |
| Bounded mutation trial | One selected mutant missed | One caught, none missed. This covers one `sgr_span` guard, not the whole suite. |
| Escape hatches | 285 matching lines across `src`, `tests` and `benches` | 285 at Types, using the tools-reference expression for unsafe, allow, unwrap and casts. No strict candidate was adopted; later campaign-wide count not measured. |
| Dead code | Unreachable public methods identified | No dead implementation or unused dependency found; #12 narrowed 35 live methods to crate visibility. |
| Findings | Four P2 findings, DSLP-6 through DSLP-9 | All four fixed in #13 and #14. Three P3 benchmark gaps remain. |
| Parity gate | Diagnose: 1,054 pass, 35 mismatch | #14: all 1,089 admitted cases passed. Three parked mismatches and one needs-environment case remain outside admission. |
| Performance | Comparative gate not established | Perf gate deferred: host busy. #15 exercised selected benchmarks but claims no timing verdict; Scenario and Full tiers were not run. |
| Escaped regressions | Not measured | #13 introduced a help-mode escape regression. The parity gate caught it and #14 fixed it. |

The tests now protect rejection of OSC 52 while retaining SGR and OSC 8,
unknown Escape chords and bare Escape in normal mode, oversized CSI parameters,
closed redirected pipes, help diagnostics and help-mode Escape handling. The
verification map also identifies existing tests for terminal restoration,
search, large files and pipes. Wider mutation, native Linux execution and the
full live-pipe and follow matrix remain unverified in these step records. The
parity run has the parked and environment gaps above. The first dead-code
parity selftest failed before a repeat passed; its intermittent cause remains
unverified. Long-SGR readiness timed out in some runs under load, although it
passed the final full parity run.

**Steps and cost**

- [#4](https://github.com/joshuarli/lz/pull/4) seeded the tier B backlog. Cost: not measured.
- [#8](https://github.com/joshuarli/lz/pull/8) established Ground and the verification boundaries. Empty-target `make test` took 422.99 s, warm `make test` 2.47 s, and `make lint` 482.82 s, under the recorded host and cache conditions.
- [#9](https://github.com/joshuarli/lz/pull/9) diagnosed four P2 findings. Review cost: not measured.
- [#10](https://github.com/joshuarli/lz/pull/10) audited nine strict lint candidates and retained the existing profile. Audit cost: not measured.
- [#11](https://github.com/joshuarli/lz/pull/11) mapped test boundaries and added the OSC 52 regression. The bounded mutation attempts took about 452 runner-seconds, including an incomplete run.
- [#12](https://github.com/joshuarli/lz/pull/12) audited dead code and narrowed live method visibility. Audit cost: not measured.
- [#13](https://github.com/joshuarli/lz/pull/13) fixed DSLP-7 through DSLP-9. Implementation cost: not measured.
- [#14](https://github.com/joshuarli/lz/pull/14) fixed DSLP-6 and the help-mode regression from #13. The owner ruled the help divergence unintended; `--lz-help` retains the lz reference. Implementation and parity-run costs: not measured.
- [#15](https://github.com/joshuarli/lz/pull/15) added selective benchmark entry points and filed DSLP-12 through DSLP-14. Audit cost: not measured.

Ground used default revision `b76d8b11869193dbea1b61ad17fef444a14122f9`
on macOS ARM64 with nightly 2026-11-09. Both `make test` runs
passed 169 cases. The empty target took 422.99 s and the warm
repeat took 2.47 s. `make lint` passed in 482.82 s. The
measurement used repo-local `TMPDIR`, an isolated
`CARGO_TARGET_DIR` and two Cargo build jobs. The timings include
host load and compiler caching.

The fleet test rule passed. The macOS and both Linux musl
default jobs passed through the retained Make entry point.
[Default tests](https://github.com/joshuarli/lz/actions/runs/44477817892) ·
[Default lint](https://github.com/joshuarli/lz/actions/runs/44477818011)

The [verification guide](AGENTS.md#verification) maps the pager
boundaries, pinned Less oracle and existing benchmark workload.
Ground ran the ordinary suite. The live parity and release
performance workloads remain separate.

Diagnose reviewed `14edc95` on macOS ARM64: all 22 modules, the option
catalog and CLI parser, the pty input and escape parser, terminal
session and signal handling, byte source and line index, search and
display layout, history and LESSOPEN, tests, manifests, both backlogs
and the parity infrastructure. Lenses covered untrusted terminal and
file bytes, terminal restoration, signals, implicit state machines,
large-file behavior, errors, tests and documentation. Four defects are
filed, all P2 and all reproduced against the built pager: DSLP-6 makes
the admitted parity gate red (1054 pass, 35 mismatch), DSLP-7 loses the
reading position on any unhandled ESC chord or bare Escape, DSLP-8
panics or wraps on a long CSI parameter, and DSLP-9 turns an early
consumer close into a reported error and status 1. The redirected
binary copy differs from the reference, but AGENTS.md records it as an
intentional divergence, so it is not filed. Baseline `make
test` (169 cases) and `make lint` passed before any review work.
Reviewed and verified: a 128 MiB single-line file stays under 6 MiB RSS
at rest and 268 MiB during search, system SIGTERM exits 15 after
restoring the alternate screen, `-K` Ctrl-C exits 2, and the `-R` color
path discards OSC 52 while preserving OSC 8 and SGR. Not verified:
native Linux or musl runs, the live-pipe and follow matrix in full,
wrapped and persisted marks, tag and GLOBAL helpers, LESSOPEN/LESSCLOSE
parity beyond one malformed template, resize during every prompt, and
the release performance gate. The `perf-gate-ready` check exits 0, but
no S14 workload exists to gate. Coverage, mutation, the strict type
profile and dead-code removal remain DSLP-3 through DSLP-5.

Types audited revision `d8652b95` on macOS ARM64 with the pinned nightly.
Each candidate ran independently with `cargo clippy --workspace --all-targets
--message-format=json -- -W <candidate>`; counts below are distinct primary
source locations, deduplicated across targets. There are 12,340 first-party
Rust lines in `src`, `tests` and `benches`, so the false-positive limit is two
for this scope. No setting met the adoption rule; the existing `make lint`
profile remains the adopted profile.

| Candidate | Findings | Decision and evidence |
| --- | ---: | --- |
| `unsafe_code` | 65 | Reject: required FFI and system calls, including `readiness.rs:24` and `terminal.rs:174`; forbidding them rejects supported behavior. |
| `wildcard_enum_match_arm` | 21 | Reject: intentional dispatch fallbacks, including `pager.rs:1570` and `pager.rs:1597`; no demonstrated defect. |
| `match_wildcard_for_single_variants` | 0 | Reject: no defect or escape hatch witness. |
| `cast_possible_truncation` | 19 | Reject: more than two bounded conversions, including `buffer.rs:172`, `input.rs:56` and `options.rs:123`; no demonstrated defect. |
| `cast_sign_loss` | 14 | Reject: more than two guarded conversions, including `readiness.rs:47`, `pager.rs:679` and `tabs.rs:21`; no demonstrated defect. |
| `unwrap_used` | 30 production; 230 all targets | Reject: more than two established invariants, including nonempty UTF-8 in `charset.rs:24`, positive width in `pager.rs:496` and positive window in `pager.rs:730`. |
| `expect_used` | 1 production; 3 all targets | Reject: `search.rs:191` preserves the input UTF-8 while replacing ASCII bytes; no demonstrated defect. |
| `redundant_clone` | 4 | Reject: test-harness clones only; no typing defect or escape hatch removed. |
| `needless_pass_by_value` | 14 | Reject: no demonstrated typing defect or escape hatch removed. |

Production assertion counts come from a separate `cargo clippy --workspace
--lib --bins --message-format=json -- -W clippy::unwrap_used -W
clippy::expect_used` run. The strict candidates ran successfully at warning
level; none was adopted, so the existing lint profile is the clean profile.
The tools-reference escape-hatch expression counted 285 matching lines before
and 285 after across `src`, `tests` and `benches`. No type contract changed,
so no new negative type witness is claimed. Deferred leads, in order: the CSI
parameter overflow and wrap (DSLP-8), then the regex longest-match assertion
at `search.rs:238`, which relies on the two automata sharing an NFA and needs
dependency-level proof. DSLP-6 through DSLP-9 remain for Findings.

Tests audited fd064ac7 on macOS ARM64, nightly 2026-11-09. The verification
map names each critical contract and its tests. The ordinary renderer lacked an
OSC 52 assertion: `color_display_rejects_osc52_but_preserves_sgr_and_osc8`
checks rejection and positive SGR/OSC 8 controls. Replacing the `sgr_span`
character guard with `true` made that assertion fail (exit 101); the original
was restored. No production behavior or dependency changed.

`cargo-mutants 27.1.0`, one job, selected only that guard (required scope).
Before: 1 missed, 0 caught, 0 timeout, 0 unviable (about 153 s). Its behavior
was the OSC 52 gap, now asserted. After: 1 caught, 0 missed, 0 timeout, 0
unviable (59 s); the failing assertion was the OSC 52 check. No equivalence or
accepted-risk exclusions. A first after-run hit its 240 s outer deadline while
building; it is incomplete, not a pass. Total runner time about 452 s, under
10 minutes. Another attempt rejected incompatible copy flags before running.
Mutation tools are not mirrored, so no mutation CI lane is claimed. Wider
mutation is advisory and was not run. No false pass was demonstrated in the
mapped tests; no tests were removed. Warm `make test` took 1.26 s versus
2.47 s at ground (170 versus 169 tests). Known defects DSLP-6 through DSLP-9 stay
open. Native Linux and the full parity gate were not rerun.

`make coverage` is a report for library and CLI tests, excluding the separate
parity oracle: before 40.33% lines / 21.90% branches, after 40.77% / 22.28%.
The numbers indicate reach, not correctness. The existing parity harness
protects signal and terminal contracts that this coverage run does not measure.

Dead code audited `88bd3aaa` on macOS ARM64 and checked for Linux ARM64
musl; no Cargo features are declared. The compiler found no dead code, but
reported unreachable public methods. `storage::Offsets` methods feed
`LineBuffer` indexing; `preprocessor::Template` and `InvalidTemplate` feed
main's LESSOPEN startup and opening paths. Their visibility is now crate-local.
Parity fixture methods feed generation and reduction in `fuzz.rs`; inventory,
interactive, stdio, supervisor and shard methods feed the parity runner,
its selftests or subprocess entry points in `tests/parity/main.rs`. Their
visibility is also crate-local. No implementation, dependency, or behavior
was removed. No `allow(dead_code)` suppressions were found.

Test-only `find_matches`, `from_lines` and `parse_keys_from_bytes` feed the
search and library unit tests; buffer and history fixture helpers feed their
own test modules. The remaining test-module helpers are compiled by the
all-target checks. The option catalog is consumed by option parsing and help;
accepted less options remain a compatibility surface even when their effects
are incomplete. `cargo-machete` reports no unused dependencies; in particular,
the `laputa-ptytest` package is imported as `ptytest` by the parity runner.
Public library exports remain available to external callers. No walk-back was
proposed. DSLP-6 through DSLP-9 remain Findings work.

`cargo check --locked --workspace --all-targets` with `-W dead_code -W
unreachable_pub` passed without warnings on macOS and for
`aarch64-unknown-linux-musl`. `cargo-machete` 0.10.1, `make test` (170
tests), `make lint` and `make parity-selftest` passed. The first parity
selftest failed at the auxiliary repeat reducer; the repeat passed, so its
intermittent cause remains unverified. Reference setup required Homebrew
Python because the system Python 3.9 lacks the tar extraction filter API.

DSLP-6: Option diagnostics precede help handling; redirected `--help`
writes the less help page, and `--lz-help` retains the lz reference. The full
run at `7c86047` passed all 1089 admitted cases, with 3 parked mismatches and
1 needs-environment case. The long-SGR case passed that run and at `ca9a146f`;
other attempts timed out on the host, so their timing remains unverified.

DSLP-7: Unknown Escape chords now ring without leaving the file; bare
Escape stays in normal mode. The PTY regression covers the seven listed
chords and bare Escape; input tests check their decoded keys.

DSLP-8: CSI parameters longer than three digits are consumed as unknown;
unit regressions cover arithmetic overflow and the value that wrapped to PageUp.

DSLP-9: Redirected copying restores default SIGPIPE behavior. File and stdin
regressions verify signal termination and empty stderr after the reader closes.

## Tickets

### DSLP-12 · P3 · Measure complete pager journeys and resource growth

- **Area:** deslop · bench
- **Where:** benches/bench.rs; docs/benchmarks.md
- **Problem:** The synthetic library benchmarks do not verify completed interactive or redirected pager work, startup, memory growth, or long-line and stream behavior; regressions in those outcomes can be missed.
- **Recommendation:** Add representative end-to-end workloads with output or terminal oracles and scoped resource measurements.
- **Fix with:** refactor-benchmarks
- **Effort:** L

### DSLP-13 · P3 · Establish valid comparative performance gates and attribution

- **Area:** deslop · bench
- **Where:** benches/baseline.json; benches/driver.py; Makefile
- **Problem:** The baseline has synthetic results without interleaved reference comparisons or budgets; CPU, memory, I/O and trace attribution for pager journeys is incomplete, so a regression can go undetected.
- **Recommendation:** Define matched reference budgets and host-valid interleaved runs, then document low-overhead profiling and syscall attribution for each journey.
- **Fix with:** bench-pyramid
- **Effort:** L

### DSLP-14 · P3 · Verify benchmark cleanup and input identity

- **Area:** deslop · bench
- **Where:** benches/bench.rs; benches/driver.py
- **Problem:** Fixture cleanup is best effort and runs do not verify leftover processes or record complete artifact and host identity; stale state can obscure a regression.
- **Recommendation:** Verify cleanup and capture the minimum artifact, source and host identity needed to reproduce a run.
- **Fix with:** bench-pyramid
- **Effort:** M
