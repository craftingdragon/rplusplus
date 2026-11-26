#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Audit ordinary and preprocessed paths with the two release CMake versions."""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import tempfile

from generators import ROOT, snapshot
from preprocess_graph import GENERATORS

PROJECT = '''cmake_minimum_required(VERSION 3.25)
list(PREPEND CMAKE_MODULE_PATH "${RPLUSPLUS_MODULE_DIR}")
project(PathAudit LANGUAGES CXX Rpp)
add_executable(MyApp main.rs)
set_target_properties(MyApp PROPERTIES RPP_PREPROCESS ON RPP_PREPROCESS_DATA data.txt)
'''


def run(*args):
    return subprocess.run(list(map(str, args)), text=True, errors='replace',
                          stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=120)


def audit(folder, cmake, version, rustc, driver, generator, label, preprocess):
    source, build = folder/'source', folder/'build'
    asset = source/'data.txt'
    if preprocess:
        source.mkdir(parents=True)
        (source/'CMakeLists.txt').write_text(PROJECT)
        (source/'main.rs').write_text('fn main(){print!("{}",include_str!("data.txt"));}\n')
        asset.write_text('original')
    else:
        shutil.copytree(ROOT/'examples/native', source)
    result = {'cmake': version, 'generator': generator, 'path': label,
              'mode': 'preprocessed' if preprocess else 'ordinary'}
    configured = run(cmake, '-S', source, '-B', build, '-G', generator,
                     '-DCMAKE_BUILD_TYPE=Debug', f'-DRPLUSPLUS_MODULE_DIR={ROOT}/cmake',
                     f'-DCMAKE_Rpp_COMPILER={driver}', f'-DRPP_RUSTC_EXECUTABLE={rustc}')
    if configured.returncode:
        expected = ((label == 'quote"' and 'Parse error' in configured.stdout)
                    or (label == 'hash#' and version == 'cmake version 3.25.0'
                        and 'containing a "#" is not allowed' in configured.stdout))
        return dict(result, status='expected-unsupported' if expected else 'failed',
                    step='configure', log=configured.stdout)
    cmd = (cmake, '--build', build, '--config', 'Debug', '--parallel', '4')
    exe = build/('Debug/MyApp' if generator == 'Ninja Multi-Config' else 'MyApp')
    tree = build/'.rpp-preprocess/MyApp/cfg-Debug/tree'

    def check(expected=None):
        built = run(*cmd)
        assert not built.returncode, built.stdout
        executed = run(exe)
        assert not executed.returncode and (expected is None or executed.stdout == expected), executed.stdout

    def noop():
        before = snapshot(build)
        times = {p: p.stat().st_mtime_ns for p in tree.rglob('*') if p.is_file()}
        built = run(*cmd)
        assert not built.returncode, built.stdout
        assert before == snapshot(build)
        assert times == {p: p.stat().st_mtime_ns for p in tree.rglob('*') if p.is_file()}

    try:
        check('original' if preprocess else None)
        noop()
        if preprocess:
            asset.write_text('changed')
            check('changed')
            (tree/'data.txt').unlink()
            check('changed')
            assert (tree/'data.txt').read_text() == 'changed'
            noop()
    except AssertionError as error:
        return dict(result, status='failed', step='build/recovery', log=str(error))
    return dict(result, status='passed')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cmake', action='append', required=True)
    parser.add_argument('--rustc', required=True)
    parser.add_argument('--driver', required=True)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    checks = []
    with tempfile.TemporaryDirectory(prefix='rpp path audit ') as tmp:
        for cmake in args.cmake:
            version = subprocess.check_output([cmake, '--version'], text=True).splitlines()[0]
            for generator in GENERATORS:
                for preprocess in (False, True):
                    for label in ('spaces 🦀', "apostrophe'", 'dollar$', 'hash#', 'quote"'):
                        folder = Path(tmp)/str(len(checks))/label
                        checks.append(audit(folder, cmake, version, args.rustc, args.driver,
                                            generator, label, preprocess))
    status = 'failed' if any(c['status'] == 'failed' for c in checks) else 'passed'
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps({'schema': 1, 'status': status, 'results': checks}, indent=2)+'\n')
    return int(status != 'passed')


if __name__ == '__main__':
    raise SystemExit(main())
