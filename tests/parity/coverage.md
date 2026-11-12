# less 668 parity coverage

Generated from `inventory.json` and fingerprint-checked full current/baseline results. Counts are manual/source entries, not proof of all interactions. Platform: macOS arm64, POSIX regex, system ncurses. Four platform-only manual entries are outside Unix scope. Other platforms and S14 performance remain unproved.

| Family | Applicable entries | Implemented | Tested by named cases | Compatible in named scenarios |
| --- | ---: | ---: | ---: | ---: |
| configuration | 69 | 0 | 1 | 1 |
| integrations | 22 | 0 | 0 | 0 |
| invocation | 133 | 0 | 12 | 12 |
| lifecycle | 3 | 0 | 2 | 1 |
| navigation | 53 | 0 | 2 | 2 |
| rendering | 55 | 0 | 3 | 3 |
| search | 28 | 0 | 1 | 1 |
| sessions | 10 | 0 | 0 | 0 |
| sources | 8 | 1 | 8 | 7 |

Untested entries stay in the denominator. A named scenario does not cover every boundary of its linked manual entry. See the canonical inventory for every anchor, cause, owner and platform.

| Corpus | Pass | Mismatch | Harness error | Needs environment | Total |
| --- | ---: | ---: | ---: | ---: | ---: |
| baseline | 3 | 140 | 0 | 1 | 144 |
| current | 142 | 1 | 0 | 1 | 144 |

All 14 instrumented upstream tests pass against instrumented less. Two have partial generic VT translations; twelve await translation. These are oracle validation results, not lz passes. Discovery mismatches remain strict failures in `make parity-test MODE=all`. Identity normalization changes only the exact program token in recognized option-diagnostic help hints. Other diagnostic bytes and all other locations remain strict.
