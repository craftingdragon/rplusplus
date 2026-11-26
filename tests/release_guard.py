#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Exercise release preflight, including rejection of stale reports."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

from compatibility_matrix import verify_inventory
from generators import ROOT
from release_gate import EXPECTED


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for role in EXPECTED:
        parser.add_argument('--'+role.replace('_', '-'), required=True, type=Path)
    parser.add_argument('--source-manifest', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix='rpp release guard ') as temporary:
        root = Path(temporary)
        report = root/'report.json'
        checks = []
        for reason, overrides in (
            ('missing compiler', {'current_rustc': root/'absent-rustc'}),
            ('wrong CMake version', {'current_cmake': args.minimum_cmake}),
            ('wrong stable compiler version', {'current_rustc': args.previous_rustc}),
        ):
            report.write_text('{"status":"passed","stale":true}')
            command = [sys.executable, str(ROOT/'tests/release_gate.py'), '--source-manifest',
                       str(args.source_manifest), '--output', str(report)]
            for role in EXPECTED:
                command.extend(['--'+role.replace('_', '-'), str(overrides.get(role, getattr(args, role)))])
            result = subprocess.run(command, capture_output=True, text=True, timeout=90)
            data = json.loads(report.read_text())
            assert result.returncode and data['status'] == 'blocked' and data['blockers'], data
            assert 'stale' not in data, data
            checks.append(reason)
        for role in ('minimum_cmake', 'current_cmake'):
            ctest = getattr(args, role).with_name('ctest')
            try:
                verify_inventory(ctest, root, os.environ)
            except RuntimeError as error:
                assert 'Incorrect CTest inventory' in str(error), error
            else:
                raise AssertionError('Empty CTest inventory accepted')
            checks.append(f'{role}: empty CTest inventory')
        args.output.write_text(json.dumps({'schema': 1, 'status': 'passed', 'checks': checks}, indent=2)+'\n')


if __name__ == '__main__':
    main()
