#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Offline Qt/SQLite and isolated installed-consumer regression gate."""
import os
from pathlib import Path
import shutil
import tempfile

from generators import ROOT, RUSTC, CTEST, run, snapshot

GENERATORS = ('Ninja', 'Unix Makefiles', 'Ninja Multi-Config')
MODES = ('plain', 'count', 'preprocessed-fallback')
CONFIGS = ('Debug', 'Release')


def install_tools(base):
    source, build, prefix = (base/name for name in ('tool source', 'tool build', 'prefix'))
    source.mkdir()
    for name in ('CMakeLists.txt', 'README.md', 'LICENSE'):
        shutil.copy2(ROOT/name, source/name)
    for name in ('cmake', 'src', 'docs', 'LICENSES'):
        shutil.copytree(ROOT/name, source/name)
    run('cmake', '-S', source, '-B', build, '-G', 'Ninja', '-DBUILD_TESTING=OFF',
        f'-DRPP_RUSTC_EXECUTABLE={RUSTC}')
    run('cmake', '--build', build)
    run('cmake', '--install', build, '--prefix', prefix)
    moved = base/'relocated tools'
    prefix.rename(moved)
    shutil.rmtree(build)
    shutil.rmtree(source)
    return moved, (source, build, prefix)


def check(build, config, expected_count):
    run('cmake', '-E', 'env', f'RPP_NOTES_EXPECT_COUNT={expected_count}', CTEST,
        '--test-dir', build, '-C', config, '--output-on-failure')


def build_state(build):
    state = snapshot(build)
    for path in (build/'.rpp-preprocess').rglob('*'):
        if path.is_file():
            state[path.relative_to(build)] = path.stat().st_mtime_ns
    for name in ('RppNotes', 'libnotes_backend.a'):
        for path in build.rglob(name):
            if path.is_file():
                state[path.relative_to(build)] = path.stat().st_mtime_ns
    return state


def validate(base, prefix, absent_tools, generator, mode):
    source, build, consumer, installed = (base/name for name in
                                           ('source', 'build', 'consumer', 'installed'))
    shutil.copytree(ROOT/'examples/qt-notes', source)
    shutil.copytree(source/'consumer', consumer)
    preprocess = mode != 'plain'
    expected_count = int(mode == 'count')
    if mode == 'preprocessed-fallback':
        with (source/'CMakeLists.txt').open('a') as fixture:
            fixture.write('\nset_property(TARGET notes_backend PROPERTY RPP_PREPROCESS_DEFINITIONS "RPP_NOTES_COUNT=0")\n')

    run('cmake', '-S', source, '-B', build, '-G', generator, '-DCMAKE_BUILD_TYPE=Debug',
        f'-DRPLUSPLUS_MODULE_DIR={prefix}/share/rplusplus/cmake',
        f'-DRPP_RUSTC_EXECUTABLE={RUSTC}', f'-DRPP_NOTES_PREPROCESS={"ON" if preprocess else "OFF"}')
    for config in CONFIGS:
        if generator != 'Ninja Multi-Config':
            run('cmake', '-S', source, '-B', build, f'-DCMAKE_BUILD_TYPE={config}')
        command = ('cmake', '--build', build, '--config', config, '--parallel', '4')
        run(*command)
        tree = build/'.rpp-preprocess'/'notes_backend'/f'cfg-{config}'/'tree'
        if preprocess:
            assert (tree/'.rpp/complete').is_file(), (generator, mode, config)
            assert (tree/'count_preprocessed.rs').is_file()
        else:
            assert not (build/'.rpp-preprocess').exists(), (generator, mode, config)
        check(build, config, expected_count)
        before = build_state(build)
        assert before and any(p.name == 'RppNotes' for p in before)
        run(*command)
        assert build_state(build) == before, (generator, mode, config, 'no-op rewrote outputs')

    run('cmake', '--install', build, '--config', 'Release', '--prefix', installed)
    moved = base/'moved package'
    installed.rename(moved)
    shutil.rmtree(build)
    shutil.rmtree(source)
    assert not list(moved.rglob('.rpp-preprocess'))
    for export in moved.rglob('*.cmake'):
        contents = export.read_text()
        for absent in (*absent_tools, source, build, installed):
            assert not absent.exists() and str(absent) not in contents, (export, absent)

    external = base/'external'
    run('cmake', '-S', consumer, '-B', external, '-G', generator, '-DCMAKE_BUILD_TYPE=Release',
        f'-DCMAKE_PREFIX_PATH={prefix};{moved}', f'-DRPP_RUSTC_EXECUTABLE={RUSTC}')
    run('cmake', '--build', external, '--config', 'Release', '--parallel', '4')
    check(external, 'Release', expected_count)
    print(f'PASS {generator} / {mode}: Qt Debug and Release, relocated installed consumer', flush=True)


def main():
    with tempfile.TemporaryDirectory(prefix='rpp qt validation ') as tmp:
        base = Path(tmp)
        sentinel = base/'forbidden'
        sentinel.mkdir()
        (sentinel/'cargo').write_text('#!/usr/bin/env python3\nraise SystemExit("Cargo forbidden")\n')
        (sentinel/'cargo').chmod(0o755)
        os.environ['PATH'] = str(sentinel)+os.pathsep+os.environ['PATH']
        prefix, absent_tools = install_tools(base)
        for generator in GENERATORS:
            for mode in MODES:
                validate(base/generator/mode, prefix, absent_tools, generator, mode)


if __name__ == '__main__':
    main()
