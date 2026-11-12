# Coding campaign: Bring `lz` to exact behavioral parity with `less` 668

## Mission

Reference: https://github.com/gwsw/less/tree/v668

Your mission is to bring `lz`, a minimal Rust pager, to **complete observable behavioral parity with GNU `less` version 668**.

This is an implementation campaign, not a planning or documentation exercise. Work autonomously, implement improvements, run tests, commit verified progress, and continue iterating.

The primary methodology is **differential testing against a pinned reference executable**, combined with an automated, coverage-driven correctness hillclimb.

Do not settle for the existing project's goal of supporting 90% of common `less` usage. The new goal supersedes that limitation.

### Nonnegotiable engineering constraints

- Remain a native Rust implementation. Do not wrap, execute, or embed upstream `less` as the production implementation.
- Preserve the project's minimalist architecture: direct Unix terminal interfaces, small binary, fast startup, limited dependencies.
- Do not introduce crossterm, termion, clap, or a heavyweight TUI framework merely for convenience.
- Test-only dependencies are acceptable where they materially improve correctness or testing reliability.
- Support Linux and macOS; isolate legitimate OS-specific behavior.
- Do not impose artificial line-length, file-size, or Unicode restrictions inconsistent with `less` 668.
- Preserve lazy loading and bounded memory behavior where appropriate. A pager must work with gigantic files and streaming input.
- Prefer clean abstractions over accumulating special cases.
- Do not introduce dependencies on the upstream `less` executable at runtime.
- Do not copy upstream C implementation code without addressing licensing. Use the executable as the behavioral oracle and the source as a semantic reference.

Correctness comes before optimization. Performance regressions must be measured and addressed after functional equivalence is established.

---

## Phase 1 — Establish the authoritative reference

Obtain the exact upstream `less` 668 source distribution.

The GitHub tag is `gwsw/less@v668`. A GNU release tarball is also available as `less-668.tar.gz`.

Pin the actual source revision and release archive checksum. Record the source URL, checksum, build configuration, compiler, terminal library, regex backend, locale, and target platform.

Build an isolated reference binary under a gitignored testing/tool cache. Do not use `/usr/bin/less` as the oracle unless its identity and configuration have been independently verified.

Provide reproducible commands:

- `make parity-setup` — download or locate, verify, and build the reference.
- `make parity-test` — run the deterministic differential suite.
- `make parity-fuzz` — run randomized differential cases.
- `make parity-report` — emit machine-readable and human-readable progress reports.

After setup, ordinary tests must not require network access.

Verify that the executable actually identifies as version 668. A reference mismatch is a hard harness failure, never a skipped test.

Use the version-668 manual, bundled help, command definitions, options, and upstream source to enumerate the intended behavior. Do not inadvertently implement behavior introduced in 679, 685, 692, 710, or other later releases.

### Reuse upstream lesstest

Study `gwsw/less/tree/v668/lesstest` thoroughly.

This is particularly valuable: upstream already has a testing system that feeds keyboard actions into `less`, simulates terminal output, records screen states, and replays sessions against golden expectations.

Investigate:

1. Building upstream `less` with `LESSTEST=1`.
2. Running its existing `.lt` regression corpus.
3. Extracting input files, recorded keystrokes, and expected screen states.
4. Reusing those scenarios in the new `lz` differential harness.
5. Importing additional upstream regression cases when their fixtures can be reproduced reliably.

Understand that upstream `lesstest` uses test-specific instrumentation and a simulated terminal. Do not assume it can run directly against an ordinary `lz` binary.

Either adapt it appropriately or translate its scenarios into a generic black-box PTY harness.

Keep the `lz` harness independent of the internal implementation details of either pager.

Preserve test provenance and observe upstream licensing requirements.

---

## Phase 2 — Build a proper differential-testing harness

This is the most important infrastructure deliverable.

Implement a deterministic test runner that launches the pinned `less` binary and the current `lz` binary under equivalent conditions.

A real Unix pseudo-terminal is required for interactive testing.

Python 3's standard-library `pty`, `termios`, `fcntl`, `selectors`, `signal`, and `subprocess` modules are reasonable for orchestration. A compact Rust test harness is also acceptable.

Use a mature terminal emulator library for interpreting captured VT output, if needed. Keep such dependencies test-only.

### Process and terminal isolation

Each test must control:

- Terminal width and height.
- `TERM` and terminfo capabilities.
- Locale and character encoding.
- Environment variables.
- Working directory.
- `HOME`, XDG directories, history and configuration.
- stdin as file, pipe, PTY, FIFO, or closed stream.
- stdout as terminal or nonterminal.
- stderr separately where applicable.
- Process groups and controlling terminals.
- Signal delivery, pipe closure, and process termination.
- Timeouts and cleanup.

Use proper session and controlling-terminal setup so `/dev/tty` behaves realistically.

Account for programs flushing queued keyboard input while entering raw mode. A keystroke sent before initialization must not become a fake pager bug.

Do not rely on arbitrary long sleeps. Detect startup/readiness, use bounded output-quiescence checkpoints, and preserve detailed diagnostics when a synchronization condition fails.

### Compare observable behavior, not just stdout bytes

`less` and `lz` may use different terminal escape sequences to produce identical visible output.

Therefore, raw output comparison is insufficient for interactive parity.

At each checkpoint, compare the fully interpreted terminal state:

- Visible character cells and their positions.
- Foreground/background colors and relevant attributes.
- Cursor position and visibility.
- Status line and prompt contents.
- Screen scrolling and clearing.
- Alternate-screen behavior.
- Search-match highlighting.
- Terminal state after exit or suspension.
- Audible/visual bells where relevant.
- Exit status and signals.
- Filesystem side effects, including history and logging.
- External helper invocations in controlled integration tests.

Capture raw terminal streams alongside the interpreted screen for debugging.

Do not normalize away meaningful behavior. Allow only documented, narrowly scoped differences, such as the executable's own name or version identifier where unavoidable.

A different rendering strategy is acceptable. A different final screen, cursor state, command response, or terminal lifecycle is not.

For OSC sequences and terminal behaviors unsupported by the selected emulator, implement focused assertions on the relevant escape streams and effects. Unsupported emulator features must never silently count as passes.

### Declarative test scenarios

Represent scenarios as portable, reviewable fixtures (JSON, TOML, or another simple format).

Each scenario should describe:

- Input fixture and source type.
- Command-line arguments.
- Environment overrides.
- Terminal configuration.
- Ordered keyboard events.
- Signal, resize, file-append, or pipe events.
- Checkpoints.
- Expected completion conditions.

For every scenario, run the same procedure against both executables.

Each mismatch should produce:

- Test identifier and deterministic seed.
- Reproduction command.
- Input fixture.
- Event/keystroke transcript.
- Expected and actual interpreted screen states.
- A focused visual/cell-level diff.
- Raw terminal traces.
- Exit status and stderr differences.
- Relevant environment and reference-build metadata.

A failed test must be reproducible independently of the rest of the suite.

Classify harness errors separately from implementation mismatches.

---

## Phase 3 — Build a complete feature inventory

Extract the full observable feature surface of `less` 668 from its manual and reference implementation.

Maintain `tests/parity/coverage.md` or an equivalent generated report.

Every feature must have a status:

- Not tested
- Known incompatible
- Partially compatible
- Fully compatible with passing tests
- Platform-specific, with a documented reason

Do not conflate test-pass percentage with feature coverage.

### CLI and invocation

Test all documented options, including their argument syntax, long and short forms, interactions, negation/toggling, and applicable errors.

Include at minimum:

`-S`, `-R`, `-r`, `-N`, `-n`, `-F`, `-X`, `-i`, `-I`, `-e`, `-E`, `-M`, `-m`, `-p`, `-P`, `-s`, `-f`, `-K`, `-k`, numeric options, `+` commands, and `--`.

These are examples, not an exhaustive inventory. Derive the complete list from 668.

Test unknown flags, missing arguments, combined flags, conflicting options, `LESS` environment defaults, and command-line overrides.

Do not silently ignore unsupported options or pretend they work.

### Navigation and interactive commands

Cover all documented commands and their aliases, including:

- Character, line, half-page, and full-page movement.
- Numeric repeat prefixes.
- Absolute line numbers and percentages.
- Beginning/end navigation.
- Horizontal shifting and long-line chopping.
- Marks, mark navigation, and position restoration.
- Multiple files, next/previous file, and file examination.
- Help, status, redraw, and option inspection/toggling.
- Follow mode and interruption.
- Command-line editing, history, completion, and cancellation.
- Key sequences and custom key bindings.

Compare command semantics, positioning, and error responses, not just whether a key is accepted.

### Search and filtering

Search parity requires special attention.

Test literal and regular-expression searches, regex dialect, forward/backward search, repetitions, search offsets, empty patterns, invalid expressions, case sensitivity, and search modifiers.

Test `&` filtering and interactions with scrolling, highlighting, headers, and file changes.

Preserve match positions accurately across multibyte UTF-8, wrapping, truncation, and control sequences.

Do not assume `regex-lite` is automatically equivalent to the regex backend used by the reference build. Differential tests must establish semantic equivalence; change the implementation where necessary.

### Text and terminal rendering

Generate fixtures covering:

- ASCII, Unicode, CJK, combining marks, emoji, and complex grapheme clusters.
- Valid and invalid UTF-8.
- Tabs, backspaces, carriage returns, form feeds, and control characters.
- LF, CRLF, CR, and unterminated final lines.
- ANSI SGR, 256-color and true-color sequences.
- OSC 8 hyperlinks.
- Terminal-width edge cases, wrapping, and chopped lines.
- Search highlighting over colored text.
- Long lines far beyond the current 64 KiB limit.
- Tiny and very large terminal dimensions.
- Terminal resizing while scrolling or searching.

Model display columns, file offsets, logical lines, wrapped screen lines, and Unicode boundaries separately.

Avoid byte-index assumptions that corrupt column positions.

### Files, pipes, and streaming

Test empty and nonexistent files, directories, unreadable files, binary data, sparse huge files, symlinks, and multiple-file navigation.

Test piped input, slow writers, pipes that remain open, premature EOF, FIFO input, file append, truncation, replacement, and follow-mode transitions.

Test noninteractive stdout behavior: `less` must not require an interactive terminal in situations where the reference can operate without one.

Include interruption and cancellation during potentially long operations.

### Configuration and integrations

Cover environment variables, history behavior, lesskey source and binary configurations, input preprocessors, postprocessors, editor invocation, tags, and other integrations documented by 668.

Test `LESSSECURE` and related restrictions.

Where features invoke external commands, use isolated fixture executables and a sandbox with no uncontrolled writes or network access.

The objective is observable compatibility, not executing arbitrary real-world user programs during CI.

---

## Phase 4 — Establish the initial baseline

Run the entire feasible test suite against the existing `lz` before making significant functional changes.

Capture the baseline and commit the testing infrastructure independently.

Identify failures by subsystem and root cause.

Some existing architectural choices are already incompatible with full parity:

- Disabled wrapping by default.
- Unconditional smart-case search.
- Incorrect `-r` semantics.
- Silently ignored CLI arguments.
- Single-file argument model.
- Fixed 64 KiB logical-line truncation.
- Custom status display and help behavior.
- Limited input and command dispatch.
- Missing lesskey and configuration behavior.

Confirm each discrepancy using the pinned oracle.

Do not preserve an incompatible behavior merely because it is documented in the existing `AGENTS.md`. Update that guide as implementation and architecture evolve.

Produce a prioritized failure inventory based on impact, dependency ordering, and implementation cost.

Distinguish fundamental architectural gaps from isolated command bugs.

---

## Phase 5 — Continuous correctness hillclimb

Now begin the central campaign.

Repeat the following loop:

**1. Measure.** Run the differential suite and identify the highest-value class of mismatches.

**2. Cluster.** Group failures by shared root cause rather than treating every failing screenshot as a separate bug.

**3. Minimize.** Use delta debugging to shrink failing input, arguments, keyboard events, terminal dimensions, and environment until the smallest useful reproduction remains.

**4. Preserve.** Turn the minimized failure into a permanent regression test. Ensure it fails before the implementation fix.

**5. Implement.** Fix the underlying semantics cleanly, refactoring where justified.

**6. Validate.** Run focused tests, the full deterministic suite, Rust unit tests, and relevant platform checks.

**7. Measure again.** Confirm the mismatch class disappears without introducing new differences.

**8. Commit.** Commit the fix, associated regression tests, and relevant coverage updates.

**9. Continue.** Select the next root-cause cluster.

Do not stop after one successful feature or one pass of the test suite.

Do not optimize the pass percentage by removing difficult tests, weakening comparisons, introducing unexplained skips, or hardcoding individual golden outputs.

### Prioritization

First fix foundational behavior that affects many scenarios: argument parsing, terminal lifecycle, file representation, line wrapping, navigation positioning, and rendering.

Next implement search, input semantics, multibuffer navigation, configuration, and remaining command families.

Then work through edge cases, platform differences, and low-frequency features.

This ordering is a guide, not a substitute for the measured failure inventory.

Prefer removing one architectural cause of 100 failures over patching 100 separate symptoms.

---

## Phase 6 — Differential fuzzing and adversarial testing

Extend the harness into a deterministic, grammar-aware differential fuzzer.

Generate valid and deliberately invalid combinations of:

- CLI options and arguments.
- Text and binary input.
- Terminal dimensions.
- Keyboard-command sequences.
- Search patterns.
- File transitions.
- Resizes and signals.
- Environment and configuration settings.

Use weighted generation so common workflows remain well covered, while boundary and adversarial cases receive substantial attention.

Use metamorphic transformations where their equivalence is justified, such as changing the input source from a regular file to an equivalent completed pipe.

Do not assume transformations preserve behavior when the reference distinguishes those input types.

Record failing seeds and minimize them automatically.

Promote meaningful minimized failures into permanent fixtures.

Maintain a deterministic fixed-seed suite for normal CI and larger rotating-seed campaigns for deeper testing.

Add holdout scenarios not directly used to guide recent fixes. This helps detect implementations that overfit the known corpus.

Verify the reference harness itself using upstream test cases and deliberately faulty implementations or controlled mutations. Ensure that meaningful divergences are actually detected.

### Required robustness properties

Neither pager should leave the test terminal unusable after ordinary termination.

No uncontrolled hang, infinite loop, terminal corruption, unexpected panic, or unbounded memory growth is acceptable.

Use resource and execution limits for adversarial tests.

Test with large files and streaming sources without requiring full-file materialization in the harness or the pager.

---

## Phase 7 — Cross-platform correctness and performance

The project currently supports Linux and macOS.

Maintain native correctness tests on both platforms, accounting for differences in termios layout, signals, terminal libraries, and filesystem behavior.

Do not label a macOS-only failure as a generic incompatibility until it has been reproduced against the macOS reference binary under identical conditions.

Integrate a fast deterministic smoke suite into regular CI.

Run the broader parity suite on Linux and macOS where runners are available. Run extended fuzzing separately so routine commits remain reasonably fast.

After major compatibility work, revisit the existing `bench/run.sh` performance tests.

Measure:

- Startup latency.
- Time to first meaningful screen.
- Navigation throughput.
- Search hit/miss performance.
- File-opening and switching costs.
- Resident memory and allocation behavior.
- Streaming responsiveness.
- Binary size.

Compare both against the earlier `lz` baseline and the pinned `less` 668 build.

Avoid sacrificing important performance properties unnecessarily, but never keep an incorrect optimization solely because a microbenchmark looks favorable.

Performance tests and correctness tests must remain separate and cannot share assumptions that hide semantic differences.

---

## Phase 8 — Completion criteria

Do not declare exact parity on the strength of a high pass percentage alone.

The completion gate requires:

1. The full documented `less` 668 behavioral inventory has been enumerated and covered, including meaningful edge cases and interactions.
2. All applicable deterministic differential tests pass.
3. The upstream `lesstest` scenarios have been incorporated or accounted for, with any nonportable cases explained.
4. Fuzz campaigns produce no unexplained reproducible differences.
5. Linux and macOS compatibility tests pass on their supported reference configurations.
6. CLI, terminal, filesystem, history, and integration behavior have no unresolved discrepancies within the declared 668 scope.
7. Every initially identified incompatibility is resolved or explicitly demonstrated to be equivalent.
8. The ordinary Rust test suite, CI checks, and performance regression checks pass.
9. Documentation accurately describes the implemented behavior without unsupported parity claims.

For each feature, clearly distinguish **implemented**, **tested**, and **proven equivalent within the tested scenarios**.

No finite test suite mathematically proves equivalence over all possible interactions. Therefore, the claim of parity must be grounded in comprehensive coverage, differential evidence, and an explicit accounting of remaining uncertainty.

A feature without adequate coverage is not complete.

## Required deliverables

Leave the repository with:

- The improved Rust implementation.
- A reproducible, version-pinned reference build.
- A fully working PTY differential harness.
- Declarative test fixtures and regression corpus.
- Upstream test-suite reuse where practicable.
- Automated mismatch minimization.
- Deterministic differential fuzzing.
- CLI commands for setup, testing, fuzzing, and reporting.
- Linux/macOS CI coverage.
- Updated README and AGENTS.md.
- A living parity scorecard documenting coverage, failures, and remaining gaps.

The report should show both feature coverage and test outcomes, broken down by subsystem, with historical progress across commits.

Do not report an aggregate percentage without its denominator and an explanation of what is still untested.

## Execution policy

Work unattended. Make reasonable engineering decisions without requesting routine approvals.

The repository's default branch is `master`. Commit verified, coherent increments directly to it where permitted. Preserve unrelated work and do not force-push.

Keep commits reviewable, with meaningful messages linking the implementation to the behavioral discrepancy and its regression evidence.

Do not spend the entire campaign generating a plan. Establish the reference and harness quickly, then use them to drive actual implementation.

Do not stop merely because the obvious commands work or because the current test suite is green. Continue expanding coverage until the completion criteria have been satisfied.

If execution limits interrupt the campaign before exact parity is achieved, leave the repository in a passing, usable state with committed progress and a precise list of remaining incompatibilities, reproducible failures, and next actions. Never claim completion prematurely.

**Guiding principle: The reference binary is the oracle. The harness measures reality. The hillclimb closes the gap. Every fix must make the implementation more generally correct, not merely better at passing a particular test.**
