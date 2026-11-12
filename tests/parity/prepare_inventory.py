#!/usr/bin/env python3
"""Regenerate the initial manual inventory only before it has result annotations."""
import hashlib
import json
from pathlib import Path
import re
import sys
sys.dont_write_bytecode = True
import setup

BASE = setup.BASE / 'ordinary'
DEST = Path(__file__).parent
if (DEST / 'inventory.json').exists():
    raise RuntimeError('inventory already exists; edit canonical entries without discarding results')

def read_lt(path):
    data = dict(env={}, args=[], files=[], events=[])
    with path.open('rb') as source:
        while line := source.readline():
            if line.startswith(b'F '):
                match = re.fullmatch(rb'F "([^"]+)" (\d+)\n', line)
                data['files'].append((match[1].decode(), source.read(int(match[2]))))
            elif line.startswith(b'E '):
                pieces = line.decode().split('"')
                data['env'][pieces[1]] = pieces[3]
            elif line.startswith(b'A '):
                data['args'] = re.findall(r'"([^"]+)"', line.decode())
            elif line.startswith(b'+'):
                data['events'].append(dict(key=int(line[1:], 16)))
            elif line.startswith(b'='):
                if data['events']:
                    data['events'][-1]['original_screen'] = line[1:].rstrip(b'\n').decode()
    return data

entries = []
section = ''
section_family = {'NAME':'invocation', 'SYNOPSIS':'invocation', 'DESCRIPTION':'sources', 'COMMANDS':'navigation', 'OPTIONS':'invocation', 'LINE EDITING':'configuration', 'KEY BINDINGS':'configuration', 'INPUT PREPROCESSOR':'integrations', 'NATIONAL CHARACTER SETS':'rendering', 'PROMPTS':'rendering', 'SECURITY':'integrations', 'COMPATIBILITY WITH MORE':'configuration', 'ENVIRONMENT VARIABLES':'configuration'}
for line_number, line in enumerate((BASE / 'less.nro').read_text().splitlines(), 1):
    if line.startswith('.SH '):
        section = line[4:].strip('"')
    if (line.startswith('.SH ') or line.startswith('.IP ')) and section in section_family:
        family = section_family[section]
        if section == 'COMMANDS':
            if 206 <= line_number <= 337: family = 'search'
            elif 341 <= line_number <= 373 or line_number in [108,112,127,139]: family = 'sessions'
            elif line_number in [375,378,456,462,474,478,489,492]: family = 'integrations'
        owner = dict(invocation='S04',sources='S05',rendering='S06',navigation='S07',search='S08',sessions='S09',configuration='S10',integrations='S11')[family]
        excluded = section == 'ENVIRONMENT VARIABLES' and line_number in [2340,2344,2391,2393]
        entries.append(dict(id=f'man-{line_number:04}',family=family,title=line[4:].strip('"'),anchor=dict(file='less.nro',line=line_number,text=line),slice=owner,platforms=[] if excluded else ['macos','linux-musl'],cases=[],implementation='unassessed',evidence='not tested',cause='Outside Unix scope: platform-specific documented configuration' if excluded else 'No distinguishing differential scenario yet'))
entries.append(dict(id='source-redirected',family='sources',title='Nonterminal copy and diagnostics',anchor=dict(file='main.c',line=1,text=(BASE/'main.c').read_text().splitlines()[0]),slice='S01',platforms=['macos','linux-musl'],cases=[case['id'] for case in json.loads((DEST/'cases.json').read_text())],implementation='implemented',evidence='compatible in the named tested scenarios',cause='Other platforms remain unproved'))
for ident, file, label, owner, family in [('source-bytes','ch.c','Byte-preserving seekable/nonseekable sources','S05','sources'),('terminal-lifecycle','signal.c','Signal, suspend, resume and terminal ownership','S04','lifecycle'),('input-boundaries','ttyin.c','Keyboard decoding and read interruption','S04','lifecycle')]:
    entries.append(dict(id=ident,family=family,title=label,anchor=dict(file=file,line=1,text=(BASE/file).read_text().splitlines()[0]),slice=owner,platforms=['macos','linux-musl'],cases=[],implementation='unassessed',evidence='not tested',cause='No distinguishing differential scenario yet'))
cases = json.loads((DEST / 'cases.json').read_text())
fixture = ''.join(f'{n:06} reference parity\n' for n in range(1,81)).encode()
cases.append(dict(id='startup-quit',family='invocation',admitted=False,source='pty',fixture=list(fixture),args=['input'],env={},size=[80,24],locale='C',events=[dict(type='checkpoint',ready='000001'),dict(type='send',bytes=[113])]))
provenance=[]
for name, ident, steps, family in [('version.c','upstream-version-navigation',6,'navigation'),('seq200','upstream-seq-search',3,'search')]:
    path=BASE/'lesstest/lt'/f'{name}.lt'
    original=read_lt(path)
    selected=original['events'][:steps]
    events=[]
    content=original['files'][0][1]
    first=content.splitlines()[0].decode()
    events.append(dict(type='checkpoint',ready=first))
    if family=='navigation':
        for n,event in enumerate(selected):
            events.append(dict(type='send',bytes=list(chr(event['key']).encode())))
            # Each translated j changes the first visible source line.
            ready=content.splitlines()[n+1].decode() or content.splitlines()[n+2].decode()
            events.append(dict(type='checkpoint',ready=ready))
    else:
        events.append(dict(type='send',bytes=list(''.join(chr(e['key']) for e in selected).encode())))
        events.append(dict(type='checkpoint',ready='30'))
    events.append(dict(type='send',bytes=[113]))
    cases.append(dict(id=ident,family=family,admitted=False,source='pty',fixture=list(content),args=['input'],env={},size=[int(original['env']['COLUMNS']),int(original['env']['LINES'])],locale='C',events=events))
    provenance.append(dict(case=ident,upstream_file=f'lesstest/lt/{name}.lt',sha256=hashlib.sha256(path.read_bytes()).hexdigest(),original_args=original['args'],original_env=original['env'],selected_events=selected,adaptation='Embedded file renamed input in independent worlds; custom termcap replaced by xterm-256color; ordinary-oracle VT checkpoints replace instrumentation screen encoding. Selected command subsequence completes with q.'))
upstream=[]
for path in sorted((BASE/'lesstest/lt').glob('*.lt')):
    links=[p['case'] for p in provenance if p['upstream_file'].endswith('/'+path.name)]
    upstream.append(dict(id=path.stem,file='lesstest/lt/'+path.name,sha256=hashlib.sha256(path.read_bytes()).hexdigest(),instrumented='not tested',cases=links,disposition='partial translation' if links else 'pending translation',cause='Remaining commands/custom termcap checkpoints require generic VT scenarios in S12'))
for entry in entries:
    if entry['id']=='man-0066':entry['cases']=['upstream-version-navigation']
    if entry['id']=='man-0206':entry['cases']=['upstream-seq-search']
    if entry['id']=='man-0005':entry['cases']=['startup-quit']
(DEST/'inventory.json').write_text(json.dumps(dict(reference_sha256=setup.SHA256,entries=entries,upstream=upstream),indent=2)+'\n')
(DEST/'cases.json').write_text(json.dumps(cases,indent=2)+'\n')
(DEST/'provenance.json').write_text(json.dumps(provenance,indent=2)+'\n')
(DEST/'LESS-LICENSE').write_bytes((BASE/'LICENSE').read_bytes())
print(len(entries),'manual/source entries;',len(upstream),'upstream dispositions;',len(cases),'scenarios')
