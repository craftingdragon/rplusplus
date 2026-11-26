#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Exercise production preprocessing through relocated native Rpp modules."""
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time

from generators import CMAKE, RUSTC
from preprocess_feasibility import ROOT, run, write
from preprocess_graph import GENERATORS

PROJECT = '''cmake_minimum_required(VERSION 3.25)
find_package(Rplusplus CONFIG REQUIRED PATHS "${PREFIX}/share/rplusplus/cmake" NO_DEFAULT_PATH)
project(ProductionGraph LANGUAGES CXX Rpp)
add_library(local STATIC local.rs)
set_target_properties(local PROPERTIES RPP_CRATE_TYPE rlib)
add_subdirectory(app)
add_executable(Plain plain.rs)
'''
APP = '''add_library(backend STATIC lib.rs)
set_target_properties(backend PROPERTIES RPP_PREPROCESS ON RPP_CRATE_EDITION 2021)
add_executable(Cxx main.cpp)
target_link_libraries(Cxx PRIVATE backend)
install(TARGETS backend ARCHIVE DESTINATION lib)
add_executable(App main.rs outer.rs outer/nested.rs alternate.rs code.rs worker.cpp)
set_target_properties(App PROPERTIES
  RPP_PREPROCESS ON RPP_CRATE_ROOT main.rs RPP_CRATE_EDITION 2024
  RPP_PREPROCESS_INPUTS "selector.rph;headers/first.rph;headers/other.rph;headers/second.rph"
  RPP_PREPROCESS_DATA "data é.txt"
  RPP_PREPROCESS_INCLUDE_DIRECTORIES "$<$<CONFIG:Debug>:headers>;$<$<NOT:$<CONFIG:Debug>>:headers>"
  RPP_PREPROCESS_DEFINITIONS "APP_BONUS=0;$<$<CONFIG:Debug>:DEBUG_BONUS=100>")
target_link_libraries(App PRIVATE local)
'''
MAIN = '''#include "selector.rph"
#if 0
unused_a
#endif
#ifdef DEBUG_BONUS
const BONUS:i32 = DEBUG_BONUS;
#else
const BONUS:i32 = 0;
#endif
mod outer;
#[path="alternate.rs"] mod alternate;
include!("code.rs");
unsafe extern "C" { fn worker()->i32; }
fn main() {
    let data = include_str!("data é.txt");
    assert_eq!(data.as_bytes(), include_bytes!("data é.txt"));
    let total = data.parse::<i32>().unwrap() + INCLUDED + outer::nested::N +
        alternate::N + unsafe {worker()} + local::value() + SELECTED + BONUS + APP_BONUS;
    println!("{}", total);
}
'''
CXX = '''#include <iostream>
extern "C" int get_value();
int main() { std::cout << get_value() << "\\n"; }
'''
PAYLOAD = ('main.rs', 'outer.rs', 'outer/nested.rs', 'alternate.rs', 'code.rs',
           'data é.txt', '.rpp/manifest', '.rpp/source-map.json', '.rpp/owned', '.rpp/complete')


def shim(path, tool, trace, kind, barrier):
    path.write_text('#!' + sys.executable + '\n' + f'''
import json, os, subprocess, sys, time
from pathlib import Path
args = sys.argv[1:]
kind = {kind!r}
if kind == 'rustc' and args == ['--print', 'sysroot']:
    print(Path(sys.argv[0]).parent.parent)
    sys.exit(0)
if kind == 'rpp':
    kind = args[0]
with open({str(trace)!r}, 'a') as file:
    file.write(json.dumps([kind, args]) + '\\n')
if kind == 'preprocess' and Path({str(barrier)!r}).exists() and '/App/' in ' '.join(args):
    subprocess.run([{str(tool)!r}, *args], check=True)
    Path({str(barrier) + '.ready'!r}).touch()
    time.sleep(60)
os.execv({str(tool)!r}, [{str(tool)!r}, *args])
''')
    path.chmod(0o755)


def fixture(source):
    write(source, 'CMakeLists.txt', PROJECT)
    write(source, 'app/CMakeLists.txt', APP)
    write(source, 'local.rs', 'pub fn value() -> i32 { 10 }\n')
    write(source, 'plain.rs', 'fn main() {}\n')
    write(source, 'app/lib.rs', '#define BACKEND_VALUE 54\n#[no_mangle] pub extern "C" fn get_value()->i32 {BACKEND_VALUE}\n')
    write(source, 'app/main.cpp', CXX)
    write(source, 'app/main.rs', MAIN)
    write(source, 'app/worker.cpp', 'extern "C" int worker(){return 1;}\n')
    write(source, 'app/outer.rs', 'pub mod nested;\n')
    write(source, 'app/outer/nested.rs', 'pub const N:i32=2;\n')
    write(source, 'app/alternate.rs', 'pub const N:i32=3;\n')
    write(source, 'app/code.rs', 'const INCLUDED:i32=4;\n')
    write(source, 'app/data é.txt', '5')
    write(source, 'app/selector.rph', '#include "first.rph"\n')
    write(source, 'app/headers/first.rph', '#define SELECTED 20\n')
    write(source, 'app/headers/other.rph', '#define SELECTED 20\n')
    write(source, 'app/headers/second.rph', '#define SELECTED 30\n')


def timestamps(path):
    return {p: p.stat().st_mtime_ns for p in path.rglob('*') if p.is_file()}


def probe(base, prefix, generator):
    base.mkdir()
    source, build = base/'consumer é source', base/'build é space'
    fixture(source)
    trace, barrier = base/'trace', base/'pause'
    driver, rustc, cxx = base/'rpp', base/'rust sysroot/bin/rustc', base/'cxx'
    rustc.parent.mkdir(parents=True)
    rustc_lib = rustc.parent.parent/'lib'
    rustc_lib.mkdir()
    sysroot = Path(run([RUSTC, '--print', 'sysroot']).stdout.strip())
    (rustc_lib/'rustlib').symlink_to(sysroot/'lib/rustlib', target_is_directory=True)
    shim(driver, prefix/'bin/rpp', trace, 'rpp', barrier)
    shim(rustc, Path(shutil.which(RUSTC)), trace, 'rustc', barrier)
    shim(cxx, Path(shutil.which('c++')), trace, 'cxx', barrier)
    multi = generator == 'Ninja Multi-Config'
    configure = [CMAKE, '-S', source, '-B', build]
    run([*configure, '-G', generator, f'-DPREFIX={prefix}', f'-DCMAKE_Rpp_COMPILER={driver}',
         f'-DRPP_RUSTC_EXECUTABLE={rustc}', f'-DCMAKE_CXX_COMPILER={cxx}',
         *(['-DCMAKE_CONFIGURATION_TYPES=Debug;Release', '-DCMAKE_CROSS_CONFIGS=all'] if multi
           else ['-DCMAKE_BUILD_TYPE=Debug'])])
    if multi:
        run([CMAKE, '--build', build, '--config', 'Debug', '--target', 'App:all', '--parallel', '4'])
    for config in ('Debug', 'Release'):
        if not multi:
            run([*configure, '-DCMAKE_BUILD_TYPE='+config])
        cmd = [CMAKE, '--build', build, '--config', config, '--parallel', '4']
        state = build/'app'/'.rpp-preprocess'
        tree = state/'App'/('cfg-'+config)/'tree'
        out = build/'app'/config if multi else build/'app'
        exe = out/'App'
        baseline = 45 + (100 if config == 'Debug' else 0)

        def read_trace():
            return [json.loads(line) for line in trace.read_text().splitlines()]

        def build_now():
            trace.write_text('')
            run(cmd)
            return read_trace()

        def check(expected):
            assert run([exe]).stdout.strip() == str(expected), (generator, config, expected)
            assert run([out/'Cxx']).stdout.strip() == '54'

        def noop():
            before = timestamps(state)
            assert not build_now(), (generator, config, read_trace())
            after = timestamps(state)
            assert after == before, (generator, config, {str(p): (before.get(p), after.get(p))
                for p in before.keys() | after.keys() if before.get(p) != after.get(p)})

        build_now()
        check(baseline)
        noop()
        for name in PAYLOAD:
            before = {p: stamp for p, stamp in timestamps(state).items() if tree not in p.parents}
            (tree/name).unlink()
            assert any(kind == 'preprocess' for kind, _ in build_now())
            assert (tree/name).is_file()
            assert before == {p: stamp for p, stamp in timestamps(state).items() if tree not in p.parents}
            check(baseline)
            noop()

        original = source/'app/main.rs'
        original.touch()
        edits = build_now()
        assert any(kind == 'preprocess' for kind, _ in edits)
        if generator != 'Unix Makefiles':
            assert [kind for kind, _ in edits] == ['preprocess'], edits
        noop()
        content = original.read_text()
        original.write_text(content.replace('unused_a', 'unused_b' if 'unused_a' in content else 'unused_a'))
        edits = build_now()
        assert any(kind == 'preprocess' for kind, _ in edits)
        if generator != 'Unix Makefiles':
            assert [kind for kind, _ in edits] == ['preprocess'], edits
        noop()

        cmake_file = source/'app/CMakeLists.txt'
        cmake_file.write_text(APP.replace('APP_BONUS=0', 'APP_BONUS=1'))
        run(configure)
        edits = build_now()
        assert any(kind == 'compile' and str(tree/'main.rs') in args for kind, args in edits)
        assert any(kind == 'preprocess' for kind, _ in edits)
        check(baseline+1)
        noop()
        cmake_file.write_text(APP)
        run(configure)
        build_now()
        check(baseline)
        noop()

        generated_root = (tree/'main.rs').read_bytes()
        source_map = (tree/'.rpp/source-map.json').read_bytes()
        write(source, 'app/selector.rph', '#include "other.rph"\n')
        edits = build_now()
        assert (tree/'main.rs').read_bytes() == generated_root
        assert (tree/'.rpp/source-map.json').read_bytes() != source_map
        assert any(kind == 'compile' and str(tree/'main.rs') in args for kind, args in edits)
        assert all('/backend/' not in ' '.join(args) and 'Plain' not in ' '.join(args)
                   for _, args in edits)
        check(baseline)
        noop()

        local = source/'local.rs'
        local.write_text('pub fn value() -> i32 { 11 }\n')
        edits = build_now()
        assert any(kind == 'compile' and str(tree/'main.rs') in args for kind, args in edits)
        assert not any(kind == 'preprocess' for kind, _ in edits)
        check(baseline+1)

        write(source, 'app/selector.rph', '#include "second.rph"\n')
        build_now()
        check(baseline+11)
        compiler_deps = list(build.rglob('*.rs.rlib.d'))
        prep_dep = tree/'.rpp/preprocess.d'
        if generator == 'Unix Makefiles':
            assert compiler_deps and prep_dep.exists()
            assert 'second.rph' in prep_dep.read_text()
        else:
            assert not compiler_deps, compiler_deps
        for path in (*compiler_deps, prep_dep):
            path.unlink(missing_ok=True)
        write(source, 'app/headers/second.rph', '#define SELECTED 40\n')
        write(source, 'app/data é.txt', '6')
        events = build_now()
        rust_inputs = [args for kind, args in events if kind == 'rustc' and str(tree/'main.rs') in args]
        assert len(rust_inputs) >= 2, events
        check(baseline+22)
        noop()
        # Final linking independently verifies the recorded snapshot.
        obj = next(p for p in build.rglob('main.rs.rlib') if ('cfg-'+config) in p.parts)
        marker = tree/'.rpp/complete'
        saved = marker.read_bytes()
        marker.unlink()
        rejected = run([driver, 'link', '--rustc', rustc, '-o', base/'unsafe-link', obj], ok=False)
        assert rejected.returncode and 'missing tree completion marker' in rejected.stderr
        assert not (base/'unsafe-link').exists()
        marker.write_bytes(saved)

        # The journal must recover even when neither in-tree ownership record survived.
        for name in ('.rpp/complete', '.rpp/owned', '.rpp/manifest'):
            (tree/name).unlink()
        build_now()
        check(baseline+22)
        noop()

        intact = exe.read_bytes()
        write(source, 'app/headers/second.rph', '#error interrupted test\n')
        trace.write_text('')
        failure = run(cmd, ok=False)
        assert failure.returncode and 'interrupted test' in failure.stderr+failure.stdout
        assert exe.read_bytes() == intact
        assert [kind for kind, _ in read_trace()] == ['preprocess']
        write(source, 'app/headers/second.rph', '#define SELECTED 41\n')
        barrier.touch()
        trace.write_text('')
        process = subprocess.Popen(list(map(str, cmd)), stdout=subprocess.DEVNULL,
                                   stderr=subprocess.DEVNULL, start_new_session=True)
        ready = Path(str(barrier)+'.ready')
        try:
            deadline = time.monotonic()+15
            while not ready.exists():
                assert process.poll() is None and time.monotonic() < deadline
                time.sleep(.02)
            # Leave a deterministically incomplete publication regardless of the
            # generator's own cleanup policy.
            (tree/'.rpp/complete').unlink()
            os.killpg(process.pid, signal.SIGINT)
            assert process.wait(timeout=10)
            assert exe.read_bytes() == intact
            assert [kind for kind, _ in read_trace()] == ['preprocess']
        finally:
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait(timeout=10)
            barrier.unlink()
            ready.unlink(missing_ok=True)
        build_now()
        check(baseline+23)
        noop()
        run([CMAKE, '--build', build, '--config', config, '--target', 'clean'])
        build_now()
        check(baseline+23)
        noop()
        write(source, 'app/selector.rph', '#include "first.rph"\n')
        write(source, 'app/headers/second.rph', '#define SELECTED 30\n')
        write(source, 'app/data é.txt', '5')
        local.write_text('pub fn value() -> i32 { 10 }\n')
        build_now()
        check(baseline)
        noop()

    for path in build.rglob('*'):
        if path.suffix in ('.cmake', '.make', '.ninja') and path.is_file():
            assert str(ROOT) not in path.read_text(errors='replace'), path
    installed = base/'backend install'
    run([CMAKE, '--install', build, '--prefix', installed, '--config', 'Release'])
    shutil.rmtree(source)
    shutil.rmtree(build)
    external = write(base, 'unrelated/main.cpp', CXX)
    native = base/'unrelated/consumer'
    run([shutil.which('c++'), external, installed/'lib/libbackend.a', '-o', native])
    assert run([native]).stdout.strip() == '54'


def main():
    with tempfile.TemporaryDirectory(prefix='rpp m6 ') as temp:
        base = Path(temp)
        sentinel = write(base, 'no-cargo/cargo', '#!/bin/sh\nexit 99\n')
        sentinel.chmod(0o755)
        os.environ['PATH'] = str(sentinel.parent)+os.pathsep+os.environ['PATH']
        producer, package, prefix = base/'producer', base/'package', base/'installed'
        for name in ('cmake', 'src', 'docs', 'LICENSES'):
            shutil.copytree(ROOT/name, producer/name)
        for name in ('CMakeLists.txt', 'README.md', 'LICENSE'):
            shutil.copy2(ROOT/name, producer/name)
        run([CMAKE, '-S', producer, '-B', package, '-G', 'Ninja', '-DBUILD_TESTING=OFF',
             f'-DRPP_RUSTC_EXECUTABLE={RUSTC}'])
        run([CMAKE, '--build', package])
        run([CMAKE, '--install', package, '--prefix', prefix])
        moved = base/'relocated tools'
        prefix.rename(moved)
        shutil.rmtree(producer)
        shutil.rmtree(package)
        for generator in GENERATORS:
            probe(base/generator, moved, generator)
            print(generator, 'passed Debug and Release', flush=True)


if __name__ == '__main__':
    main()
