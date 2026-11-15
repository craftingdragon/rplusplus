#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""M0 test helper, NOT a Rust preprocessor or installed R++ component."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time


def record(path, kind, args):
    with Path(path).open('a') as f:
        f.write(json.dumps({'kind': kind, 'args': list(map(str, args))})+'\n')


def publish(path, data):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.exists() and path.read_bytes() == data:
        return
    stage = path.with_name(path.name+'.stage')
    stage.write_bytes(data)
    stage.replace(path)


def validate(tree):
    tree = Path(tree)
    manifest = json.loads((tree/'manifest').read_text())
    if (tree/'complete').read_bytes() != (tree/'manifest').read_bytes():
        raise ValueError('incomplete M0 snapshot: completion does not match manifest')
    for name, digest in manifest.items():
        if hashlib.sha256((tree/name).read_bytes()).hexdigest() != digest:
            raise ValueError('incomplete M0 snapshot: '+name)


def emit(specfile):
    s = json.loads(Path(specfile).read_text())
    record(s['trace'], 'preprocess', [specfile])
    source, tree = Path(s['source']), Path(s['tree'])
    tree.mkdir(parents=True, exist_ok=True)
    (tree/'complete').unlink(missing_ok=True)
    selector = (source/'selector').read_text().strip()
    value = (source/selector).read_text().strip()
    payload = {
        'root.rs': (source/'root.rs').read_bytes(),
        'module.rs': ('pub mod nested; pub const VALUE:i32='+value+';\n').encode(),
        'module/nested.rs': b'pub const VALUE:i32=1;\n',
        'data.txt': (source/'data.txt').read_bytes(),
        'map': (source/'mapping.txt').read_bytes(),
    }
    for index, (name, data) in enumerate(payload.items()):
        publish(tree/name, data)
        if index == 0 and (source/'pause').exists():
            (source/'ready').touch()
            while True:
                time.sleep(.01)
    manifest = json.dumps({n: hashlib.sha256(d).hexdigest() for n, d in payload.items()}, sort_keys=True).encode()
    publish(tree/'manifest', manifest)
    def escape(path):
        return str(path).replace('\\', '\\\\').replace(' ', '\\ ').replace('#', '\\#').replace('$', '$$')
    deps = [source/'root.rs', source/'selector', source/selector, source/'data.txt', source/'mapping.txt', Path(specfile)]
    publish(tree/'preprocess.d', (escape(tree/'complete')+': '+ ' '.join(map(escape, deps))+'\n').encode())
    # Scheduling stamp advances even if all content bytes remained identical.
    publish(tree/'complete', manifest)
    (tree/'complete').touch()


def adapter(configfile, args):
    s = json.loads(Path(configfile).read_text())
    kind = args[0] if args else 'empty'
    record(s['trace'], kind, args)
    tree_flags = [a for a in args if a.startswith('--m0-tree=')]
    args = [a for a in args if not a.startswith('--m0-tree=')]
    if tree_flags:
        tree = Path(tree_flags[0].split('=', 1)[1])
        validate(tree)
        if kind == 'compile':
            args = [str(tree/'root.rs') if a.endswith('/root.rs') else a for a in args]
    if '--rustc' in args:
        args[args.index('--rustc')+1] = s['rustc_wrapper']
    raise SystemExit(subprocess.call([s['driver'], *args]))


if __name__ == '__main__':
    try:
        if sys.argv[1] == 'emit':
            emit(sys.argv[2])
        elif sys.argv[1] == 'adapter':
            adapter(sys.argv[2], sys.argv[3:])
        elif sys.argv[1] == 'tool':
            s = json.loads(Path(sys.argv[2]).read_text())
            record(s['trace'], sys.argv[3], sys.argv[4:])
            raise SystemExit(subprocess.call([s[sys.argv[3]], *sys.argv[4:]]))
        else:
            raise ValueError('unknown M0 helper operation')
    except (OSError, ValueError) as error:
        print('M0 snapshot rejected: '+str(error), file=sys.stderr)
        raise SystemExit(1)
