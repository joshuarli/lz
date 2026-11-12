#!/usr/bin/env python3
import json
import os
from pathlib import Path
import signal
import subprocess
import tempfile
import sys
sys.dont_write_bytecode = True
import setup

setup.verify()
base = setup.BASE / 'instrumented'
with tempfile.TemporaryDirectory(dir=setup.ROOT / '.work/tmp', prefix='upstream-') as world:
    world = Path(world)
    command = [str(base / 'lesstest/runtest'), '-l', str(base / 'less'), '-r', str(world / 'run'), 'lt']
    env = dict(HOME=str(world), TMPDIR=str(world), PATH='/usr/bin:/bin', LC_ALL='C', TERM='xterm-256color', LESSHISTFILE='-')
    with (world / 'output').open('wb') as output:
        child = subprocess.Popen(command, cwd=base / 'lesstest', env=env, stdout=output, stderr=subprocess.STDOUT, start_new_session=True)
        try:
            code = child.wait(timeout=30)
        finally:
            try:
                os.killpg(child.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            child.wait()
    trace = (world / 'output').read_text()
    print(trace)
    names = [entry.name for entry in (base / 'lesstest/lt').glob('*.lt')]
    if code or 'RAN  14 tests with 0 errors' not in trace or any('TEST ' + name not in trace for name in names):
        failure = setup.ROOT / '.work/upstream-failure.txt'
        failure.write_text(trace)
        raise RuntimeError('upstream corpus failed; retained ' + str(failure))
    metadata = json.loads((setup.BASE / 'metadata.json').read_text())
    result = dict(command=command, metadata=metadata, outcome='pass', files=sorted(names), trace=trace)
    (setup.ROOT / '.work/upstream-results.json').write_text(json.dumps(result, indent=2)+'\n')
