# Parity test dependencies

The production dependency graph remains `regex-lite` and `unicode-width`.

The S02 runner pins MIT-licensed `laputa-ptytest` 0.4.0 to commit
`2efca1cffe9f4f19bddb7208643c18de63bbbc41` (2026-11-11). Its Unix process-group
cleanup, independent VT model, resize, signal delivery and condition waits avoid
implementing another PTY framework. The pin's source and consumer selftests were
inspected locally; changing it requires the plan's regression and dependency checks.

The lock adds five packages: `laputa-ptytest`, `rustc-hash` 2.1.3,
`vt100` 0.18.1, `vte` 0.15.0 and `arrayvec` 0.7.8. Their manifests declare
MIT, Apache-2.0, or a choice of these licenses. `libc` 0.2.182 was already
locked and remains test-only. `itoa` advances from 1.0.17 to 1.0.19 to meet
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
