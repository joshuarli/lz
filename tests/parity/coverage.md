# less 668 parity coverage

Generated from `inventory.json` and fingerprint-checked full current/baseline results. Counts are manual/source entries, not proof of all interactions. Platform: macOS arm64, POSIX regex, system ncurses. Four platform-only manual entries are outside Unix scope. Other platforms and S14 performance remain unproved.

| Family | Applicable entries | Implemented | Tested by named cases | Compatible in named scenarios |
| --- | ---: | ---: | ---: | ---: |
| configuration | 69 | 6 | 13 | 13 |
| integrations | 22 | 4 | 6 | 6 |
| invocation | 133 | 13 | 51 | 50 |
| lifecycle | 3 | 0 | 3 | 2 |
| navigation | 53 | 1 | 29 | 28 |
| rendering | 55 | 36 | 20 | 17 |
| search | 28 | 6 | 13 | 13 |
| sessions | 10 | 0 | 6 | 4 |
| sources | 8 | 2 | 8 | 8 |

Untested entries stay in the denominator. A named scenario does not cover every boundary of its linked manual entry. See the canonical inventory for every anchor, cause, owner and platform.

| Corpus | Pass | Mismatch | Harness error | Needs environment | Total |
| --- | ---: | ---: | ---: | ---: | ---: |
| baseline | 3 | 1088 | 1 | 1 | 1093 |
| current | 1089 | 3 | 0 | 1 | 1093 |

All 14 instrumented upstream tests pass against instrumented less. Two have partial generic VT translations; twelve await translation. These are oracle validation results, not lz passes. Discovery mismatches remain strict failures in `make parity-test MODE=all`. Identity normalization changes only the exact program token in recognized option-diagnostic help hints. Other diagnostic bytes and all other locations remain strict.
