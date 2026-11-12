#!/usr/bin/env python3
"""Installed Rpp language gate. Uses real rustc; never calls Cargo."""
# SPDX-License-Identifier: GPL-3.0-or-later
from pathlib import Path
import shutil
import subprocess
import tempfile
import os
from contextlib import nullcontext

ROOT = Path(__file__).resolve().parents[1]
RUSTC = os.environ.get('RPP_TEST_RUSTC', 'rustc')


def run(*args):
    result = subprocess.run([str(a) for a in args], text=True,
                            stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    if result.returncode:
        raise RuntimeError(f"Command failed: {args}\n{result.stdout}")
    return result.stdout


def snapshot(build):
    return {p.relative_to(build): p.stat().st_mtime_ns
            for p in list(build.rglob('*.o')) + list(build.rglob('*.rlib'))}


def main():
    for tool in ('cmake', 'rustc', 'c++', 'ninja', 'make'):
        if not shutil.which(tool):
            raise RuntimeError(f"Required gate tool missing: {tool}")
    with (nullcontext(tempfile.mkdtemp(prefix='rpp gate ')) if os.environ.get('RPP_KEEP_TEST') else tempfile.TemporaryDirectory(prefix='rpp gate ')) as tmp:
        if os.environ.get('RPP_KEEP_TEST'): print(tmp,flush=True)
        base = Path(tmp)
        sentinel=base/'no-cargo'
        sentinel.mkdir()
        cargo=sentinel/'cargo'
        cargo.write_text('#!/usr/bin/env python3\nraise SystemExit("Cargo is forbidden in this test")\n')
        cargo.chmod(0o755)
        os.environ['PATH']=str(sentinel)+os.pathsep+os.environ['PATH']
        package = base / 'package'
        prefix = base / 'installed modules'
        run('cmake', '-S', ROOT, '-B', package, '-G', 'Ninja',
            '-DBUILD_TESTING=OFF', f'-DRPP_RUSTC_EXECUTABLE={RUSTC}', '-DCMAKE_INSTALL_LIBDIR=lib')
        run('cmake', '--build', package)
        run('cmake', '--install', package, '--prefix', prefix)
        modules = prefix / 'share/rplusplus/cmake'
        for gen in ('Ninja', 'Unix Makefiles', 'Ninja Multi-Config'):
            source = base / gen / 'source tree'
            shutil.copytree(ROOT / 'examples/native', source)
            build = base / gen / 'build tree'
            run('cmake', '-S', source, '-B', build, '-G', gen, '-DCMAKE_BUILD_TYPE=Debug',
                f'-DRPLUSPLUS_MODULE_DIR={modules}',
                f'-DCMAKE_Rpp_COMPILER={prefix / "bin/rpp"}', f'-DRPP_RUSTC_EXECUTABLE={RUSTC}')
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
            assert any(p.name == 'value.rs.rlib' for p in changed), changed
            assert not any(p.name.endswith('.cpp.o') for p in changed), changed
            run('ctest', '--test-dir', build, '-C', 'Debug', '--output-on-failure')
            # Reconfiguration must preserve valid generator output.
            cmakelists = source / 'CMakeLists.txt'
            cmakelists.write_text(cmakelists.read_text() + '\n# regenerate\n')
            run('cmake', '--build', build, '--config', 'Debug')
            graph = base / gen / 'graph'
            graph_source=base/gen/'graph source'
            shutil.copytree(ROOT/'examples/multicrate',graph_source)
            run('cmake', '-S', graph_source, '-B', graph, '-G', gen, '-DCMAKE_BUILD_TYPE=Debug',
                f'-DRPLUSPLUS_MODULE_DIR={modules}',
                f'-DCMAKE_Rpp_COMPILER={prefix / "bin/rpp"}', f'-DRPP_RUSTC_EXECUTABLE={RUSTC}')
            for config in configs:
                run('cmake', '--build', graph, '--config', config, '--parallel', '4')
                run('ctest', '--test-dir', graph, '-C', config, '--output-on-failure')
            before_graph=snapshot(graph)
            run('cmake', '--build', graph, '--config', 'Debug')
            assert snapshot(graph)==before_graph
            leaf=graph_source/'base.rs'
            leaf.write_text(leaf.read_text()+'\n// leaf dependency edit\n')
            run('cmake','--build',graph,'--config','Debug','--parallel','4')
            after_graph=snapshot(graph)
            changed=[p for p in before_graph if before_graph[p]!=after_graph[p]]
            assert any(p.name=='base.rs.rlib' for p in changed),changed
            assert any(p.name=='middle.rs.rlib' for p in changed),changed
            assert any(p.name=='app.rs.rlib' for p in changed),changed
            assert not any(p.name.endswith('.cpp.o') for p in changed),changed
            installed=base / gen / 'export'
            run('cmake', '--install', graph, '--config', 'Debug', '--prefix', installed)
            relocated=base/gen/'relocated export'
            installed.rename(relocated)
            installed=relocated
            consumer=base / gen / 'consumer'
            consumer.mkdir()
            (consumer/'main.cpp').write_text((ROOT/'examples/multicrate/main.cpp').read_text())
            (consumer/'CMakeLists.txt').write_text(
                'cmake_minimum_required(VERSION 3.25)\n'
                'list(PREPEND CMAKE_MODULE_PATH "${RPLUSPLUS_MODULE_DIR}")\n'
                'project(Consumer LANGUAGES CXX Rpp)\n'
                'include("${BRIDGE_PREFIX}/lib/cmake/Bridge/BridgeTargets.cmake")\n'
                'add_executable(Consumer main.cpp)\n'
                'target_link_libraries(Consumer PRIVATE bridge)\n'
                'enable_testing()\nadd_test(NAME external COMMAND Consumer)\n')
            external=base/gen/'external'
            run('cmake','-S',consumer,'-B',external,'-G',gen,'-DCMAKE_BUILD_TYPE=Debug',
                f'-DRPLUSPLUS_MODULE_DIR={modules}',f'-DBRIDGE_PREFIX={installed}',
                f'-DCMAKE_Rpp_COMPILER={prefix / "bin/rpp"}', f'-DRPP_RUSTC_EXECUTABLE={RUSTC}')
            run('cmake','--build',external,'--config','Debug')
            run('ctest','--test-dir',external,'-C','Debug','--output-on-failure')
            print(f'PASS {gen}: installed modules, compiler check, mixed link, execution, '
                  'no-op, selective module rebuild, regeneration', flush=True)


if __name__ == '__main__':
    main()
