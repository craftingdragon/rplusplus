#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Offline v0.2 release gate with preinstalled, pinned tool paths."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile

from generators import ROOT

EXPECTED = {'minimum_cmake': '3.25.0', 'current_cmake': '4.5.1',
            'minimum_rustc': '1.85.0', 'previous_rustc': '1.99.1',
            'current_rustc': '1.100.0'}


def record(output, data):
    with tempfile.NamedTemporaryFile(mode='w', dir=output.parent, delete=False) as stream:
        json.dump(data, stream, indent=2)
        stream.write('\n')
    Path(stream.name).replace(output)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for role in EXPECTED:
        parser.add_argument('--'+role.replace('_', '-'), required=True, type=Path)
    parser.add_argument('--source-manifest', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    parser.add_argument('--jobs', type=int, default=1)
    opts = parser.parse_args()
    opts.output.parent.mkdir(parents=True, exist_ok=True)
    result = {'schema': 1, 'status': 'blocked', 'inventory': {},
              'blockers': ['Validation is incomplete']}
    record(opts.output, result)
    failures = []
    try:
        manifest = json.loads(opts.source_manifest.read_text())
        revision, checksum = manifest['revision'], manifest['source_sha256']
        if not (re.fullmatch(r'[0-9a-f]{40,64}', revision) and re.fullmatch(r'[0-9a-f]{64}', checksum)):
            raise ValueError('Invalid candidate identity')
        if manifest['version'] != '0.2.0' or manifest['source_archive'] != 'rplusplus-0.2.0.tar.gz':
            raise ValueError('Expected the v0.2.0 source archive')
        with (opts.source_manifest.parent / manifest['source_archive']).open('rb') as archive:
            if hashlib.sha256(archive.read()).hexdigest() != checksum:
                raise ValueError('Source archive checksum does not match the manifest')
        result.update(revision=revision, source_sha256=checksum)
    except (OSError, KeyError, TypeError, ValueError) as error:
        failures.append(f'Candidate source manifest: {error}')
    paths = {}
    for role, version in EXPECTED.items():
        path = getattr(opts, role)
        paths[role] = path
        try:
            if not path.is_absolute() or not path.is_file():
                raise FileNotFoundError(f'Missing absolute installed tool path: {path}')
            actual = subprocess.check_output([path, '--version'], text=True, stderr=subprocess.STDOUT,
                                             timeout=15).splitlines()[0]
            cmake = role.endswith('_cmake')
            pattern = r'cmake version (\S+)' if cmake else r'rustc (\S+)(?: .*)?'
            match = re.fullmatch(pattern, actual)
            if not match or match[1] != version:
                raise ValueError(f'expected stable {version}, found {actual}')
            entry = {'path': str(path), 'version': actual}
            if cmake:
                ctest = path.with_name('ctest')
                identity = subprocess.check_output([ctest, '--version'], text=True, timeout=15).splitlines()[0]
                if identity != 'ctest version '+version:
                    raise ValueError(f'matching CTest {version} required, found {identity}')
                entry['ctest'] = identity
            else:
                entry['identity'] = subprocess.check_output([path, '-vV'], text=True, timeout=15)
                if f'release: {version}' not in entry['identity'].splitlines():
                    raise ValueError('rustc identity does not match stable release')
            result['inventory'][role] = entry
        except (OSError, ValueError, IndexError, subprocess.SubprocessError) as error:
            failures.append(f'{role}: {error}')
    if failures:
        result['blockers'] = failures
        record(opts.output, result)
        print('\n'.join(failures), file=sys.stderr)
        return 1
    record(opts.output, result)
    with tempfile.TemporaryDirectory(prefix='rpp release matrix ') as tmp:
        report_path = Path(tmp)/'matrix.json'
        cmd = [sys.executable, str(ROOT/'tests/compatibility_matrix.py'), '--require-qt',
               '--exact-release-tests', '--jobs', str(opts.jobs), '--output', str(report_path)]
        for role, path in paths.items():
            cmd.extend(['--cmake' if role.endswith('_cmake') else '--rustc', str(path)])
        status = subprocess.call(cmd, env=dict(os.environ, RPP_SOURCE_REVISION=revision,
                                               RPP_SOURCE_SHA256=checksum))
        if report_path.is_file():
            result.update(json.loads(report_path.read_text()))
    cases = result.get('results', [])
    if status == 0 and len(cases) == 6 and all(e['status'] == 'passed' for e in cases):
        result.update(status='passed', blockers=[])
    else:
        result.update(status='failed' if not cases or any(e['status'] == 'failed' for e in cases) else 'blocked',
                      blockers=['Required compatibility matrix did not pass in full'])
    record(opts.output, result)
    return int(result['status'] != 'passed')


if __name__ == '__main__':
    sys.exit(main())
