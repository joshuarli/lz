#!/usr/bin/env python3
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tarfile
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
TOOLS = ROOT / '.work/tools/less-668'
ARCHIVE = TOOLS / 'less-668.tar.gz'
SHA256 = '2819f55564d86d542abbecafd82ff61e819a3eec967faa36cd3e68f1596a44b8'
PLATFORM = platform.system().lower() + '-' + platform.machine()
BASE = TOOLS / PLATFORM
os.environ['TMPDIR'] = str(ROOT / '.work/tmp')

def run(args, cwd=None, **kwargs):
    return subprocess.run(args, cwd=cwd, check=True, **kwargs)

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def verify():
    if not ARCHIVE.is_file() or digest(ARCHIVE) != SHA256:
        raise RuntimeError('missing or corrupt pinned archive; run make parity-setup')
    metadata = json.loads((BASE / 'metadata.json').read_text())
    if metadata['archive_sha256'] != SHA256:
        raise RuntimeError('reference metadata has wrong archive')
    for kind in ['ordinary', 'instrumented']:
        binary = BASE / kind / 'less'
        version = run([str(binary), '--version'], capture_output=True).stdout.decode()
        if 'less 668 (POSIX regular expressions)' not in version:
            raise RuntimeError('wrong reference version or regex backend')
        if ('LESSTEST' in version) != (kind == 'instrumented'):
            raise RuntimeError('wrong reference instrumentation')
        if digest(binary) != metadata[kind + '_sha256']:
            raise RuntimeError('reference executable changed')
        helper = BASE / kind / 'lessecho'
        if run([str(helper), '--version'], capture_output=True).stdout.strip() != b'1.15':
            raise RuntimeError('wrong reference lessecho version')
    return metadata

if __name__ == '__main__':
    import sys
    if sys.argv[1:] == ['--verify']:
        verify()
        print(BASE / 'ordinary/less')
        sys.exit(0)
    if platform.system() not in ['Darwin', 'Linux']:
        raise RuntimeError('unsupported reference platform')
    if platform.system() == 'Linux' and not Path('/etc/alpine-release').exists():
        raise RuntimeError('Linux reference requires repository Alpine/musl environment')
    TOOLS.mkdir(parents=True, exist_ok=True)
    if not ARCHIVE.exists():
        cached = ROOT / '.work/tmp/preflight-reference/less-668.tar.gz'
        if cached.is_file() and digest(cached) == SHA256:
            shutil.copyfile(cached, ARCHIVE)
        else:
            pending = ARCHIVE.with_suffix('.download')
            try:
                with urllib.request.urlopen('https://www.greenwoodsoftware.com/less/less-668.tar.gz', timeout=30) as response, pending.open('wb') as dest:
                    shutil.copyfileobj(response, dest)
                if digest(pending) != SHA256:
                    raise RuntimeError('reference archive checksum mismatch')
                pending.replace(ARCHIVE)
            finally:
                pending.unlink(missing_ok=True)
    if digest(ARCHIVE) != SHA256:
        raise RuntimeError('reference archive checksum mismatch')
    if (BASE / 'metadata.json').is_file():
        verify()
        print(BASE / 'ordinary/less')
        sys.exit(0)
    BASE.mkdir(parents=True, exist_ok=True)
    config = ['--with-regex=posix', '--with-editor=vi', '--sysconfdir=' + str(BASE / 'etc')]
    for kind in ['ordinary', 'instrumented']:
        dest = BASE / kind
        if dest.exists():
            raise RuntimeError('partial reference tree retained; inspect and move it before retry: ' + str(dest))
        with tarfile.open(ARCHIVE) as archive:
            members = archive.getmembers()
            for member in members:
                parts = Path(member.name).parts
                if not parts or parts[0] != 'less-668' or '..' in parts or Path(member.name).is_absolute() or not (member.isfile() or member.isdir()):
                    raise RuntimeError('unsafe archive member: ' + member.name)
            staging = BASE / ('extract-' + kind)
            staging.mkdir()
            archive.extractall(staging, members=members, filter='data')
            (staging / 'less-668').rename(dest)
            staging.rmdir()
        with (dest / 'configure.log').open('wb') as log:
            run(['./configure', *config], cwd=dest, stdout=log, stderr=subprocess.STDOUT)
        with (dest / 'build.log').open('wb') as log:
            run(['make', '-j2', *(['LESSTEST=1'] if kind == 'instrumented' else [])], cwd=dest, stdout=log, stderr=subprocess.STDOUT)
        if kind == 'instrumented':
            with (dest / 'lesstest/build.log').open('wb') as log:
                run(['make'], cwd=dest / 'lesstest', stdout=log, stderr=subprocess.STDOUT)
    metadata = dict(archive_sha256=SHA256, source_commit='e77e1176c80cca989694818866ae6cd7e7707161', tag_object='210fb607972a99957294a603eea50fc9c67cca30', platform=PLATFORM, configure=config, compiler=run(['cc', '--version'], capture_output=True).stdout.decode(), flags='configure defaults; make -j2; LESSTEST=1 only instrumented', libc=platform.libc_ver(), regex='POSIX', locale='C', term='xterm-256color')
    for kind in ['ordinary', 'instrumented']:
        metadata[kind + '_sha256'] = digest(BASE / kind / 'less')
    link = ['otool', '-L'] if platform.system() == 'Darwin' else ['ldd']
    metadata['terminal_libraries'] = run([*link, str(BASE / 'ordinary/less')], capture_output=True).stdout.decode()
    info = run(['infocmp', 'xterm-256color'], capture_output=True).stdout
    metadata['terminfo_sha256'] = hashlib.sha256(info).hexdigest()
    (BASE / 'metadata.json').write_text(json.dumps(metadata, indent=2) + '\n')
    verify()
    print(BASE / 'ordinary/less')
