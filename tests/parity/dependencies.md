# Parity test dependencies

The production graph is `regex-automata` 0.4.18, its parser
`regex-syntax` 0.8.11, and `unicode-width` 0.2.2. The first two are
MIT OR Apache-2.0 crates from the Rust regex project. Their local manifests,
licenses, PikeVM implementation and source identity were inspected. The
automata crate records source commit `e6ad938198e68339a593f85d228f2d44cb54718f`.
The replacement is pinned and builds offline from the existing cache.

`regex-lite` cannot expose the all-match execution needed for POSIX
leftmost-longest highlighting. A portable standard-library implementation
does not provide regexes. Two PikeVM policies share one compiled NFA: locate
the earliest start, then find the longest match anchored there. This avoids
writing another regex compiler or adding a system C regex dependency.
Default features, DFA engines and literal accelerator dependencies are
disabled; enabled features are std, syntax, nfa-pikevm, unicode-case,
unicode-perl and unicode-word-boundary. `cargo tree --edges normal` shows
only the three runtime packages above. The lock has 65 packages, including
inactive optional and target-only packages. `regex-lite` remains needed by
test-only rustybench.

On macOS arm64, the S08a dist build grew from 268,992 to 533,440 bytes.
Both use the pinned toolchain and the repository dist recipe. The result
remains below the plan's 2 MiB budget. One version-query RSS observation
was 1,998,848 bytes before and 2,113,536 after; this is not proof for
interactive or large-input memory gates. Logs and binaries are retained
under `.work/tmp/s08a-*`. Clean-build, startup and runtime timing gates
are deferred: `perf-gate-ready` rejected a hot process tree in another
project. No timing result from that interval counts as a pass.
S14's complete workflow, resource and performance gates remain open.

The S02 runner pins MIT-licensed `laputa-ptytest` 0.4.0 to commit
`2efca1cffe9f4f19bddb7208643c18de63bbbc41` (2026-11-11). Its Unix process-group
cleanup, independent VT model, resize, signal delivery and condition waits avoid
implementing another PTY framework. The pin's source and consumer selftests were
inspected locally; changing it requires the plan's regression and dependency checks.

The lock adds five packages: `laputa-ptytest`, `rustc-hash` 2.1.3,
`vt100` 0.18.1, `vte` 0.15.0 and `arrayvec` 0.7.8. Their manifests declare
MIT, Apache-2.0, or a choice of these licenses. `libc` 0.2.182 was already
locked and remains test-only. S12b makes that same version a direct test dependency
for the platform rusage ABI, wait4, page-size constants and Darwin process usage.
Its MIT/Apache-2.0 source declarations were checked against the local SDK. No
package or production dependency is added by that promotion. `itoa` advances from 1.0.17 to 1.0.19 to meet
vt100's requirement. The full lock has 62 packages, including target-only ones,
versus 57 before S02.

`serde` 1.0.228 and `serde_json` 1.0.149 were already locked by rustybench;
they are now direct test dependencies. Their MIT/Apache-2.0 manifests and available
source were inspected. Typed JSON with unknown-field rejection keeps scenarios
as data and avoids a handwritten JSON parser. Neither reaches production.

On macOS arm64, a fresh test target directory built the runner with kache enabled
in 2.81 seconds elapsed (0.93 user, 0.78 system), one observation, not an uncached
compiler or product performance claim. Both `perf-gate-ready` checks exited 0.
Command: `CARGO_NET_OFFLINE=true RUSTC_WRAPPER=kache /usr/bin/time -p cargo test
--test parity --no-run --target-dir .work/tmp/s02-clean-target`. The timing log
is `.work/tmp/s02-build-time.txt`. S14's product timing and size gates remain open.

The generated navigation runner adds no Cargo dependency. It uses the host
`gzip` tool to retain complete observations without accumulating uncompressed
screen snapshots. A scoped child owner closes compression input, checks exit,
and reaps on errors; the selftest verifies decompression round-trip. The same
four-worker collector retains case order and drains each in-flight batch before
stopping on failure.

The pinned PTY API drains continuously available bytes until its deadline.
Readiness therefore uses public `wait_for_output` calls in bounded 20 ms slices
before checking the semantic predicate. Once ready, raw output must remain quiet
for 20 ms within a 250 ms settling bound and the original checkpoint deadline.
A continuously redrawing program still fails; its typed readiness/quiescence
timeout retains input, output and screen artifacts. Candidate-only failure after
oracle completion is a mismatch. Oracle failure and trace-limit exhaustion remain
harness errors. No dependency pin, trace budget or comparison is relaxed.
