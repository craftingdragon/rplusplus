#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Candidate configuration switching and archive/runtime compatibility."""
import argparse
import json
from pathlib import Path
import shutil
import tempfile

from generators import ROOT
from preprocess_feasibility import run, write
from preprocess_graph import GENERATORS


def compile_example(work, cmake, driver, rustc, generator, modes, archiver=None):
    for config in modes:
        extra = [] if archiver is None else [f'-DCMAKE_AR={archiver}']
        run([cmake, '-S', ROOT/'examples/multicrate', '-B', work, '-G', generator,
             f'-DCMAKE_BUILD_TYPE={config}', f'-DRPLUSPLUS_MODULE_DIR={ROOT}/cmake',
             f'-DCMAKE_Rpp_COMPILER={driver}', f'-DRPP_RUSTC_EXECUTABLE={rustc}', *extra])
        run([cmake, '--build', work, '--config', config, '--parallel', '4'])
        run([cmake.with_name('ctest'), '--test-dir', work, '-C', config,
             '--output-on-failure', '--no-tests=error'])


def runtime_metadata(work, driver, rustc, ar):
    source = write(work, 'value.rs', '#[unsafe(no_mangle)] pub extern "C" fn answer()->i32{42}')
    main = write(work, 'main.cpp', 'extern "C" int answer(); int main(){return answer()!=42;}')
    obj, rlib, archive, exe = (work/name for name in ('main.o', 'value.rlib', 'libvalue.a', 'app'))
    run([driver, 'compile', '--rustc', rustc, '--kind', 'staticlib', '--name', 'audit',
         '--depfile', work/'value.d', '-o', rlib, source])
    run([driver, 'link', '--rustc', rustc, '--ar', ar, '-o', archive, rlib, '--libraries'])
    run(['c++', '-c', main, '-o', obj])

    def link(library, ok=True):
        return run([driver, 'link', '--rustc', rustc, '-o', exe, obj, '--libraries', library], ok=ok)

    link(archive)
    run([exe])
    payload = run([ar, 'p', archive, 'rpp-runtime']).stdout.encode()
    assert payload.startswith(b'rpp-runtime-v1\0')
    legacy = work/'legacy.a'
    shutil.copyfile(archive, legacy)
    run([ar, 'd', legacy, 'rpp-runtime'])
    Path(str(legacy)+'.runtime').write_bytes(payload.partition(b'\0')[2]+b'\0')
    link(legacy)
    run([exe])
    old = exe.read_bytes()
    incompatible = work/'bad.a'
    incompatible.write_bytes(archive.read_bytes().replace(b'rpp-runtime-v1\0', b'rpp-runtime-v2\0', 1))
    rejected = link(incompatible, ok=False)
    assert rejected.returncode and 'invalid Rpp runtime metadata' in rejected.stderr
    assert exe.read_bytes() == old


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cmake', action='append', required=True, type=Path)
    parser.add_argument('--rustc', type=Path, required=True)
    parser.add_argument('--driver', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    report = {'schema': 1, 'status': 'blocked', 'checks': []}
    try:
        ar, llvm_ar = (shutil.which(name) for name in ('ar', 'llvm-ar'))
        if not ar or not llvm_ar:
            raise FileNotFoundError('GNU ar and llvm-ar are required')
        with tempfile.TemporaryDirectory(prefix='rpp release audit ') as tmp:
            work = Path(tmp)
            for index, cmake in enumerate(args.cmake):
                for generator in GENERATORS:
                    compile_example(work/f'{index}-{generator}', cmake, args.driver, args.rustc,
                                    generator, ('Debug', 'Release', 'Debug'))
                    report['checks'].append({'configuration_switching': generator, 'cmake': str(cmake)})
            for archiver in (ar, llvm_ar):
                compile_example(work/Path(archiver).name, args.cmake[-1], args.driver,
                                args.rustc, 'Ninja', ('Debug',), archiver)
                report['checks'].append({'archiver': run([archiver, '--version']).stdout.splitlines()[0]})
            runtime_metadata(work/'runtime', args.driver, args.rustc, ar)
            report['checks'].extend(['legacy runtime sidecar', 'incompatible runtime rejected, output preserved'])
        report['status'] = 'passed'
    except (OSError, AssertionError, RuntimeError) as error:
        report['status'] = 'blocked' if isinstance(error, FileNotFoundError) else 'failed'
        report['error'] = str(error)
        print(error)
    args.output.write_text(json.dumps(report, indent=2)+'\n')
    return int(report['status'] != 'passed')


if __name__ == '__main__':
    raise SystemExit(main())
