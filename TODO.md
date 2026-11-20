Prefix: LZP
Next: 4

# Open work
### LZP-1 · P3 · Measure complete pager journeys and resource growth

- **Area:** bench
- **Where:** benches/bench.rs; docs/benchmarks.md
- **Problem:** The synthetic library benchmarks do not verify completed interactive or redirected pager work, startup, memory growth, or long-line and stream behavior; regressions in those outcomes can be missed.
- **Recommendation:** Add representative end-to-end workloads with output or terminal oracles and scoped resource measurements.
- **Fix with:** refactor-benchmarks
- **Effort:** L

### LZP-2 · P3 · Establish valid comparative performance gates and attribution

- **Area:** bench
- **Where:** benches/baseline.json; benches/driver.py; Makefile
- **Problem:** The baseline has synthetic results without interleaved reference comparisons or budgets; CPU, memory, I/O and trace attribution for pager journeys is incomplete, so a regression can go undetected.
- **Recommendation:** Define matched reference budgets and host-valid interleaved runs, then document low-overhead profiling and syscall attribution for each journey.
- **Fix with:** bench-pyramid
- **Effort:** L

### LZP-3 · P3 · Verify benchmark cleanup and input identity

- **Area:** bench
- **Where:** benches/bench.rs; benches/driver.py
- **Problem:** Fixture cleanup is best effort and runs do not verify leftover processes or record complete artifact and host identity; stale state can obscure a regression.
- **Recommendation:** Verify cleanup and capture the minimum artifact, source and host identity needed to reproduce a run.
- **Fix with:** bench-pyramid
- **Effort:** M

