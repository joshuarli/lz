# Implement observable parity with less 668

## Status

- Active owner goal: finish the campaign completely. `/yea` resumes all remaining local deliverables and closure gates. Keep one writer, local commits, no push, no formatter, and the pinned toolchain. Baseline remains `2ae5660`; ten earlier slices landed through product `feec5fd`.
1. **Implementation:** S02i expands the bounded frozen-baseline queue from four to eight workers. Nine controlled cases include one timeout and queued work beyond the worker count; order, independent worlds, classification and draining pass. The complete 293-case baseline command took 90 seconds versus 175 before, with every case ID/outcome unchanged. No case, timeout, trace or resource limit changed. Fresh current/baseline accounting and Clippy pass; product Rust/fuzz proof from S07d remains valid. Next S04o owns real SIGINT and normal-mode interrupt behavior in terminal/pager/main, beginning with a retained lifecycle reproduction and cleanup proof. Interactive +cmd, tag diagnostics/duplicates/color conversion, long-running cancellation, missing-file skipping, wrapped geometry, headers, resource budgets beyond PTY pagers, broad fuzz/hardening, helpers, history concurrency/editing, POSIX syntax/locale semantics and S13–S15 remain open.
2. **HUP decision:** `/yea` approves retaining lz cleanup as an explicit safety divergence from the pinned oracle, whose one-byte HUP case leaks represented modes. Keep `parity/S04c-hup` and its strict mismatch; this decision does not establish equivalence or permit changing the oracle. No further owner ruling is needed for the cleanup choice.
3. **Environment proof:** repeat the invalid-byte filename case on an available Linux filesystem. Native Linux musl x86_64/aarch64, macOS x86_64 and remote CI evidence remain absent; Docker iteration does not count as native proof. No remote runs are authorized.
4. **Closure:** deterministic fuzz, holdouts, performance gates, parity CI changes/checks, cleanup and ultrareview remain open. Run in dependency order without publishing. Current local checks: 144 Rust tests, Clippy, supervisor/mutation/worker/file-mode controls and 291 admitted differential cases pass. [Coverage](tests/parity/coverage.md) retains 385 entries and 381 Unix-applicable denominators. The HUP safety divergence and raw-filename environment requirement remain outside admission.

## Charter

Execute this plan only within the owner's execution grant. This document authorizes no implementation by itself. During execution, decide routine methods and continue. Record a decision only when later work depends on it.

Keep one writer. All product slices share core files, fixtures, and the parity register, so none of the slices below may run in parallel. Follow the [campaign mechanics](https://github.com/joshuarli/usr/blob/main/dotfiles/base/.agents/skills/orchestrate/references/campaigns.md) and run their [parent steering checkpoint](https://github.com/joshuarli/usr/blob/main/dotfiles/base/.agents/skills/orchestrate/references/campaigns.md#parent-steering-checkpoint) after every accepted slice, on resume, and after two failed attempts. Delegation, if useful, follows the installed `orchestrate` skill and its bridge contract. The parent owns integration and acceptance.

Keep `master` green. Commit accepted, coherent changes locally using Conventional Commits with scope `parity`; put the slice ID and causal discrepancy in the body. Never push. Preserve unrelated edits. If the integration tree contains unrelated uncommitted work, use a clean worktree at `.work/wt/parity` from its current HEAD instead of committing that work. Follow the [worktree lifecycle](https://github.com/joshuarli/usr/blob/main/dotfiles/base/.agents/skills/orchestrate/SKILL.md#worktree-lifecycle) before retiring it.

Before writing scratch, verify `.work/` is ignored through `.git/info/exclude`. Set `TMPDIR` to the absolute repository `.work/tmp` path. Put reference sources, tools, fixture worlds, traces, and retained failures under `.work/`. Never use system temporary directories for campaign state. Do not change the pinned Rust nightly or run formatters.

Complete feature and test parity before optimization or polish. Measure performance during implementation when useful, but gate only the egregious conditions below until S14. Repairs needed to keep verification useful may happen earlier.

For each root-cause cluster:

1. Reproduce one mismatch against the pinned oracle. Separate a harness defect from a pager defect.
2. Minimize input, arguments, events, environment, and dimensions without changing the failing observation.
3. Keep a regression that fails before the fix. Implement the cause at its owning boundary.
4. Run that slice's acceptance and the narrowest affected Rust tests on the integrated tree.
5. Commit green work, update the register, and steer before selecting another slice.

Allow at most two materially different attempts or 90 minutes without a passing local acceptance, whichever comes first. After two refuted hypotheses, bisect input, history, or the failing boundary. After the limit, park the slice on `parity/<id>` with the cause and a concrete retry condition. Park its dependent slices and continue independent ready work. One parent owns a shared environment blocker. Never retry unchanged work to green.

Fix an encountered panic, terminal leak, orphaned owned child, silent data loss, a standard scenario exceeding its 10-second bound, or a routine loop exceeding 180 seconds in a separate focused commit. Diagnose first. Do not expand this into a repository audit. Record lesser unrelated defects without fixing them. A harness timeout is a harness error until the same input establishes a candidate-only hang.

Stop an item for missing authority, forbidden effects, an unresolvable reference identity, or the absence of any usable local check. State the condition in Status. Proof requiring unavailable hardware, services, or hosts goes to Needs environment and does not hold local completion open.

## Decisions

### Owner decisions — 2026-11-12

- **O1:** less 668 behavior takes precedence over conflicting lz flags, key bindings, defaults, and history format. Keep lz's name and package version. This replaces the old 90-percent scope and intentional feature omissions.
- **O2:** parity-specific Linux and macOS CI changes are authorized. This grant does not include changing release automation, pinned toolchains, unrelated workflows, or triggering remote runs.
- **O3:** keep runtime dependencies pure Rust, apart from existing direct Unix OS interfaces. Do not add a system regex, terminal, or other C library to the production implementation. If pure Rust cannot satisfy an entry, park it with evidence. Such an entry prevents a full-parity claim.
- **O4:** prefer `laputa-ptytest` when helpful. Use it for interactive execution and semantic screen observations as prescribed below.

### Prescribed decisions

| Decision | Binding rule and reason | What permits reconsideration |
| --- | --- | --- |
| Oracle | Use the verified release archive and POSIX regex build below. less behavior depends on its build, libc, locale, and terminfo. | Evidence of a wrong source pin or an unavailable platform backend. Park the affected proof instead of silently changing the oracle. |
| Harness | Use a Rust integration runner with `laputa-ptytest`, JSON scenarios, and a small stdio supervisor. Keep the production binary independent of all three. | A minimized missing capability. Add a consumer-side assertion or adapter first; do not build another general PTY framework. |
| Test dependency | Pin `laputa-ptytest` by git revision `2efca1cffe9f4f19bddb7208643c18de63bbbc41` from `https://github.com/laputa-systems/ptytest`; library name `ptytest`, package version 0.4.0. Commit Cargo.lock. | A proven defect at that revision. Change the pin only after a failing consumer case and repeat dependency checks. |
| Source reuse | Use upstream source as a semantic reference. Translate test scenarios with provenance and required notices. Do not port upstream C implementation code. | An explicit owner licensing decision, outside this execution grant. |
| Identity | Normalize only exact program-name and package-version fields at identified observation locations. Keep surrounding help, diagnostics, and status text comparable. | A new identified identity location, tested with a negative control. No global string replacement or prompt masking. |
| History | Implement the 668 path, format, and concurrency behavior. Leave the old `$XDG_STATE_HOME/lz/history` file untouched and unread. No automatic migration or deletion. | A separate owner migration request. |
| Failure admission | Maintain a green admitted corpus and a separately reported discovery corpus. Every selected feature must pass strictly before admission. | Never weaken comparisons or remove regressions to improve a score. |
| Architecture | Evolve modules by capability. Keep parser, source, terminal lifecycle, display layout, and search policy distinct. No wholesale rewrite before the pilot. | Two unrelated fixes requiring the same workaround justify revisiting the owning boundary. |
| Timing | Use condition readiness, then at most 20 ms output quiescence within a 3-second checkpoint deadline. Standard scenarios have a 10-second total deadline, large/adversarial cases 30 seconds. | A measured oracle case requiring more time gets an explicit per-case bound. Never increase a timeout to conceal a candidate hang. |
| Closure | Local completion and cross-platform parity are separate verdicts. Missing environment proof remains visible. | Full parity requires every applicable platform and performance gate, with no unresolved discrepancy. |

`laputa-ptytest` is MIT licensed and adds `libc`, `rustc-hash`, and `vt100` to test builds. It provides real sessions, process groups, deadlines, resize, snapshots, raw traces, and cleanup. It avoids duplicating those mechanisms. Python stdlib PTYs plus a separate emulator would require additional synchronization and lifecycle code. A custom Rust PTY implementation duplicates this crate's tested boundary. Use neither as the primary harness.

Additional runtime crates require a demonstrated semantic need, permissive compatible licensing, maintained source, and a transitive-cost review. Prefer replacing an existing crate over adding a second implementation. No crossterm, termion, clap, or heavyweight TUI framework. Test-only JSON parsing and seeded generation dependencies are permitted if standard-library code would obscure the test contract. Record their versions, licenses, maintenance evidence, clean test-build time, and locked dependency count. Production dependency changes also measure dist binary size, clean build time, startup, and affected runtime workflows against S14's gates. Host-busy timing gates defer completion, not useful iteration.

### Delegated decisions

The executor chooses private types and module splits inside the owned files. Keep source offsets, logical line indices, wrapped row offsets, and display columns distinct where exchanging them would be a bug. Keep non-UTF-8 paths and content as bytes until presentation. Resolve CLI, environment, and lesskey defaults once into typed policy, using precedence observed in 668.

Within each family, choose the next cluster by dependent behaviors unblocked, then common-workflow impact, then smallest complete change. After one end-to-end pass for every family, deepen edges and interactions. Record only choices another slice needs.

For a missing regex behavior, first write the smallest POSIX-oracle case. Extend or replace the pure Rust implementation only if the same regression and retained corpus pass. Limit a speculative backend experiment to two attempts or 90 minutes. Otherwise park the regex entry and continue nondependent work. Do not narrow the inventory to fit `regex-lite`.

## Baseline

Evidence below is from commit `2ae5660205fb1b114610750ede846845196872ee` on macOS. Revalidate changed inputs at execution start. Preserve the existing library split and rustybench harness.

| Check | Observed result | Elapsed time |
| --- | --- | --- |
| `RUSTC_WRAPPER=kache cargo test --quiet -- --skip history::tests::load_nonexistent_returns_empty --skip history::tests::save_and_load_roundtrip --skip history::tests::load_skips_empty_lines` | 125 passed; 3 filtered out because their literal paths use system temporary directories | 0.15 s |
| `RUSTC_WRAPPER=kache cargo build --quiet` | Passed | 1.25 s |
| `RUSTC_WRAPPER=kache cargo clippy --quiet --workspace --all-targets -- -D warnings` | Passed, including the existing benchmark target | 0.72 s |
| Archive `./configure --with-regex=posix` and `make -j2` | less 668, POSIX regex, Apple clang 21.0.0, system `libncurses.5.4` | 12.90 s |
| Instrumented reference build plus `lesstest/runtest -l ../less -r <repo-scratch> lt` | All 14 upstream `.lt` files passed | 8.33 s |
| Disposable `laputa-ptytest` Rust probe | Both pagers rendered content, accepted `q`, exited 0, and restored represented terminal lifecycle. Independent snapshots differed. | Repeat build/run 1.07 s; remeasure the integrated runner in S02 |

These are single-run feedback-loop observations, not product speed comparisons. The preflight subset does not establish a fully green baseline. S00 must run all tests after removing the three isolation defects.

The first verified product discrepancy is redirected output without a controlling terminal: less copies the fixture bytes with exit 0. Existing lz attempts to open `/dev/tty` and exits with an error. S01 fixes this before building the full evaluator.

Additional source-confirmed gaps require oracle scenarios before implementation:

- `src/main.rs` silently ignores unknown flags and `+` commands, accepts one filename, and defines lz-specific raw behavior.
- `src/buffer.rs` stores `Vec<String>`, reads a complete line before truncation, and then truncates at 64 KiB. Invalid UTF-8 read errors become EOF. This is not a bounded allocation strategy.
- `src/pager.rs` starts unwrapped, has custom help and status, lacks wrapped-row positions, and reads all input for filtering and follow entry.
- `src/search.rs` always applies smart case and uses ASCII folding on its literal path. No evidence establishes equivalence to POSIX regex.
- `src/terminal.rs` and `src/input.rs` use direct platform FFI. Names in the old guide such as `Terminal`, `InputReader`, and `LineBuffer::get` do not describe the baseline implementation.
- The baseline has two runtime dependencies and test-only `tempfile` and `rustybench`. `src/lib.rs` exports the modules consumed by `benches/bench.rs`. The eight current benchmarks cover buffer/search/ANSI hot paths with a 100,000-line fixture, not interactive pager journeys. `make release` produces `target/<target>/dist/lz`.

## Sequence

Sizes: S is one narrow change, M is one capability with fixtures, and L is a family delivered as serial sub-slices. An L item is not one giant commit. Before each sub-slice, name its exact behavior IDs, owned paths, prerequisites, and one acceptance command in Status. Use suffixes such as S06a. The parent may revise methods and batching inside the declared family without changing required behavior or proof.

Every product slice owns its affected `tests/parity/` cases and register entries, plus relevant README and AGENTS.md corrections. New Rust modules stay under `src/` and keep the owner named below. A changed library contract also owns its `src/lib.rs` wiring and affected benchmark callers. Update them in the same slice and compile all targets. Shared manifests and Makefile changes belong to the active slice only. All rows are serial.

| ID / size | Prerequisites | Owned files | Deliverable and one local acceptance command |
| --- | --- | --- | --- |
| S00 / S | Clean integration context | `src/history.rs`, existing test owners if concurrent work moved them | Replace all three literal system-temp test paths with unique repository-local fixtures. Preserve their assertions. `RUSTC_WRAPPER=kache make test` passes all tests with TMPDIR under `.work/tmp`. |
| S01 / M | S00 | `src/main.rs`, `Makefile`, `Cargo.toml`, `Cargo.lock`, `tests/parity/` setup and first runner | Pin and build the reference. Deliver correct nonterminal file and completed-pipe output, including empty, binary, and invalid-byte input. Preserve reference exit/error behavior. `make parity-test GROUP=redirected MODE=admit` strictly compares bytes, stderr, and exit status. |
| S02 / M | S01 | Test manifests, Makefile, `tests/parity/` PTY runner, supervisor, first interactive fixtures | Integrate `laputa-ptytest`, self-comparison, mutation controls, and independent worlds. Establish interactive startup, quit, and lifecycle observations without changing comparison policy. `make parity-selftest` detects all seeded faults and passes reference-against-itself. |
| S03 / M | S02 | `tests/parity/` inventory, upstream translation, reports | Enumerate every 668 documented surface and all 14 upstream tests. Translate one navigation and one search scenario end to end. Capture discovery results against the unchanged baseline. `make parity-inventory-check` verifies source anchors, scenario links, all upstream dispositions, and complete result accounting. |
| S04 / L | S03 | `src/main.rs`, option parsing modules, `src/terminal.rs`, `src/input.rs`, `src/pager.rs` startup | Implement all CLI forms, numeric options, `+` commands, errors, env precedence, terminal capabilities, startup/exit flags, and a single terminal owner. `make parity-test GROUP=invocation MODE=admit`. |
| S05 / L | S04 | `src/buffer.rs`, source modules, `src/main.rs`, `src/pager.rs` source consumers | Introduce byte-preserving random access and seekable/nonseekable source states. Remove the 64 KiB truncation and full-line allocation prerequisite. Handle binary prompts, file errors, EOF, huge/sparse files, and interruption. `make parity-test GROUP=sources MODE=admit`. |
| S06 / L | S05 | `src/ansi.rs`, display-layout modules, `src/pager.rs`, terminal output helpers | Deliver default folding, chopping, display positioning, colors, controls, malformed bytes, widths, resize, prompts, headers, and highlighting layout. `make parity-test GROUP=rendering MODE=admit`. |
| S07 / L | S06 | `src/input.rs`, command modules, `src/pager.rs` movement/help/option dispatch | Deliver all movement commands and aliases, numeric prefixes, line/byte/percent jumps, marks, bracket matching, redraw, help, status, option inspection and toggles. `make parity-test GROUP=navigation MODE=admit`. |
| S08 / L | S07 | `src/search.rs`, `src/pager.rs`, layout-to-source mappings | Deliver POSIX search semantics, default case behavior, `-i`/`-I`, modifiers, repetition, offsets, empty/invalid patterns, filters, and search through colored/wrapped text. `make parity-test GROUP=search MODE=admit`. |
| S09 / L | S08 | File-list modules, `src/main.rs`, `src/pager.rs`, source/session owners | Deliver multiple files, examine/reopen/next/previous commands, per-file positions, search/filter transitions, follow interruption, append/truncate/replace, pipes and FIFOs. `make parity-test GROUP=sessions MODE=admit`. |
| S10 / L | S09 | `src/history.rs`, command editing and lesskey modules, policy initialization | Deliver line editing, completion, histories, lesskey source/binary formats, custom keys, config and environment semantics. `make parity-test GROUP=configuration MODE=admit`. |
| S11 / L | S10 | Integration/secure-mode modules, `src/pager.rs`, child lifecycle and file/log owners | Deliver preprocessors, postprocessors, shell/pipe/editor invocations, tags, logging, and all 668 secure-mode restrictions and allowances. Test only inert fixture helpers. `make parity-test GROUP=integrations MODE=admit`. |
| S12 / L | S04–S11, or their nonparked independent portions | Affected source owners, `tests/parity/` minimizer, fuzzer, holdouts | Complete remaining inventory cases and interactions; translate/account for every upstream case. Add bounded grammar fuzzing, minimization, and lifecycle hardening. `make parity-harden SEEDS=0..999` runs the deterministic corpus, fixed seeds, holdouts, and mutation controls. |
| S13 / M | S12 | `.github/workflows/ci.yml`, `Dockerfile` parity prerequisites, test scripts, README, AGENTS.md | Add admitted smoke tests to existing Linux musl and macOS jobs. Broader and rotating suites stay separate. `make parity-ci-check` validates local workflow commands and performs the available native smoke run; unavailable runner proof goes to Needs environment. |
| S14 / L | All locally applicable functional entries pass | `benches/bench.rs`, `benches/baseline.json`, source hotspots with evidence, Makefile, dependency manifest when needed | Extend rustybench to validated baseline/candidate/oracle work and end-to-end journeys, then optimize and polish. `make parity-perf` enforces the gates below and reports every deferred gate. |
| S15 / M | All nonparked local slices | Dead paths in affected modules, tests, README, AGENTS.md, parity reports | Remove retired lz behavior and dead compatibility paths after caller proof. Run `ultrareview`, fix demonstrated P1/P2 findings, and regenerate the final report. `make parity-close` applies the completion gate below. |

S04–S11 are a dependency order, not permission to perfect one family while leaving another unstarted. Land each family's minimum end-to-end path before expanding its edge cases in S12. If a blocked sub-slice is not a real prerequisite, write the exact dependency removal in Status and continue.

Full milestones run `make test`, `make lint`, and `make parity-test MODE=admitted` after S03, after the first pass through S11, after S12, and at close. Before S03, use the available admitted pilot cases. Discovery mismatches remain visible throughout. Run `make parity-test MODE=all` at the post-S12 functional gate and close; its unresolved cases must fail. Do not run the full suite after every small fix.

If an acceptance exceeds 180 seconds, diagnose its cost once. Shard by fixed case IDs while retaining the full milestone corpus. A single case should run within 10 seconds, with explicit large cases capped at 30 seconds. Announce reference rebuilds, clean dependency builds, release builds, and extended fuzz campaigns before starting them.

## Acceptance

### Reference identity and offline setup

Use these immutable inputs:

- Release archive: `https://www.greenwoodsoftware.com/less/less-668.tar.gz`.
- Archive SHA-256: `2819f55564d86d542abbecafd82ff61e819a3eec967faa36cd3e68f1596a44b8`.
- Git tag object: `210fb607972a99957294a603eea50fc9c67cca30`.
- Peeled source commit: `e77e1176c80cca989694818866ae6cd7e7707161` from [less v668](https://github.com/gwsw/less/tree/e77e1176c80cca989694818866ae6cd7e7707161).

`make parity-setup` verifies the archive before extraction and rejects unsafe archive paths. Build an ordinary oracle and a separate `LESSTEST=1` tree under `.work/tools/less-668/<platform>/`. Never mutate one into the other. Configure both with `--with-regex=posix`, normal nonsecure compilation, and the same editor, system-config path, and terminal-library choices. Point system configuration lookup into the owned tool world. Do not install either binary.

Commit reference inputs and build recipes under `tests/parity/`. Store host-specific build metadata with each result: compiler/version, flags, configure output, libc, terminal library, regex backend, locale, terminfo digest, source/archive identity, and executable digest. On macOS use system ncurses for the test oracle. On Linux use the repository Alpine/musl environment and its ncurses for the oracle. Test-only native libraries do not enter the production binary.

The ordinary oracle must report 668 and POSIX regex, without LESSTEST. The instrumented build must report its test instrumentation. A wrong version, checksum, backend, or missing tool is a harness error, never a skipped parity pass. Do not substitute `/usr/bin/less` automatically. The release archive and tag are separately identified inputs; do not assume their generated files are byte-identical.

Setup may fetch the pinned archive and locked test dependencies. Afterwards, `make parity-test`, `make parity-fuzz`, `make parity-report`, and ordinary Rust tests operate offline. Prove this with `CARGO_NET_OFFLINE=true` and no downloader invocation. Missing cached inputs fail with the setup command.

### Harness commands and admission

S01–S03 create these interfaces. They do not exist in the baseline:

| Command | Contract |
| --- | --- |
| `make parity-test CASE=<id>` | Run one exact scenario against both executables. Fail on a mismatch. Print its standalone reproduction command. |
| `make parity-test GROUP=<family> MODE=admit` | Run all selected cases strictly plus the existing admitted corpus. On all-pass only, permit the executor to commit their admitted status. The command itself never changes expected results. |
| `make parity-test MODE=admitted` | Fast green regression corpus. Default mode when no selector is supplied. |
| `make parity-test MODE=all` | Strict run of every applicable scenario, including discovery. Any mismatch fails. |
| `make parity-baseline` | Run every existing scenario against frozen baseline lz and reference. Emit all outcomes and known mismatch IDs. Exit 0 only when collection is complete with no harness error; mismatches remain explicitly failing results in the report. |
| `make parity-selftest` | Oracle self-comparison plus controlled output, attribute, cursor, exit, side-effect, and timeout faults. Every fault must fail for its intended observation. |
| `make parity-fuzz SEED=<n> CASES=<n>` | Deterministic generated cases, strict comparison, fixed resource limits, retained failure seeds, automatic reduction. |
| `make parity-report` | Generate JSON and Markdown coverage/outcomes from one canonical inventory and result set. Reject stale source or oracle metadata. |
| `make parity-inventory-check` | Check inventory accounting, source references, upstream dispositions, and that fully compatible entries have current passing evidence. |

Use stable JSON scenario IDs. Include fixture recipe/bytes, source type, arguments, environment overrides, cwd, size, TERM, locale, ordered input/signal/resize/append events, readiness predicates, checkpoints, and completion requirements. Validate the schema once. Unknown events or fields are errors. Load JSON as data, never executable code.

Each oracle and candidate run gets a separate but equivalent fixture world, the same relative paths, and the same deterministic input/event stream. Clear inherited application variables. Own HOME, XDG paths, cwd, config, history, log destinations, PATH helpers, and TERM/terminfo. Use `C` for byte/ASCII scenarios and a validated UTF-8 locale for Unicode: `C.UTF-8` on Alpine and `en_US.UTF-8` on macOS. Missing locale is environment debt, never a successful fallback.

Compare redirected output as bytes. For interactive checkpoints, compare every represented cell's text, wide-cell markers, colors and attributes, cursor position/visibility, wrap markers, screen dimensions, alternate-screen and other relevant modes, prompt/status text, bells, and exit/signal status. A never-written empty cell and a plain space are the same visible character, but retain their attributes. Compare filesystem bytes and helper effects after each relevant event. Preserve raw traces alongside semantic snapshots.

Normalize only the exact identity fields and per-world absolute-root prefixes. Do not remove colors, prompts, errors, filenames, terminal modes, timestamps under test, or differing status text. Frozen goldens come only from the verified oracle. Candidate output never updates them. A golden update must retain the old mismatch and its reason in the commit.

Every mismatch bundle includes case ID/seed, reproduction command, fixture recipe or bytes, minimized transcript, expected/actual cells and focused diff, raw input/output, stderr, exit status, side-effect diff, environment, platform, executable/source digests, and reference-build metadata. Distinguish `mismatch`, `harness-error`, and `needs-environment`. Neither of the latter two counts as compatible.

Admitting a new family cannot forgive an admitted regression. Newly discovered existing incompatibilities become discovery entries and retain their failure. A new behavior benefiting an unchanged consumer is eligible when its real differential case and admitted regressions pass; no coverage percentage or changed-file quota is required. A harness error blocks the affected acceptance, even if all product assertions reached so far passed.

### Use laputa-ptytest within its actual capabilities

Use `CommandSpec` for direct absolute executable paths, `Scenario`, `Size`, hermetic `TestEnv`, explicit `Deadline`s, raw input bytes for exact bindings, kernel `resize`, group `signal`, `wait_for_screen`, bounded `wait_for_quiescence`, and `wait_for_exit` followed by `finish`. Check both returned exit statuses. Pair every acquired process, FD, temp world, and producer with one owner and bounded cleanup.

Retain the baseline before driving the pager. After exit, assert represented lifecycle restoration before forced harness cleanup can repair anything. Cleanup success alone is not restoration proof. A supervisor must retain a usable PTY session and report kernel termios before initialization, at suspension, and after child exit. A direct macOS probe that queried the slave after its session leader exited returned ENOTTY; do not mistake that harness limitation for a pager failure.

The pinned API connects stdin/stdout/stderr to one PTY. For file/pipe/FIFO/closed stdin, redirected stdout, separate stderr, resource limits, and post-exit termios, use a small test-only supervisor compiled into `.work/tools/`. It owns the controlling session, redirects only the requested descriptors, forwards events, waits for its child, and writes bounded machine-readable status/effect records. It must preserve argv, environment, foreground-group ownership, signals, and exit status. Self-test those facts before using its observations as oracle evidence. Never add a production hook for the harness.

Keep the supervisor as the session leader and put the pager in its own foreground process group. Terminal-generated signals reach the pager through the kernel. Explicit harness signals reach the supervisor, which forwards each once to the pager group. The supervisor observes stop/continue/exit, retains the PTY for termios checks, and mirrors the pager's final code or signal. On cancellation it terminates, escalates, and reaps the owned pager group before exiting. Prove this cleanup because `laputa-ptytest` alone owns only its original process group.

The default protocol profile is disabled. The crate's `xterm-minimal-v1` allowlist rejects some legitimate less output, including intermediate-byte escapes and OSC 8. Select a named profile only after reviewing both pagers' controls. Do not learn an allowlist from arbitrary observed output. With the disabled profile, add explicit consumer assertions and deterministic replies for queries the cases require. Unknown queries or unrepresented controls are harness errors until a focused assertion covers them.

`vt100` does not prove soft-wrap resize reflow. `ScreenSnapshot` also does not cover every possible terminal extension or kernel mode. Add focused raw-stream/effect assertions for OSC 8 links, bell behavior, unsupported attributes, queries, and reflow-sensitive contracts. Test the adapter against an independently specified trace or a mature second emulator for each new unsupported semantic. Do not add an entire emulator as an entry requirement. If a missing capability cannot be checked locally, park that observation and forbid a full-parity claim.

Prove readiness before input that raw-mode setup could flush. Wait for a scenario-specific visible prompt/content and required terminal mode, then confirm bounded quiescence. A missing readiness condition produces a diagnostic timeout. Never send speculative early keystrokes or use long settle sleeps.

### Inventory, scenario families, and upstream reuse

One canonical inventory under `tests/parity/` owns each behavior ID, 668 manual/source anchor, responsible slice, platform/build applicability, case IDs, implementation status, evidence status, and unresolved cause. Generate `tests/parity/coverage.md` and JSON from it. Keep feature coverage separate from test pass counts. Every percentage includes its denominator and untested entries.

Allowed evidence states are not tested, known incompatible, partially compatible, compatible in the named tested scenarios, and platform-specific with evidence. Track implemented, tested, and equivalent separately. An entry without a distinguishing test cannot be complete. Platform-specific status must name the reference configuration and a demonstrated reason; it cannot hide an lz-only failure.

The inventory must cover these families, including success, failure, boundaries, and meaningful interactions:

| Family | Required scenarios | Owner/proof |
| --- | --- | --- |
| Invocation | Every documented short/long option, arguments, combined flags, conflicts, toggling/negation, numeric options, `+` commands, `--`, unknown/missing args, LESS defaults and CLI overrides. Include `-S -R -r -N -n -F -X -i -I -e -E -M -m -p -P -s -f -K -k`; derive the remainder from 668. | S01/S04; `GROUP=redirected` and `GROUP=invocation` |
| Navigation | All aliases, numeric repetition, line/byte/percent positions, wrapped rows, begin/end, half/full pages, horizontal shifts, marks, bracket matching, position restoration, help/status/redraw, option inspection/toggles, and custom key sequences. | S07/S10; `GROUP=navigation` and `GROUP=configuration` |
| Search/filter | Literal/regex dialect, forward/backward, n/N, repeats, offsets, empty patterns, invalid expressions, modifiers, default/case options, zero-length/multibyte matches, highlighting and filter interactions with wrap, headers, files, and colors. | S08; `GROUP=search` |
| Rendering | ASCII, CJK, combining marks, emoji/graphemes, invalid UTF-8, tabs/backspace/CR/form-feed/control bytes, LF/CRLF/CR/unterminated lines, SGR indexed/RGB, OSC 8, overstrikes, width boundaries, tiny/large sizes, resize during navigation/search. Default, raw, and color-safe modes follow the oracle. | S06; `GROUP=rendering` |
| Sources | Empty/missing/unreadable files, directories, symlinks, binary data, non-UTF-8 paths, sparse/huge files, very long lines, short/error reads, slow/open pipes, FIFO, closed stdin, nonterminal stdout, premature EOF, cancellation. | S01/S05; `GROUP=redirected` and `GROUP=sources` |
| Sessions | Multiple files, examine, next/previous, reopen and remembered state, append/truncation/replacement, follow transitions and interruption, search/filter across files. | S09; `GROUP=sessions` |
| Configuration | All documented environment variables, locale/charset tables, history contents/path/limits/concurrent writers, command editing/completion/cancel, source and binary lesskey input, system/user precedence. | S10; `GROUP=configuration` |
| Integrations/security | LESSOPEN/LESSCLOSE and their pipe/filename forms, editor, shell/pipe commands, tags, logging, helper failures/termination, LESSSECURE and every 668 LESSSECURE_ALLOW restriction/allowance. Assert argv, stdin/out, exit, cleanup, and files. | S11; `GROUP=integrations` |
| Terminal lifecycle | No controlling terminal, startup failures, ordinary/error exit, Ctrl-C, TSTP/CONT, WINCH, TERM/HUP/QUIT, inherited signal dispositions, broken pipe, panic/fatal cleanup where catchable, and application query responses. Match reference signal statuses and restore acquired state. SIGKILL requires process cleanup, not pager restoration. | S04/S12; `GROUP=lifecycle` |

Run the pinned instrumented corpus with `lesstest/runtest -l <instrumented-less> -r <owned-run-dir> lt` after building `lesstest`. All 14 files need dispositions: `chinese1`, `colorbars`, `github216`, `github265`, `github329`, `hdr-unicode`, `hdr-unicode1`, `proc-special`, `search-hist`, `seq200`, `subsearch`, `table-50x200`, `utf8-2.txt`, and `version.c` (all `.lt`).

Study `lesstest/README`, `extract`, `parse.c`, `run.c`, and the `.lt` records. Translate their embedded files, arguments, environment, keys, and screen checkpoints into generic scenarios. Their custom termcap controls are not ordinary VT output. Preserve original expectations as provenance and regenerate comparable VT goldens through the ordinary oracle. Never run unmodified lesstest directly against lz. Account for an untranslatable case with the exact instrumentation dependency, replacement test, or Needs environment entry. Preserve required upstream license notices.

### Architectural and failure constraints

Use byte-preserving chunked storage for source data, with a bounded hot cache and seekable indices or owned spill storage as needed. No full-file materialization at startup and no full-line allocation merely to render the first viewport. Source states distinguish ready data, temporary lack of data, EOF, follow polling, and read errors. A producer that remains open must not block keyboard handling indefinitely. Bound producer buffers and choose backpressure or disk spill explicitly.

Use one display-layout mapping between source offsets, logical lines, screen rows, and columns. Search and rendering consume that mapping instead of independently slicing strings. Keep regex backend details private. Do not expose parser or storage internals to get test access; test compiled binaries at the PTY boundary.

A terminal session owner unwinds partial setup and restores acquired modes on ordinary exit, error, suspend, and catchable termination. Signal handlers perform async-signal-safe work only. Observe 668's status and interruption policy rather than imposing a generic signal policy. Retain cleanup failures separately from the primary error. Cancellation reaches owned producers/helpers, and cleanup cannot leave a process running after the scenario deadline plus one second of escalation.

For history and logs, match the oracle's path, byte format, merge/append behavior, errors, and symlink handling. Test missing, empty, malformed, oldest supported 668 input, concurrent invocations, read-only destinations, and interrupted writes. Use atomic replacement where compatible with the observed format/semantics. Never silently impose a different concurrency policy because it is easier.

Content and filenames are untrusted inputs to the pager. They must not execute commands except through the explicitly configured/entered 668 integration surface. Test shell metacharacters in filenames and values. Where 668 explicitly specifies shell syntax, preserve that syntax and quoting; do not reinterpret every integration as a plain argv call. Run only inert owned helpers in tests. Prove both refusal under secure mode and permitted behavior under its documented allowances.

### Fuzzing and robustness

`make parity-fuzz` generates weighted CLI, byte/text, geometry, command, search, config, file-transition, resize, and signal events. Use 70 percent ordinary workflows and 30 percent boundary/invalid events. Every generated event has a precondition and deadline. Persist seed, generator version, and full environment. Minimize while preserving the same observation category and oracle completion.

Normal smoke uses 50 fixed seeds. S12 runs seeds 0–999. Close additionally runs 1,000 seeds derived from the integration commit and 100 held-out scenarios not used to guide recent fixes. These finite campaigns qualify evidence; they never prove all possible interactions. Promote meaningful minimized failures to permanent fixtures.

Use justified metamorphic checks only after the oracle establishes the transformation, such as a completed pipe versus a file for a particular scenario. Verify the comparison with deliberately wrong output, dropped errors, changed attributes/cursor, incorrect exit, missing side effects, and hangs. Detect unsupported controls rather than counting them as passes.

Large cases use a generated 1 GiB stream, a 10 GiB sparse file, and at least a 16 MiB logical line, including chunk boundaries within UTF-8 and controls. Never materialize these entirely in the runner. Bound each ordinary adversarial child to 512 MiB RSS, 30 seconds, 64 MiB captured trace, and 2 GiB owned spill. An oracle that legitimately exceeds a bound gets a separate named large scenario with a justified bound. It does not license dropping data in the pager. Trace exhaustion is a harness error; preserve its prefix and reproduction.

### Performance and polish — S14 only

Read `~/.agents/instructions/performance.md`, `make-it-fast`, and `benchmark-checklist` before measuring or selecting dependencies. Extend `benches/bench.rs` and the existing rustybench runner. Do not restore the removed shell benchmark. Assert that search-hit fixtures actually hit; the current literal hit benchmark's pattern and generated numbering need validation before its timing is useful. Keep the committed baseline immutable while comparing candidates; `make bench` rewrites it, so direct comparison output to owned scratch instead.

Freeze the original lz build at the baseline revision and the pinned ordinary less oracle. Use the same supported production build settings for candidate and original, and document oracle flags. Measure startup/first meaningful screen, one page and repeated navigation, jump-to-end, search hit/miss, open/switch, streaming responsiveness, peak RSS, clean build time, and dist size. Assert the expected screen, match, bytes, and exit inside each measured workflow. A fast error is not useful work.

Use a small common fixture and the existing million-line workload, plus the large/streaming cases above. Compare original lz only where it performs equivalent useful work. For newly implemented features compare against less and report the original as unsupported, not fast. Profile separately before optimizing.

Time gates run only after `perf-gate-ready` succeeds, and the run remains valid only if `perf-gate-ready -1` succeeds afterwards. Alternate baseline/candidate samples, at least five each. Report median, spread, sample count, workload, configuration, and observed limiter. Fail a regression only when its delta also exceeds observed spread. Memory and artifact size checks always run.

Final targets, prescribed because the repository has no numeric product budgets:

- Median first meaningful screen at most 250 ms and completed interactive action at most 100 ms on the common fixture, excluding intentional producer delay.
- No more than 20 percent median regression against original lz for equivalent baseline-supported workflows.
- New workflows at most 25 percent slower in median than the oracle under equivalent configuration.
- Default-view peak RSS below 64 MiB for the 10 GiB sparse file and the 1 GiB streamed input while viewing the first screen. Historical data may spill; account for it separately. Default runs must not consume an entire open stream just to draw.
- Dist binary at most 2 MiB and clean build elapsed time at most twice the frozen baseline on the same platform. Record all runtime dependency impacts.

These targets gate S14 completion, not feature iteration. Resolve a failed target by optimization or a parked target with evidence and a retry condition. Do not change budgets or comparisons to accept the candidate. If the host remains busy, report `perf gate deferred: host busy`; local feature work may finish but performance completion/release stays blocked.

### Completion

`make parity-close` must run the full applicable deterministic suite, all Rust tests, Clippy, inventory validation, upstream accounting, fixed/commit-seeded fuzz and holdouts, and final reports. At final verification also run `make release` and `make verify-release` on available platforms. Keep the toolchain pin. Missing platform proof is explicit Needs environment debt.

Delete only proven dead implementations, retired lz aliases/default paths, stale fixtures, and temporary compatibility shims. Keep the test oracle, pinned recipes, harness, regression corpus, minimizer, and reports as active testing consumers. The general reference-parity cleanup rule does not remove this campaign's required differential tooling. Search callers, docs, scripts, config, and persisted formats before any deletion. Never delete old user history.

Run `ultrareview` at close within its review scope. Fix demonstrated P1/P2 findings as focused slices with regressions. Retain lower-priority findings in the repository's normal issue record. No unresolved local P1/P2 finding can be called complete. Review does not expand this campaign into unrelated product work.

Finish only when every local deliverable passes or every remainder has a hard-stop cause and retry condition. Report one of:

- **Full tested parity:** every documented entry and applicable platform passes; no unresolved discrepancy, required performance gate, or environment proof remains. State the tested configuration and finite corpus limits.
- **Local deliverables complete; parity incomplete:** all executable local work passes, but named parked behaviors or Needs environment checks remain. List them and avoid exact-parity claims.
- **Incomplete:** locally actionable work remains. Leave a green usable integration branch, committed progress, and the next exact command in Status.

The report includes per-subsystem feature denominators, test outcomes, untested cases, intentional identity differences, parked mismatches/reproductions, platform/configuration evidence, performance results/deferred gates, and remaining uncertainty. Derive historical progress from committed result metadata; keep Status short and use Git history as the log. Update README and AGENTS.md with implemented behavior and working commands, without unsupported parity promises.

## Needs environment

These checks do not block other local work. The executor replaces an entry with its evidence only after actually running it:

| Missing proof | Command on the required environment | Required observation |
| --- | --- | --- |
| Native Linux musl x86_64 and aarch64 | Build the parity-enabled repository Dockerfile on each native Linux runner, then run `make parity-setup`, `make test`, `make lint`, `make parity-test MODE=all`, `make parity-harden SEEDS=0..999`, `make release`, and `make verify-release` inside it. | Matching musl-built 668 oracle, no unresolved differences, static stripped release. Docker emulation on macOS is useful iteration, not native proof. |
| Other supported macOS architecture | On macOS arm64 and x86_64, run the same native setup, correctness, fuzz, and release commands. | Platform-local POSIX regex, termios, signal, terminal-library, filesystem, and Unicode evidence. Record unavailable architecture explicitly. |
| Quiet-host timing | Run `make parity-perf` with passing before/after readiness checks. | Valid time samples plus always-run RSS/size checks. Host load never counts as a pass. |
| Remote CI execution | After a separately authorized push, inspect existing macOS/Linux parity job results. | Green native jobs at the integration revision. This campaign does not push or trigger them. |
| Invalid UTF-8 filenames on this macOS filesystem (EILSEQ before either child starts) | `make parity-test CASE=redirected-nonutf8` on a byte-preserving Unix filesystem; Linux container iteration can qualify local behavior, native platform proof remains separate. | Exact bytes in argv and file data compare; current host result is needs-environment. |

## Execution boundary and non-goals

The owner authorized unattended execution on 2026-11-12, through local deliverables with local commits and no push. This execution grant permits repository implementation, isolated reference downloads/builds, owned fixture effects, local commits, and the parity-specific CI edits O2 describes. No push, publish, deployment, host package installation, external-repository edit, or uncontrolled helper execution is included.

Keep production native Rust. Do not wrap, embed, or execute upstream less in production. Test support may use native terminal libraries and fixture processes. Do not add a production system-library dependency under the guise of a test adapter.

Contract changes covered by O1 include removing the minimalist feature omissions, correcting raw/color behavior, changing wrapping and case defaults, implementing all documented options/commands/integrations, replacing custom prompts/help, accepting arbitrary input bytes and long lines, multiple files, and the 668 history/config formats. Update code, tests, and affected docs together. Do not add compatibility aliases that 668 does not accept. Do not migrate or delete user state.

Scope is Unix behavior under the declared 668 builds, including documented less modes available on Linux/macOS. Windows, DOS, OS/2, standalone replacement utilities such as lesskey/lessecho, features introduced after 668, and behavior of arbitrary third-party helper programs are non-goals. Reading lesskey formats and reproducing less's helper-invocation contract remain required. Do not claim equivalence for every possible terminal or regex build from evidence for one configuration.

Safety of the harness takes precedence over reproducing an oracle crash, destructive side effect, or uncontrolled escape. Preserve a minimized oracle defect as an explicit divergence and park the affected parity entry. Never silently copy or silently fix it. Such a divergence prevents an unqualified full-parity claim.
