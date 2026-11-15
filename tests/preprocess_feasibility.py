#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""M0 probes only: stable rustc, mirrored inputs and native rule co-outputs.

No production preprocessor is exercised or supplied by this test.
"""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]


def run(args, *, ok=True):
    p = subprocess.run(list(map(str, args)), capture_output=True, text=True, timeout=180)
    if ok and p.returncode:
        raise RuntimeError(f'{args}\n{p.stdout}\n{p.stderr}')
    return p


def write(root, name, text):
    p = root / name
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(text)
    return p


def rust_probes(base, rustc):
    source = base / 'original 🦀'
    files = {
        'main.rs': 'mod outer; #[path="alternate.rs"] mod alternate; include!("fragment.rs"); fn main(){assert_eq!(outer::n()+alternate::N+included(), 10); assert_eq!(include_str!("text.txt"), "hé"); assert_eq!(include_bytes!("bytes.bin"), b"42");}',
        'outer.rs': 'pub mod nested; pub fn n()->i32 {nested::N}',
        'outer/nested.rs': 'pub const N:i32=2;',
        'alternate.rs': 'pub const N:i32=3;',
        'fragment.rs': 'include!("fragments/inner.rs"); fn included()->i32 {INCLUDED}',
        'fragments/inner.rs': 'const INCLUDED:i32=5;',
        'text.txt': 'hé', 'bytes.bin': '42',
    }
    for name, text in files.items():
        write(source, name, text)
    mirror = base / 'mirror 🦀'
    shutil.copytree(source, mirror)
    shutil.rmtree(source)  # resolution must not fall back to the original tree
    for edition in ('2021', '2024'):
        dep = base / f'tree-{edition}.d'
        exe = base / f'tree-{edition}'
        run([rustc, '--edition='+edition, '--crate-name=probe', mirror/'main.rs',
             '--emit=link,dep-info='+str(dep), '-o', exe])
        run([exe])
        text = dep.read_text()
        for name in files:
            assert name in text, (name, text)

    # Local and external declarative macros can introduce otherwise invisible reads.
    outside = write(base, 'undeclared.txt', '23')
    macros = write(base, 'macros.rs', '#[macro_export] macro_rules! read { ($p:expr) => {include_str!($p)}; }')
    lib = base/'libreads.rlib'
    run([rustc, '--crate-name=reads', '--crate-type=rlib', macros, '-o', lib])
    generated = write(base, 'macro_reads.rs', f'fn main(){{assert_eq!(reads::read!({json.dumps(str(outside))}), "23");}}')
    dep = base/'macro.d'
    run([rustc, '--edition=2024', generated, '--extern=reads='+str(lib),
         '--emit=link,dep-info='+str(dep), '-o', base/'macro'])
    run([base/'macro'])
    assert str(outside).replace(' ', '\\ ') in dep.read_text()
    # Dep-info identifies the path, not whether its bytes were Rust or data.
    shared = write(base, 'dual.txt', '23')
    dependency_lines = []
    for macro in ('include', 'include_str', 'include_bytes'):
        root = write(base, f'{macro}.rs', f'fn main(){{let _ = {macro}!({json.dumps(str(shared))});}}')
        d = base/f'{macro}.d'
        run([rustc, root, '--emit=metadata,dep-info='+str(d), '-o', base/f'{macro}.rmeta'])
        assert str(shared).replace(' ', '\\ ') in d.read_text()
        dependency_lines.append(str(shared))
    escaped = write(base, 'escaped.rs', 'pub const VALUE:i32=23;')
    write(base, 'macros.rs', macros.read_text() + '\n#[macro_export] macro_rules! load {()=>{#[path='+json.dumps(str(escaped))+'] mod escaped;};}')
    run([rustc, '--crate-name=reads', '--crate-type=rlib', macros, '-o', lib])
    generated.write_text('reads::load!(); fn main(){assert_eq!(escaped::VALUE,23);}')
    run([rustc, '--edition=2024', generated, '--extern=reads='+str(lib),
         '--emit=link,dep-info='+str(dep), '-o', base/'macro'])
    run([base/'macro'])
    assert str(escaped).replace(' ', '\\ ') in dep.read_text()
    return {'mirror_editions': ['2021', '2024'], 'macro_read_reported': True,
            'macro_generated_absolute_module_reported': True,
            'dep_info_read_roles': 'not supplied by stable dep-info',
            'dual_use_path': dependency_lines[0]}


def diagnostic_probes(base, rustc):
    cases = {
        'unicode-type': 'fn main(){let café = 1; let _: () = café;}\n',
        'substitution': 'fn main(){let _ = replacement_missing;}\n',
        'borrow': 'fn main(){let mut x=0; let a=&mut x; let b=&mut x; println!("{} {}",a,b);}\n',
        'expansion': 'macro_rules! m {()=>{missing_in_expansion}} fn main(){m!();}\n',
        'multiple': 'fn main(){println!("{} {}");}\n',
    }
    evidence = {}
    for name, text in cases.items():
        path = write(base, name+'.rs', text)
        p = run([rustc, '--crate-name=diagnostic', path, '--error-format=json',
                 '--emit=metadata', '-o', base/'diag.rmeta'], ok=False)
        assert p.returncode != 0
        diagnostics = [json.loads(line) for line in p.stderr.splitlines()]
        errors = [d for d in diagnostics if d.get('level') == 'error' and d.get('spans')]
        assert errors, p.stderr
        primaries = [s for d in errors for s in d['spans'] if s['is_primary']]
        assert primaries
        if name == 'multiple':
            assert any(sum(s['is_primary'] for s in d['spans']) > 1 for d in errors)
        for span in primaries:
            if span['file_name'] == str(path):
                prefix = text.encode()[:span['byte_start']].decode()
                assert span['column_start'] == len(prefix.rsplit('\n', 1)[-1])+1
                # Identity segments map all primary spans independently. No
                # inference from the first primary to later primaries is valid.
                assert text.encode()[span['byte_start']:span['byte_end']].decode()
        if name == 'substitution':
            span = primaries[0]
            assert text.encode()[span['byte_start']:span['byte_end']] == b'replacement_missing'
            # Explicit fixture mapping: replacement token -> original use-site.
            original = 'fn main(){let _ = NAME;}\n'
            start = original.index('NAME')
            mapped = (start, start+4)
            assert original[slice(*mapped)] == 'NAME'
        if name == 'expansion':
            assert any(s.get('expansion') for s in primaries), p.stderr
            # An expansion-bearing span cannot be treated as an ordinary
            # one-to-one segment. Keep the original JSON, including backtrace.
            assert all('rendered' in d for d in errors)
        evidence[name] = errors
    return evidence


def generator_probes(base, cmake, rustc, driver):
    trace = base/'trace'
    wrapper = write(base, 'rpp-wrapper', '#!'+shutil.which('python3')+'\n'+
        'import pathlib, subprocess, sys\n'+
        f'trace=pathlib.Path({str(trace)!r})\n'+
        'with trace.open("a") as f:f.write(sys.argv[1]+"\\n")\n'+
        f'p=subprocess.run([{str(driver)!r}, *sys.argv[1:]])\n'+
        'if p.returncode:raise SystemExit(p.returncode)\n'+
        'if sys.argv[1]=="compile":\n'+
        ' for i,a in enumerate(sys.argv):\n'+
        '  if a.startswith("--cfg=m0_output="):\n'+
        '   root=pathlib.Path(a.split("=",2)[2].strip(chr(34)))\n'+
        '   root.mkdir(parents=True,exist_ok=True)\n'+
        '   for name in ("module.rs","map","complete"):(root/name).write_text("complete")\n')
    wrapper.chmod(0o755)
    source = base/'source'
    write(source, 'main.rs', 'fn main(){}\n')
    write(source, 'CMakeLists.txt', '''cmake_minimum_required(VERSION 3.25)
list(PREPEND CMAKE_MODULE_PATH "${MODULES}")
project(M0 LANGUAGES CXX Rpp)
add_executable(probe main.rs)
set(state "${CMAKE_CURRENT_BINARY_DIR}/state/$<CONFIG>")
target_compile_options(probe PRIVATE "--cfg=m0_output=\\"${state}\\"")
set_property(SOURCE main.rs APPEND PROPERTY OBJECT_OUTPUTS "${state}/module.rs;${state}/map;${state}/complete")
''')
    results = []
    for gen in ('Ninja', 'Unix Makefiles', 'Ninja Multi-Config'):
        build = base/gen
        run([cmake, '-S', source, '-B', build, '-G', gen,
             '-DCMAKE_BUILD_TYPE=Debug', '-DMODULES='+str(ROOT/'cmake'),
             '-DCMAKE_Rpp_COMPILER='+str(wrapper), '-DRPP_RUSTC_EXECUTABLE='+str(rustc)])
        for config in ('Debug', 'Release'):
            if gen != 'Ninja Multi-Config':
                run([cmake, '-S', source, '-B', build, '-DCMAKE_BUILD_TYPE='+config])
            cmd = [cmake, '--build', build, '--config', config, '--parallel', '4']
            run(cmd)
            for name in ('module.rs', 'map', 'complete'):
                output = build/'state'/config/name
                assert output.exists(), output
                output.unlink()
                trace.write_text('')
                log = run(cmd)
                recovered = output.exists() and 'compile' in trace.read_text()
                results.append({'generator': gen, 'configuration': config,
                    'output': name, 'recovered': recovered,
                    'log': log.stdout + log.stderr})
                if not recovered:
                    # Preserve the failing observation; restore fixture only so
                    # subsequent independent generator cases can be measured.
                    output.write_text('restored after failed probe')
                trace.write_text('')
                run(cmd)
                assert trace.read_text() == '', (gen, config, trace.read_text())
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cmake', action='append', required=True, type=Path)
    parser.add_argument('--rustc', action='append', required=True, type=Path)
    parser.add_argument('--output', required=True, type=Path)
    args = parser.parse_args()
    report = {'schema': 1, 'stage': 'v0.2/M0', 'results': []}
    for rustc in args.rustc:
        entry = {'rustc': str(rustc)}
        report['results'].append(entry)
        try:
            entry['version'] = run([rustc, '--version']).stdout.strip()
            if 'nightly' in entry['version'] or 'beta' in entry['version']:
                raise FileNotFoundError('stable rustc required')
            with tempfile.TemporaryDirectory(prefix='rpp M0 ') as tmp:
                base = Path(tmp)
                entry['rust'] = rust_probes(base, rustc)
                entry['diagnostics'] = diagnostic_probes(base, rustc)
                driver = base/'rpp'
                run([rustc, '--edition=2024', ROOT/'src/main.rs', '-o', driver])
                entry['cmake'] = []
                for index, cmake in enumerate(args.cmake):
                    folder = base/f'cmake-{index}'
                    folder.mkdir()
                    entry['cmake'].append({'version': run([cmake, '--version']).stdout.splitlines()[0],
                        'results': generator_probes(folder, cmake, rustc, driver)})
            failures = [r for c in entry['cmake'] for r in c['results'] if not r['recovered']]
            entry['status'] = 'blocked' if failures else 'passed'
            if failures:
                entry['blocker'] = 'Native compile OBJECT_OUTPUTS does not guarantee missing generated-output recovery; architecture change required.'
        except OSError as error:
            entry.update(status='blocked', error=str(error))
        except (AssertionError, RuntimeError, subprocess.SubprocessError, ValueError) as error:
            entry.update(status='failed', error=str(error))
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps(report, indent=2)+'\n')
        print(entry['rustc'], entry['status'], entry.get('error', entry.get('blocker', '')), flush=True)
    return int(any(e['status'] != 'passed' for e in report['results']))


if __name__ == '__main__':
    raise SystemExit(main())
