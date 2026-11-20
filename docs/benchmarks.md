# Benchmarks

`make bench-list` lists workloads. `make bench-check TIER=change WORKLOAD=search_hit_literal` checks selected prerequisites. `make bench TIER=edit` tests selection; `make bench TIER=change WORKLOAD=search_hit_literal` runs a short measurement. `TIER` is an inclusive ceiling (edit, change, scenario); `WORKLOAD`, `AREA` and `SKIP` take space-separated exact names. `DRY_RUN=1` prints commands without writing. Empty and unknown selections fail. `make bench` defaults to Change. Full is manual only and requires explicit `TIER=full`; it currently contains the same registered workloads as Scenario. The legacy complete baseline command is `TMPDIR=$PWD/.work/tmp rustybench baseline --root "$PWD" --baseline "$PWD/benches/baseline.json" -- cargo bench --bench bench`; it overwrites the baseline. `make bench-syscalls` and `make rustybench` remain available. Syscall tracing uses Linux Docker and strace and is diagnostic, not timing.

Python 3, the pinned Rust toolchain, Cargo dependencies and 512 MiB free disk are required. Cargo rebuilds stale artifacts; the 100,000-line fixture is generated per workload and cleaned up by the harness. No external service is used. Results are in `.work/bench/<date>/`, with per-workload logs and a summary. Change uses one sample and one iteration; Scenario uses rustybench defaults and requires a ready host. Costs are estimated seconds for Edit and minutes per Change or Scenario workload, excluding a cold build. A host gate is checked before and after runs. A completed run has no performance verdict: there are no comparative budgets or interleaved reference runs.

## Outcomes and gaps

| Outcome | Existing evidence | Gap |
| --- | --- | --- |
| Open, render and navigate a file; startup latency | None | No end-to-end terminal workload or correctness oracle. |
| Redirected copying, pipes and long lines | None | No throughput, completion or memory-growth workload. |
| Search and filter | `search_*`: synthetic file and cached lines | No interactive journey or checked result in the timed closure. |
| Display | `strip_ansi`, `visible_width`, `truncate_to_width`: synthetic lines | No frame-rendering journey or checked output in the timed closure. |
| Line indexing | `buffer_read_all_100k_lines`: generated fixed-width file | No checked line count in the timed closure or large-line growth check. |
| Allocation traffic | rustybench allocator records counts and bytes in the benchmark process | Not peak RSS or process-tree memory. |
| CPU, I/O, syscall and byte attribution | `make bench-syscalls` offers Linux strace diagnostics | No per-journey CPU sampling, footprint attribution, operation traces or byte counters. |
| Artifact size | None in this suite | No benchmark size gate. |

The stored baseline has one warmup and three measured suite runs on macOS ARM64; it is a historical anchor, not an interleaved comparison or a budget. The workloads use generated repetitive data, and `black_box` does not validate results. The suite does not prove tail latency. Sampling and strace add overhead and must remain separate from timing. Cargo rebuilds may dominate cold runs, and the generated fixture uses the uncontrolled OS page cache. The suite does not track leftover processes or verify cleanup independently; benchmark fixture cleanup is best effort. The cheapest attribution starting points are a CPU sampler on the pager, scoped peak RSS, and a separate syscall trace with explicit operation boundaries. DSLP-12 covers representative journeys and resource growth; DSLP-13 covers comparisons and attribution. Both are missing-evidence P3 gaps, not demonstrated regressions.

On 2026-11-19 the Edit driver test passed and Change `search_hit_literal` completed one sample and reported allocation counters. The readiness check reported a busy host before and after; timing is deferred, and these numbers are not a performance verdict. Scenario and Full were not run. The next useful step is DSLP-12: add a checked pager journey before using this suite as a regression gate.
