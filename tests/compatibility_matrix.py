#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Run the complete local compatibility gate for explicitly installed tools."""
import argparse
from concurrent.futures import ThreadPoolExecutor
from datetime import datetime, timezone
import itertools
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
RELEASE_TESTS = frozenset((
    'driver-unit', 'driver-failures', 'toolchain-errors', 'lexer-regression',
    'preprocess-regression', 'preprocess-cli', 'preprocess-tree', 'snapshot-compile',
    'preprocess-cmake', 'generator-compatibility', 'qt-notes-installed',
))


def verify_inventory(ctest, build, env):
    inventory = json.loads(run([ctest, '--test-dir', build, '--show-only=json-v1'], env))
    names = [test['name'] for test in inventory['tests']]
    if len(names) != len(RELEASE_TESTS) or set(names) != RELEASE_TESTS:
        raise RuntimeError(f'Incorrect CTest inventory; missing {sorted(RELEASE_TESTS-set(names))}, '
                           f'extra {sorted(set(names)-RELEASE_TESTS)}, found {len(names)} entries')
    return sorted(names)


def run(command, env):
    result = subprocess.run(list(map(str, command)), env=env, text=True,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=900)
    if result.returncode:
        raise RuntimeError(f'{command}\n{result.stdout}')
    return result.stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cmake', action='append', required=True)
    parser.add_argument('--rustc', action='append', required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--require-qt', action='store_true')
    parser.add_argument('--exact-release-tests', action='store_true')
    parser.add_argument('--jobs', type=int, default=1)
    opts = parser.parse_args()
    if opts.jobs < 1:
        parser.error('--jobs must be positive')
    revision=subprocess.run(['git','rev-parse','HEAD'],cwd=ROOT,text=True,capture_output=True)
    dirty=subprocess.run(['git','status','--porcelain'],cwd=ROOT,text=True,capture_output=True)
    report = {'schema': 1,
              'revision': os.environ.get('RPP_SOURCE_REVISION') or (revision.stdout.strip() if revision.returncode==0 else None),
              'source_sha256': os.environ.get('RPP_SOURCE_SHA256'),
              'dirty': bool(dirty.stdout.strip()) if dirty.returncode==0 else None,
              'timestamp_utc': datetime.now(timezone.utc).isoformat(), 'results': []}
    def test_pair(pair):
        cmake, rustc = pair
        cmake, rustc = Path(cmake).absolute(), Path(rustc).absolute()
        env = dict(os.environ, RPP_TEST_CMAKE=str(cmake), RPP_TEST_RUSTC=str(rustc),
                   RPP_TEST_CTEST=str(cmake.with_name('ctest')))
        entry = {'cmake': str(cmake), 'rustc': str(rustc)}
        start = time.perf_counter()
        try:
            entry['cmake'] = run([cmake, '--version'], env).splitlines()[0]
            entry['rustc'] = run([rustc, '--version'], env).strip()
            entry['rustc_identity'] = run([rustc, '-vV'], env)
            if 'nightly' in entry['rustc'] or 'beta' in entry['rustc']:
                raise ValueError('Stable Rust required')
            if not cmake.with_name('ctest').is_file():
                raise FileNotFoundError('Matching CTest missing')
            with tempfile.TemporaryDirectory(prefix='rpp matrix ') as tmp:
                run([cmake, '-S', ROOT, '-B', tmp, '-G', 'Ninja',
                     f'-DRPP_RUSTC_EXECUTABLE={rustc}',
                     f'-DRPP_ENABLE_QT_VALIDATION={"ON" if opts.require_qt else "OFF"}'], env)
                run([cmake,'--build',tmp],env)
                if opts.exact_release_tests:
                    entry['tests'] = verify_inventory(cmake.with_name('ctest'), tmp, env)
                entry['log'] = run([cmake.with_name('ctest'),'--test-dir',tmp,
                                    '--output-on-failure','--no-tests=error'],env)
            entry['status'] = 'passed'
        except (OSError, ValueError) as error:
            entry.update(status='blocked', log=str(error))
        except (RuntimeError, subprocess.TimeoutExpired) as error:
            entry.update(status='failed', log=str(error))
        entry['elapsed_seconds'] = time.perf_counter()-start
        print(entry['cmake'], entry['rustc'], entry['status'], flush=True)
        return entry

    opts.output.parent.mkdir(parents=True, exist_ok=True)
    with ThreadPoolExecutor(max_workers=opts.jobs) as pool:
        for entry in pool.map(test_pair, itertools.product(opts.cmake, opts.rustc)):
            report['results'].append(entry)
            opts.output.write_text(json.dumps(report, indent=2)+'\n')
    if any(r['status'] != 'passed' for r in report['results']):
        sys.exit(1)


if __name__ == '__main__':
    main()
