#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Measure warm-cache build latency and subprocess counts, never invoking Cargo."""
import argparse
from collections import Counter
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import platform
import shutil
import statistics
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]
GENERATORS = ('Ninja', 'Unix Makefiles', 'Ninja Multi-Config')


def run(args, env=None):
    result = subprocess.run(list(map(str, args)), env=env, text=True,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    if result.returncode:
        raise RuntimeError(f'{args}\n{result.stdout}')
    return result.stdout


def wrapper(path, real, tool, trace):
    path.write_text(f'''#!{sys.executable}
import json, os, tempfile, sys
with tempfile.NamedTemporaryFile(mode='w', dir={str(trace)!r}, delete=False) as log:
    json.dump({{'tool': {tool!r}, 'args': sys.argv[1:]}}, log)
os.execv({str(real)!r}, [{str(real)!r}] + sys.argv[1:])
''')
    path.chmod(0o755)
    return path


def category(event):
    tool, args = event['tool'], event['args']
    if tool == 'rpp':
        return 'rpp.' + args[0]
    if tool == 'cxx':
        return 'cxx.compile' if '-c' in args else 'cxx.link'
    return 'rustc.' + next((a.split('=', 1)[1] for a in args
                            if a.startswith('--crate-type=')), 'query')


def measure(command, trace, env):
    for path in trace.iterdir():
        path.unlink()
    start = time.perf_counter()
    run(command, env)
    elapsed = time.perf_counter() - start
    counts = Counter(category(json.loads(p.read_text())) for p in trace.iterdir())
    return {'seconds': elapsed, 'invocations': dict(sorted(counts.items()))}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cmake', default=shutil.which('cmake'))
    parser.add_argument('--rustc', required=True, help='Absolute stable compiler path')
    parser.add_argument('--timing-only', action='store_true', help='Use real tools directly; invocation counts are not observed')
    parser.add_argument('--repeats', type=int, default=5)
    parser.add_argument('--output', type=Path, required=True)
    opts = parser.parse_args()
    if opts.repeats < 1:
        parser.error('--repeats must be positive')
    cmake, rustc = Path(opts.cmake).resolve(), Path(opts.rustc).absolute()
    ctest = cmake.with_name('ctest')
    result = {'schema': 1, 'timestamp_utc': datetime.now(timezone.utc).isoformat(),
              'platform': platform.platform(), 'machine': platform.machine(),
              'logical_cpus': os.cpu_count(), 'jobs': 4, 'config': 'Debug',
              'cmake': run([cmake, '--version']).splitlines()[0],
              'rustc': run([rustc, '--version']).strip(),
              'cxx': run(['c++', '--version']).splitlines()[0],
              'ninja': run(['ninja', '--version']).strip(),
              'make': run(['make', '--version']).splitlines()[0],
              'fixture': 'examples/multicrate', 'instrumented': not opts.timing_only, 'samples': [],
              'method': 'Warm filesystem/toolchain caches; configure/bootstrap/clean/test times excluded. Python process wrappers are used only when instrumented.'}
    with tempfile.TemporaryDirectory(prefix='rpp baseline ') as tmp:
        base = Path(tmp)
        env = os.environ.copy()
        sentinel = base / 'no-cargo'
        sentinel.mkdir()
        (sentinel / 'cargo').write_text(f'#!{sys.executable}\nraise SystemExit("Cargo is forbidden")\n')
        (sentinel / 'cargo').chmod(0o755)
        env['PATH'] = str(sentinel) + os.pathsep + env['PATH']
        package, prefix = base / 'bootstrap', base / 'installed'
        run([cmake, '-S', ROOT, '-B', package, '-G', 'Ninja',
             '-DBUILD_TESTING=OFF', f'-DRPP_RUSTC_EXECUTABLE={rustc}'], env)
        run([cmake, '--build', package], env)
        run([cmake, '--install', package, '--prefix', prefix], env)
        trace = base / 'trace'
        trace.mkdir()
        tools = base / 'tools'
        tools.mkdir()
        cc = wrapper(tools/'cxx', Path(shutil.which('c++')).absolute(), 'cxx', trace)
        rc = wrapper(tools/'rustc', rustc, 'rustc', trace)
        driver = wrapper(tools/'rpp', prefix/'bin/rpp', 'rpp', trace)
        if opts.timing_only:
            cc, rc, driver = Path(shutil.which('c++')).absolute(), rustc, prefix/'bin/rpp'
        for gen in GENERATORS:
            src, build = base/gen/'source', base/gen/'build'
            shutil.copytree(ROOT/'examples/multicrate', src)
            originals = {name: (src/name).read_text() for name in ('base.rs','app.rs','main.cpp')}
            run([cmake, '-S', src, '-B', build, '-G', gen, '-DCMAKE_BUILD_TYPE=Debug',
                 f'-DRPLUSPLUS_MODULE_DIR={prefix}/share/rplusplus/cmake',
                 f'-DCMAKE_Rpp_COMPILER={driver}', f'-DRPP_RUSTC_EXECUTABLE={rc}',
                 f'-DCMAKE_CXX_COMPILER={cc}'], env)
            command = [cmake, '--build', build, '--config', 'Debug', '--parallel', '4']
            for repeat in range(opts.repeats):
                for name, content in originals.items():
                    (src/name).write_text(content)
                run(command + ['--target', 'clean'], env)
                for scenario in ('clean', 'noop', 'rust_leaf', 'rust_root', 'cxx_leaf', 'post_noop'):
                    if scenario == 'rust_leaf':
                        (src/'base.rs').write_text(originals['base.rs'] + '\npub fn baseline_marker() {}\n')
                    elif scenario == 'rust_root':
                        (src/'app.rs').write_text(originals['app.rs'] + '\n// root edit\n')
                    elif scenario == 'cxx_leaf':
                        (src/'main.cpp').write_text(originals['main.cpp'] + '\n// C++ edit\n')
                    sample = measure(command, trace, env)
                    if opts.timing_only:
                        sample['invocations'] = None
                    sample.update(generator=gen, repeat=repeat, scenario=scenario)
                    if scenario in ('noop', 'post_noop') and sample['invocations']:
                        raise AssertionError(f'No-op invoked compilers or driver: {sample}')
                    result['samples'].append(sample)
                run([ctest, '--test-dir', build, '-C', 'Debug', '--output-on-failure'], env)
            print(f'Measured {gen}: {opts.repeats} repetitions', flush=True)
    result['summary'] = []
    for gen in GENERATORS:
        for scenario in ('clean','noop','rust_leaf','rust_root','cxx_leaf','post_noop'):
            samples = [s for s in result['samples'] if s['generator']==gen and s['scenario']==scenario]
            counts = [s['invocations'] for s in samples]
            result['summary'].append({'generator': gen, 'scenario': scenario,
                'median_seconds': statistics.median(s['seconds'] for s in samples),
                'min_seconds': min(s['seconds'] for s in samples),
                'max_seconds': max(s['seconds'] for s in samples),
                'invocations': counts[0] if all(c==counts[0] for c in counts) else counts})
    opts.output.parent.mkdir(parents=True, exist_ok=True)
    opts.output.write_text(json.dumps(result, indent=2)+'\n')
    print(f'Wrote {opts.output}', flush=True)


if __name__ == '__main__':
    main()
