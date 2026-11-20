#!/usr/bin/env python3
import argparse
import datetime
from pathlib import Path
import shutil
import subprocess
import sys
import time
import unittest

ROOT = Path(__file__).resolve().parent.parent
NAMES = {
    'buffer_read_all_100k_lines': 'buffer',
    'search_hit_literal': 'search',
    'search_miss_literal': 'search',
    'search_hit_regex': 'search',
    'search_filter_scan_cached': 'search',
    'strip_ansi_colored_line': 'display',
    'visible_width_unicode_line': 'display',
    'truncate_to_width_colored_line': 'display',
}
TIERS = ('edit', 'change', 'scenario', 'full')


def select(tier, names, areas, skips):
    for values, valid, label in ((names, set(NAMES) | {'driver'}, 'workload'), (areas, set(NAMES.values()) | {'driver'}, 'area'), (skips, set(NAMES) | {'driver'}, 'skip')):
        unknown = set(values) - set(valid)
        if unknown:
            raise ValueError(f'unknown {label}: {", ".join(sorted(unknown))}')
    available = ['driver'] + (list(NAMES) if tier != 'edit' else [])
    selected = [n for n in available if (not names or n in names) and (not areas or (NAMES.get(n, 'driver') in areas)) and n not in skips]
    if not selected:
        raise ValueError('selection matches no workloads')
    return selected


def command(name, tier):
    if name == 'driver':
        return [sys.executable, 'benches/driver.py', '--self-test']
    samples = ['--sample-count', '1', '--sample-size', '1'] if tier == 'change' else []
    return ['cargo', 'bench', '--bench', 'bench', '--', 'bench::' + name, '--exact', *samples]


def preflight(selected):
    problems = []
    for tool in (['cargo', 'rustc'] if any(n != 'driver' for n in selected) else []):
        if not shutil.which(tool):
            problems.append(f'{tool} missing: install the pinned Rust toolchain; needed by {", ".join(selected)}')
    if shutil.disk_usage(ROOT).free < 512 * 1024 * 1024:
        problems.append(f'less than 512 MiB free: free disk space; needed by {", ".join(selected)}')
    for problem in problems:
        print(problem, file=sys.stderr)
    if problems:
        return False
    print('preflight: tools and disk available; cargo rebuilds stale artifacts; fixtures generated locally')
    for tool in (['cargo', 'rustc'] if any(n != 'driver' for n in selected) else []):
        subprocess.run([tool, '--version'], check=True)
    print('host readiness is checked before and after execution; check alone does not reserve the host')
    return True


class SelectionTests(unittest.TestCase):
    def test_selection(self):
        self.assertEqual(select('edit', [], [], []), ['driver'])
        self.assertEqual(select('change', ['search_hit_literal'], [], []), ['search_hit_literal'])
        self.assertEqual(select('change', [], ['display'], ['strip_ansi_colored_line']), ['visible_width_unicode_line', 'truncate_to_width_colored_line'])
        for args in [('change', ['nope'], [], []), ('edit', [], ['search'], []), ('change', [], ['search_hit_literal'], [])]:
            with self.assertRaises(ValueError):
                select(*args)


def main():
    p = argparse.ArgumentParser()
    p.add_argument('--tier', choices=TIERS, default='change')
    p.add_argument('--workload', action='append', default=[])
    p.add_argument('--area', action='append', default=[])
    p.add_argument('--skip', action='append', default=[])
    p.add_argument('--check', action='store_true')
    p.add_argument('--list', action='store_true')
    p.add_argument('--dry-run', action='store_true')
    p.add_argument('--self-test', action='store_true')
    a = p.parse_args()
    if a.self_test:
        unittest.main(argv=[sys.argv[0]])
        return
    if a.list:
        print('driver: edit, area=driver, estimated seconds, selection tests')
        for n, area in NAMES.items():
            print(f'{n}: change, area={area}, estimated minutes, synthetic rustybench measurement')
        print('scenario and full: same workloads, default samples; full is manual only')
        return
    try:
        selected = select(a.tier, a.workload, a.area, a.skip)
    except ValueError as e:
        p.error(str(e))
    for n in ['driver', *NAMES]:
        if n not in selected:
            print(f'excluded {n}: selection or tier')
    if a.dry_run:
        for n in selected:
            print(f'{n}: {subprocess.list2cmdline(command(n, a.tier))}')
        return
    if not preflight(selected):
        sys.exit(1)
    if a.check:
        gate = shutil.which('perf-gate-ready')
        ready = bool(gate and subprocess.run([gate, '-1']).returncode == 0)
        print('host readiness: ' + ('ready' if ready else 'busy or unavailable; timing deferred'))
        return
    gate = shutil.which('perf-gate-ready')
    ready = bool(gate and subprocess.run([gate, '-1']).returncode == 0)
    if a.tier in ('scenario', 'full') and not ready:
        print(f'{a.tier} timing deferred: host busy or readiness tool unavailable', file=sys.stderr)
        sys.exit(1)
    out = ROOT / '.work' / 'bench' / (datetime.datetime.now().strftime('%Y%m%d-%H%M%S-%f'))
    out.mkdir(parents=True)
    summary = []
    for n in selected:
        start = time.monotonic()
        with (out / f'{n}.log').open('w') as log:
            result = subprocess.run(command(n, a.tier), cwd=ROOT, stdout=log, stderr=subprocess.STDOUT)
        duration = time.monotonic() - start
        status = result.returncode
        if n != 'driver' and status == 0 and not all(text in (out / f'{n}.log').read_text() for text in (n, 'samples', 'alloc:')):
            print(f'{n}: benchmark missing from output', file=sys.stderr)
            status = 1
        summary.append((n, status, duration))
        print(f'{n}: exit {status}, {duration:.2f}s, log {out / (n + ".log")}')
    after = bool(gate and subprocess.run([gate, '-1']).returncode == 0)
    validity = 'host gate passed; no performance budget or interleaved reference, inconclusive' if ready and after else 'timing deferred: host busy or readiness unavailable'
    lines = [f'timing: {validity}', 'completion is not a performance verdict'] + [f'{n}: exit {rc}, {duration:.2f}s' for n, rc, duration in summary] + [f'excluded {n}: selection or tier' for n in ['driver', *NAMES] if n not in selected]
    (out / 'summary.txt').write_text('\n'.join(lines) + '\n')
    print('\n'.join(lines))
    if any(rc for _, rc, _ in summary):
        sys.exit(1)


if __name__ == '__main__':
    main()
