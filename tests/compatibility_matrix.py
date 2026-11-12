#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Run the complete local compatibility gate for explicitly installed tools."""
import argparse
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
    opts = parser.parse_args()
    revision=subprocess.run(['git','rev-parse','HEAD'],cwd=ROOT,text=True,capture_output=True)
    dirty=subprocess.run(['git','status','--porcelain'],cwd=ROOT,text=True,capture_output=True)
    report = {'schema': 1,
              'revision': os.environ.get('RPP_SOURCE_REVISION') or (revision.stdout.strip() if revision.returncode==0 else None),
              'dirty': bool(dirty.stdout.strip()) if dirty.returncode==0 else None,
              'timestamp_utc': datetime.now(timezone.utc).isoformat(), 'results': []}
    for cmake, rustc in itertools.product(opts.cmake, opts.rustc):
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
                entry['log'] = run([cmake.with_name('ctest'),'--test-dir',tmp,
                                    '--output-on-failure'],env)
            entry['status'] = 'passed'
        except (OSError, ValueError) as error:
            entry.update(status='blocked', log=str(error))
        except (RuntimeError, subprocess.TimeoutExpired) as error:
            entry.update(status='failed', log=str(error))
        entry['elapsed_seconds'] = time.perf_counter()-start
        report['results'].append(entry)
        print(entry['cmake'],entry['rustc'],entry['status'],flush=True)
        opts.output.parent.mkdir(parents=True,exist_ok=True)
        opts.output.write_text(json.dumps(report,indent=2)+'\n')
    if any(r['status'] != 'passed' for r in report['results']):
        sys.exit(1)


if __name__ == '__main__':
    main()
