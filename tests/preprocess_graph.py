#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Offline M0 custom-command ownership experiment. No production preprocessing."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import tempfile
import time
import traceback
from preprocess_feasibility import ROOT, run, write, generator_probes, rust_probes, diagnostic_probes

PAYLOAD = ('root.rs', 'module.rs', 'module/nested.rs', 'data.txt', 'map', 'manifest', 'complete')
GENERATORS = ('Ninja', 'Unix Makefiles', 'Ninja Multi-Config')

CMAKE = r'''cmake_minimum_required(VERSION 3.25)
find_package(Rplusplus CONFIG REQUIRED PATHS "${PREFIX}/share/rplusplus/cmake" NO_DEFAULT_PATH)
project(M0Graph LANGUAGES CXX Rpp)
find_package(Python3 REQUIRED COMPONENTS Interpreter)
foreach(t IN ITEMS App Other)
  set(tree "${CMAKE_CURRENT_BINARY_DIR}/state/${t}/$<CONFIG>")
  set(spec "${CMAKE_CURRENT_BINARY_DIR}/${t}-$<CONFIG>.json")
  file(GENERATE OUTPUT "${spec}" CONTENT "{\"source\":\"${CMAKE_CURRENT_SOURCE_DIR}/${t}\",\"tree\":\"${tree}\",\"trace\":\"${TRACE}\"}")
  set(payload)
  foreach(name IN ITEMS root.rs module.rs module/nested.rs data.txt map manifest)
    list(APPEND payload "${tree}/${name}")
  endforeach()
  set(outputs "${tree}/complete")
  set(byproducts)
  if(MODE STREQUAL "byproducts")
    set(byproducts ${payload})
  else()
    list(APPEND outputs ${payload})
  endif()
  if(CMAKE_GENERATOR STREQUAL "Unix Makefiles")
    list(APPEND outputs "${tree}/preprocess.d")
  endif()
  add_custom_command(OUTPUT ${outputs} BYPRODUCTS ${byproducts}
    COMMAND "${Python3_EXECUTABLE}" "${CMAKE_CURRENT_SOURCE_DIR}/helper.py" emit "${spec}"
    DEPENDS "${spec}" "${CMAKE_CURRENT_SOURCE_DIR}/helper.py"
      "${CMAKE_CURRENT_SOURCE_DIR}/${t}/root.rs" "${CMAKE_CURRENT_SOURCE_DIR}/${t}/data.txt"
      "${CMAKE_CURRENT_SOURCE_DIR}/${t}/mapping.txt"
      "${CMAKE_CURRENT_SOURCE_DIR}/${t}/selector"
    DEPFILE "${tree}/preprocess.d" VERBATIM)
  add_custom_target(${t}_tree DEPENDS ${outputs} ${payload})
  add_executable(${t} "${t}/root.rs" "${t}/worker.cpp")
  add_dependencies(${t} ${t}_tree)
  target_compile_options(${t} PRIVATE "$<$<COMPILE_LANGUAGE:Rpp>:--m0-tree=${tree}>")
  target_link_options(${t} PRIVATE "--m0-tree=${tree}")
  if(CMAKE_CONFIGURATION_TYPES)
    set(depconfig "${CMAKE_CFG_INTDIR}")
  else()
    set(depconfig "${CMAKE_BUILD_TYPE}")
  endif()
  foreach(name IN ITEMS root.rs module.rs module/nested.rs data.txt map manifest)
    set_property(SOURCE "${t}/root.rs" APPEND PROPERTY OBJECT_DEPENDS "${CMAKE_CURRENT_BINARY_DIR}/state/${t}/${depconfig}/${name}")
  endforeach()
endforeach()
'''


def events(trace):
    return [json.loads(l) for l in trace.read_text().splitlines()] if trace.exists() else []


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def acceptance(source, build, tree, trace, adapter, cmd, gen, config):
    """Assertions on the chosen graph, not on the deliberately broken controls."""
    exe = build/config/'App' if gen == 'Ninja Multi-Config' else build/'App'
    def check(value):
        assert run([exe]).stdout.strip() == str(value)
    def build_now():
        trace.write_text('')
        run(cmd)
        return events(trace)
    def noop():
        before = {str(p): p.stat().st_mtime_ns for p in tree.rglob('*') if p.is_file()}
        assert not build_now(), 'not a zero-command no-op'
        assert before == {str(p): p.stat().st_mtime_ns for p in tree.rglob('*') if p.is_file()}
    check(14)
    original_mapping = (source/'App'/'mapping.txt').read_text()
    write(source/'App', 'mapping.txt', original_mapping+'changed fixture map '+config+'\n')
    mapping = build_now()
    assert any(e['kind'] == 'compile' for e in mapping), (gen, config, 'metadata file-level dependency missing', mapping)
    check(14)
    noop()
    # A touched input with unchanged bytes schedules once, then settles.
    before = {n: (tree/n).stat().st_mtime_ns for n in PAYLOAD if n != 'complete'}
    (source/'App'/'data.txt').touch()
    unchanged = build_now()
    assert any(e['kind'] == 'preprocess' for e in unchanged)
    after = {n: (tree/n).stat().st_mtime_ns for n in before}
    noop()
    # Changes must follow the target graph without rebuilding unrelated targets.
    write(source/'App', 'leaf.txt', '20')
    changed = build_now()
    check(24)
    assert any(e['kind'] == 'compile' for e in changed)
    assert not any('/Other/' in ' '.join(e['args']) or 'Other-' in ' '.join(e['args']) for e in changed), changed
    rust = [e for e in changed if e['kind'] == 'rustc']
    assert len(rust) >= 2  # metadata stage plus real final-link recompilation
    assert all(str(tree/'root.rs') in e['args'] for e in rust)
    # No no-op/consolidation pass between introducing and deleting dependencies.
    write(source/'App', 'new.txt', '30')
    write(source/'App', 'selector', 'new.txt')
    build_now()
    check(34)
    raw_before = {'preprocess': (tree/'preprocess.d').exists(),
        'compiler': [str(p.relative_to(build)) for p in build.rglob('*.rs.rlib.d')]}
    for p in build.rglob('*.d'):
        p.unlink()
    write(source/'App', 'new.txt', '40')
    # Also edit a rustc-discovered input after deleting compiler depfiles.
    write(source/'App', 'data.txt', '3')
    recovered = build_now()
    check(45)
    assert any(e['kind'] == 'preprocess' for e in recovered)
    assert any(e['kind'] == 'compile' for e in recovered)
    noop()
    # Interrupt a real build after it invalidates completion but before finishing.
    compile_args = next(e['args'] for e in recovered if e['kind'] == 'compile' and '--m0-tree='+str(tree) in e['args'])
    link_args = next(e['args'] for e in recovered if e['kind'] == 'link' and '--m0-tree='+str(tree) in e['args'])
    old = digest(exe)
    original_root = (source/'App'/'root.rs').read_text()
    write(source/'App', 'new.txt', '50')
    write(source/'App', 'root.rs', original_root+'// interrupted generation\n')
    write(source/'App', 'pause', '')
    ready = source/'App'/'ready'
    ready.unlink(missing_ok=True)
    trace.write_text('')
    log = build/'interrupted.log'
    with log.open('w') as stream:
        process = subprocess.Popen(list(map(str, cmd)), stdout=stream, stderr=stream, start_new_session=True)
        try:
            deadline = time.monotonic()+30
            while not ready.exists():
                if process.poll() is not None or time.monotonic() > deadline:
                    raise RuntimeError('interruption barrier not reached: '+log.read_text())
                time.sleep(.01)
            assert (tree/'root.rs').read_text() != original_root
            os.killpg(process.pid, signal.SIGTERM)
            process.wait(timeout=10)
        finally:
            if process.poll() is None:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait(timeout=10)
    assert not (tree/'complete').exists()
    # Ninja may remove modified declared outputs when the command is interrupted.
    assert not (tree/'root.rs').exists() or (tree/'root.rs').read_text() != original_root
    assert digest(exe) == old
    assert not any(e['kind'] in ('compile', 'link') for e in events(trace))
    # Direct compile AND final-link entry must reject this incomplete snapshot,
    # even when the build system is bypassed. Neither can invoke rustc.
    for argv in (compile_args, link_args):
        trace.write_text('')
        p = run([adapter, *argv], ok=False)
        assert p.returncode and 'M0 snapshot rejected' in p.stderr, (argv, p.returncode, p.stderr)
        assert not any(e['kind'] == 'rustc' for e in events(trace))
        assert digest(exe) == old
    (source/'App'/'pause').unlink()
    build_now()
    check(55)
    noop()
    # A complete marker alone cannot authenticate a partially changed tree.
    (tree/'module.rs').write_text('corrupt partial snapshot')
    p = run([adapter, *link_args], ok=False)
    assert p.returncode and 'M0 snapshot rejected' in p.stderr
    (tree/'complete').unlink()
    build_now()
    check(55)
    noop()
    # Restore inputs before the next configuration's independent run.
    write(source/'App', 'selector', 'leaf.txt')
    write(source/'App', 'leaf.txt', '10')
    write(source/'App', 'data.txt', '2')
    write(source/'App', 'root.rs', original_root)
    write(source/'App', 'mapping.txt', original_mapping)
    build_now()
    check(14)
    noop()
    return {'raw_depfiles_after_build': raw_before,
        'unchanged_input_payload_timestamps_preserved': before == after,
        'unchanged_input_commands': [e['kind'] for e in unchanged],
        'changed_commands': [e['kind'] for e in changed],
        'interrupt_recovery': 'passed', 'final_recompile': 'passed'}


def setup(base, prefix, rustc):
    source = base/'consumer 🦀'
    source.mkdir(parents=True)
    shutil.copy2(ROOT/'tests/preprocess_graph_fixture.py', source/'helper.py')
    write(source, 'CMakeLists.txt', CMAKE)
    trace = base/'trace'
    adapter = base/'rpp-adapter'
    rustc_wrapper, cxx_wrapper = base/'rustc-wrapper', base/'cxx-wrapper'
    config = base/'adapter.json'
    # The adapter selects each target's root from the original argv, below.
    config.write_text(json.dumps({'trace': str(trace), 'driver': str(prefix/'bin/rpp'),
        'rustc': str(rustc), 'cxx': shutil.which('c++'), 'root': '',
        'rustc_wrapper': str(rustc_wrapper)}))
    for path, op in ((adapter, 'adapter'), (rustc_wrapper, 'tool'), (cxx_wrapper, 'tool')):
        extra = '' if op == 'adapter' else ' '+('rustc' if path == rustc_wrapper else 'cxx')
        # Avoid shell forwarding/quoting: Python shim passes argv directly.
        text = '#!'+sys.executable+'\nimport os,sys\nos.execv(sys.executable, [sys.executable, '+repr(str(source/'helper.py'))+', '+repr(op)+', '+repr(str(config))
        if extra:
            text += ', '+repr(extra.strip())
        text += ', *sys.argv[1:]])\n'
        path.write_text(text)
        path.chmod(0o755)
    for target in ('App', 'Other'):
        write(source, target+'/root.rs', 'mod module; unsafe extern "C" {fn worker()->i32;} fn main(){assert_eq!(include_bytes!("data.txt"),include_str!("data.txt").as_bytes()); println!("{}",module::VALUE+module::nested::VALUE+include_str!("data.txt").trim().parse::<i32>().unwrap()+unsafe{worker()});}\n')
        write(source, target+'/worker.cpp', 'extern "C" int worker(){return 1;}\n')
        write(source, target+'/selector', 'leaf.txt')
        write(source, target+'/leaf.txt', '10')
        write(source, target+'/data.txt', '2')
        write(source, target+'/mapping.txt', 'M0 fixture identity source map\n')
    return source, trace, adapter, cxx_wrapper


def probe(base, cmake, rustc, prefix, gen, mode):
    source, trace, adapter, cxx = setup(base, prefix, rustc)
    build = base/'build'
    run([cmake, '-S', source, '-B', build, '-G', gen, '-DCMAKE_BUILD_TYPE=Debug',
        '-DPREFIX='+str(prefix), '-DTRACE='+str(trace), '-DMODE='+mode,
        '-DCMAKE_Rpp_COMPILER='+str(adapter), '-DCMAKE_CXX_COMPILER='+str(cxx),
        '-DRPP_RUSTC_EXECUTABLE='+str(rustc)])
    observations = []
    for config in ('Debug', 'Release'):
        if gen != 'Ninja Multi-Config':
            run([cmake, '-S', source, '-B', build, '-DCMAKE_BUILD_TYPE='+config])
        cmd = [cmake, '--build', build, '--config', config, '--parallel', '4']
        first = run(cmd, ok=False)
        if first.returncode and mode == 'byproducts':
            observations.append({'config': config, 'file': 'initial parallel build',
                'recovered': False, 'log': first.stdout+first.stderr, 'events': events(trace)})
            continue
        assert first.returncode == 0, first.stdout+first.stderr
        tree = build/'state'/'App'/config
        for name in PAYLOAD:
            inactive = {str(p): p.stat().st_mtime_ns for p in (build/'state').rglob('*')
                if p.is_file() and (str(tree) not in str(p))}
            output = tree/name
            output.unlink()
            trace.write_text('')
            p = run(cmd, ok=False)
            recovered = p.returncode == 0 and output.exists()
            observations.append({'config': config, 'file': name, 'recovered': recovered,
                'log': p.stdout+p.stderr if not recovered else '', 'events': events(trace)})
            if not recovered:
                # Comparison negative control; do not hide failed recovery.
                # Force a fresh preprocessing command for subsequent cases.
                (tree/'complete').unlink(missing_ok=True)
                run(cmd)
            if mode == 'outputs':
                assert recovered, (gen, config, name, p.stdout, p.stderr)
                assert inactive == {str(p): p.stat().st_mtime_ns for p in (build/'state').rglob('*')
                    if p.is_file() and (str(tree) not in str(p))}, 'another target/config output modified'
            trace.write_text('')
            before = {str(p): p.stat().st_mtime_ns for p in tree.rglob('*') if p.is_file()}
            run(cmd)
            assert not events(trace), (gen, mode, config, name, events(trace))
            assert before == {str(p): p.stat().st_mtime_ns for p in tree.rglob('*') if p.is_file()}
        if mode == 'outputs':
            observations.append({'config': config, 'acceptance': acceptance(
                source, build, tree, trace, adapter, cmd, gen, config)})
    if mode == 'outputs':
        run([*cmd[:2], build, '--config', config, '--target', 'clean'])
        trace.write_text('')
        run(cmd)
        assert any(e['kind'] == 'preprocess' for e in events(trace))
        exe = build/config/'App' if gen == 'Ninja Multi-Config' else build/'App'
        assert run([exe]).stdout.strip() == '14'
        trace.write_text('')
        run(cmd)
        assert not events(trace)
    # Recipe dependencies must refer exclusively to the relocated tools/consumer.
    for p in build.rglob('*'):
        if p.is_file() and p.suffix in ('.cmake', '.make', '.ninja', '.json'):
            assert str(ROOT) not in p.read_text(errors='replace'), p
    return observations


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cmake', action='append', required=True, type=Path)
    parser.add_argument('--rustc', action='append', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    report = {'schema': 1, 'stage': 'M0/custom-output', 'status': 'blocked', 'results': []}
    for rustc in args.rustc:
        for cmake in args.cmake:
            entry = {'rustc': str(rustc), 'cmake': str(cmake)}
            report['results'].append(entry)
            try:
                entry['rustc_version'] = run([rustc, '--version']).stdout.strip()
                entry['cmake_version'] = run([cmake, '--version']).stdout.splitlines()[0]
                assert entry['rustc_version'].split()[1] in ('1.85.0', '1.100.0')
                assert entry['cmake_version'].split()[-1] in ('3.25.0', '4.5.0')
                with tempfile.TemporaryDirectory(prefix='rpp M0 graph ') as tmp:
                    base = Path(tmp)
                    producer = base/'producer'
                    producer.mkdir()
                    for name in ('CMakeLists.txt', 'README.md', 'LICENSE'):
                        shutil.copy2(ROOT/name, producer/name)
                    for name in ('cmake', 'src', 'docs', 'LICENSES'):
                        shutil.copytree(ROOT/name, producer/name)
                    package = base/'package'
                    prefix = base/'installed'
                    run([cmake, '-S', producer, '-B', package, '-G', 'Ninja', '-DBUILD_TESTING=OFF', '-DRPP_RUSTC_EXECUTABLE='+str(rustc)])
                    run([cmake, '--build', package])
                    run([cmake, '--install', package, '--prefix', prefix])
                    moved = base/'relocated'
                    prefix.rename(moved)
                    shutil.rmtree(package)
                    shutil.rmtree(producer)
                    assert not package.exists() and not producer.exists()
                    control = base/'old-native-control'
                    control.mkdir()
                    entry['old_native_control'] = generator_probes(control, cmake, rustc, moved/'bin/rpp')
                    assert all(o['recovered'] == (o['generator'] == 'Unix Makefiles')
                        for o in entry['old_native_control']), 'negative control behavior changed'
                    lexical = base/'lexical'
                    lexical.mkdir()
                    entry['source_resolution'] = rust_probes(lexical, rustc)
                    entry['diagnostic_feasibility'] = diagnostic_probes(lexical, rustc)
                    entry['graphs'] = []
                    for gen in GENERATORS:
                        for mode in ('byproducts', 'outputs'):
                            folder = base/(gen+'-'+mode)
                            entry['graphs'].append({'generator': gen, 'mode': mode,
                                'observations': probe(folder, cmake, rustc, moved, gen, mode)})
                assert any(not o['recovered'] for g in entry['graphs'] if g['mode'] == 'byproducts'
                    for o in g['observations']), 'missing BYPRODUCTS negative control'
                entry['status'] = 'passed'
            except OSError as error:
                entry.update(status='blocked' if not cmake.is_file() or not rustc.is_file() else 'failed',
                    error=str(error), traceback=traceback.format_exc())
            except (AssertionError, RuntimeError, subprocess.SubprocessError, ValueError) as error:
                entry.update(status='failed', error=str(error), traceback=traceback.format_exc())
            args.output.parent.mkdir(parents=True, exist_ok=True)
            args.output.write_text(json.dumps(report, indent=2)+'\n')
            print(entry['cmake'], entry['rustc'], entry['status'], entry.get('error', ''), flush=True)
    actual = {(e.get('rustc_version', '').split()[1] if e.get('rustc_version') else '',
        e.get('cmake_version', '').split()[-1] if e.get('cmake_version') else '') for e in report['results']}
    expected = {(r, c) for r in ('1.85.0', '1.100.0') for c in ('3.25.0', '4.5.0')}
    report['status'] = ('passed' if actual == expected and
        all(e['status'] == 'passed' for e in report['results']) else
        'failed' if any(e['status'] == 'failed' for e in report['results']) else 'blocked')
    args.output.write_text(json.dumps(report, indent=2)+'\n')
    return int(report['status'] != 'passed')


if __name__ == '__main__':
    raise SystemExit(main())
