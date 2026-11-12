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
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    if result.returncode:
        raise RuntimeError(f'{command}\n{result.stdout}')
    return result.stdout


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cmake', action='append', required=True)
    parser.add_argument('--rustc', action='append', required=True)
    parser.add_argument('--output', type=Path, required=True)
    opts = parser.parse_args()
    report = {'timestamp_utc': datetime.now(timezone.utc).isoformat(), 'results': []}
    for cmake, rustc in itertools.product(opts.cmake, opts.rustc):
        cmake, rustc = Path(cmake).absolute(), Path(rustc).absolute()
        env = dict(os.environ, RPP_TEST_CMAKE=str(cmake), RPP_TEST_RUSTC=str(rustc),
                   RPP_TEST_CTEST=str(cmake.with_name('ctest')))
        entry = {'cmake': run([cmake, '--version'],env).splitlines()[0],
                 'rustc': run([rustc, '--version'],env).strip()}
        if 'nightly' in entry['rustc'] or 'beta' in entry['rustc']:
            raise RuntimeError('This matrix requires stable Rust compilers')
        start = time.perf_counter()
        try:
            with tempfile.TemporaryDirectory(prefix='rpp matrix ') as tmp:
                run([cmake, '-S', ROOT, '-B', tmp, '-G', 'Ninja',
                     f'-DRPP_RUSTC_EXECUTABLE={rustc}'], env)
                run([cmake,'--build',tmp],env)
                entry['log'] = run([cmake.with_name('ctest'),'--test-dir',tmp,
                                    '--output-on-failure'],env)
            entry['status'] = 'passed'
        except RuntimeError as error:
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
