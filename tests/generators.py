#!/usr/bin/env python3
"""Installed Rpp language gate. Uses real rustc; never calls Cargo."""
# SPDX-License-Identifier: GPL-3.0-or-later
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def run(*args):
    result = subprocess.run([str(a) for a in args], text=True,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    if result.returncode:
        raise RuntimeError(f"Command failed: {args}\n{result.stdout}")
    return result.stdout


def snapshot(build):
    return {p.relative_to(build): p.stat().st_mtime_ns
            for p in build.rglob('*.o')}


def main():
    for tool in ('cmake', 'rustc', 'c++', 'ninja', 'make'):
        if not shutil.which(tool):
            raise RuntimeError(f"Required gate tool missing: {tool}")
    with tempfile.TemporaryDirectory(prefix='rpp gate ') as tmp:
        base = Path(tmp)
        package = base / 'package'
        prefix = base / 'installed modules'
        run('cmake', '-S', ROOT, '-B', package, '-G', 'Ninja',
            '-DBUILD_TESTING=OFF', '-DCMAKE_INSTALL_LIBDIR=lib')
        run('cmake', '--install', package, '--prefix', prefix)
        modules = prefix / 'share/rplusplus/cmake'
        for gen in ('Ninja', 'Unix Makefiles', 'Ninja Multi-Config'):
            source = base / gen / 'source tree'
            shutil.copytree(ROOT / 'examples/native', source)
            build = base / gen / 'build tree'
            run('cmake', '-S', source, '-B', build, '-G', gen,
                f'-DRPLUSPLUS_MODULE_DIR={modules}')
            configs = ('Debug', 'Release') if gen == 'Ninja Multi-Config' else ('Debug',)
            for config in configs:
                run('cmake', '--build', build, '--config', config)
                run('ctest', '--test-dir', build, '-C', config, '--output-on-failure')
            before = snapshot(build)
            assert before, 'No compiler objects found'
            run('cmake', '--build', build, '--config', 'Debug')
            assert snapshot(build) == before, f'{gen}: no-op rebuilt objects'
            # Change a dependency not explicitly listed in CMakeLists.txt.
            sub = source / 'sub.rs'
            sub.write_text(sub.read_text() + '\n// dependency change\n')
            run('cmake', '--build', build, '--config', 'Debug')
            after = snapshot(build)
            changed = [p for p in before if before[p] != after[p]]
            assert len(changed) == 1 and changed[0].name == 'value.rs.o', changed
            run('ctest', '--test-dir', build, '-C', 'Debug', '--output-on-failure')
            # Reconfiguration must preserve valid generator output.
            cmakelists = source / 'CMakeLists.txt'
            cmakelists.write_text(cmakelists.read_text() + '\n# regenerate\n')
            run('cmake', '--build', build, '--config', 'Debug')
            print(f'PASS {gen}: installed modules, compiler check, mixed link, execution, '
                  'no-op, selective module rebuild, regeneration', flush=True)


if __name__ == '__main__':
    main()
