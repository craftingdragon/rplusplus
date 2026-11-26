#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""M1 stable-rustc corpus and real multi-file bootstrap rebuild regression."""
from pathlib import Path
import hashlib
import os
import shutil
import subprocess
import tempfile
from generators import ROOT, RUSTC, CMAKE, run


def oracle(base):
    probe = base/'lexer probe'
    run(RUSTC, '--edition=2024', ROOT/'tests/lexer_probe.rs', '-o', probe)
    corpus = [
        'fn main(){let café=1; let cafe\u0301=2; let 東京=3; let r#gen=café+cafe\u0301+東京; assert_eq!(r#gen,6);}',
        '#![allow(dead_code)]\nfn f<\'a>(x:&\'a str)->&\'a str{x} fn main(){\'label: loop {break \'label;}}',
        'macro_rules! sink {($($t:tt)*)=>{}} sink!{ "abc"suffix b"\\xFF"tag r##"NAME"##suffix \'r#gen } fn main(){}',
        'macro_rules! sink {($($t:tt)*)=>{}} sink!{\n#define NAME 1\n} fn main(){}',
        '#[doc=include_str!("doc.txt")] fn main(){let _=c"Grüezi";let _=cr#"C NAME"#; let _=br"ASCII";let _=b\'\\xff\';let _=\'\\u{1f980}\';}',
        '\ufeff#!/not/executed\r\nfn main(){let s="Grüezi\r\n🦀";assert!(!s.is_empty());}\r\n',
        'fn main(){let _=1..=2;let _=1.;let _=1e-2;let _=0xffu32;let x=((1,2),3);let _=x.0.1;}',
    ]
    (base/'doc.txt').write_text('Unicode 🦀 documentation')
    for edition in ('2021', '2024'):
        for n, text in enumerate(corpus):
            path = base/f'valid{n}.rs'
            path.write_bytes(text.encode())
            result = subprocess.run([str(probe), edition, str(path)], capture_output=True, timeout=30)
            assert result.returncode == 0 and result.stdout == path.read_bytes(), (text, result.stderr)
            run(RUSTC, '--edition='+edition, path, '--emit=metadata', '-o', base/'corpus.rmeta')
        malformed = ['r#"unfinished', '/* /* x */', 'm!([)]', 'b"é"', '"\\u{d800}"', 'r#self', '"\\q"', '0b2', '2e+']
        for n, text in enumerate(malformed):
            path = base/f'invalid{n}.rs'
            path.write_text(text)
            result = subprocess.run([str(probe), edition, str(path)], capture_output=True, timeout=30)
            assert result.returncode != 0, text
            compiler = subprocess.run([RUSTC, '--edition='+edition, str(path), '--emit=metadata', '-o', str(base/'bad.rmeta')], capture_output=True, timeout=30)
            assert compiler.returncode != 0, text
    path = base/'guarded.rs'
    path.write_text('macro_rules! sink {($($t:tt)*)=>{}} sink!(### #"foo"#); fn main(){}')
    for edition, success in (('2021', True), ('2024', False)):
        for command in ([str(probe), edition, str(path)], [RUSTC, '--edition='+edition, str(path), '--emit=metadata', '-o', str(base/'guarded.rmeta')]):
            result = subprocess.run(command, capture_output=True, timeout=30)
            assert (result.returncode == 0) == success, result.stderr


def bootstrap(base):
    source = base/'bootstrap source'
    source.mkdir()
    for name in ('CMakeLists.txt', 'README.md', 'LICENSE'):
        shutil.copy2(ROOT/name, source/name)
    for name in ('src', 'cmake', 'docs', 'LICENSES'):
        shutil.copytree(ROOT/name, source/name)
    for gen in ('Ninja', 'Unix Makefiles', 'Ninja Multi-Config'):
        build = base/gen
        run('cmake', '-S', source, '-B', build, '-G', gen, '-DCMAKE_BUILD_TYPE=Debug', '-DRPP_RUSTC_EXECUTABLE='+RUSTC)
        def build_now(): return run('cmake', '--build', build, '--config', 'Debug', '--parallel', '4')
        def stamps(): return tuple((build/'bin'/name).stat().st_mtime_ns for name in ('rpp', 'rpp-tests'))
        build_now()
        run(build/'bin/rpp-tests')
        original = stamps()
        log = build_now()
        assert stamps() == original, (gen, 'no-op', original, stamps(), log)
        for module in ('lex/scanner.rs', 'preprocess/engine.rs', 'preprocess/expression.rs', 'preprocess_cli/artifacts.rs', 'preprocess_cli/snapshot.rs'):
            scanner = source/'src'/module
            scanner.write_text(scanner.read_text()+'\n// bootstrap dependency probe\n')
            build_now()
            changed = stamps()
            assert all(a != b for a,b in zip(original, changed)), (gen, 'module edit', module)
            original = changed
        for module in ('lex/tests.rs', 'preprocess/tests.rs'):
            tests = source/'src'/module
            tests.write_text(tests.read_text()+'\n// test-only dependency probe\n')
            build_now()
            tested = stamps()
            assert tested[0] == changed[0] and tested[1] != changed[1], (gen, 'test-only edit', module)
            changed = tested
        for dep in build.rglob('*.d'): dep.unlink()
        scanner.write_text(scanner.read_text()+'\n// edit after depfile deletion\n')
        build_now()
        recovered = stamps()
        assert all(a != b for a,b in zip(tested, recovered)), (gen, 'depfile recovery')
        build_now()
        assert stamps() == recovered
        run(build/'bin/rpp-tests')
        for name in ('rpp', 'rpp-tests'):
            (build/'bin'/name).unlink()
            build_now()
            assert (build/'bin'/name).is_file(), (gen, 'missing bootstrap binary', name)
            restored = stamps()
            build_now()
            assert stamps() == restored
        for bad, target in ((scanner, 'rpp'), (tests, 'rpp-tests')):
            old_source = bad.read_text()
            before_bytes = {n: hashlib.sha256((build/'bin'/n).read_bytes()).hexdigest() for n in ('rpp','rpp-tests')}
            dep = build/('rpp.d' if target == 'rpp' else 'rpp-tests.d')
            old_dep = (dep.read_bytes(), dep.stat().st_mtime_ns)
            # Dep-info-only succeeds, but code generation must reject this type
            # error. Its dependency stamp cannot commit a failed bootstrap.
            bad.write_text(old_source+'\nfn bootstrap_failure() { let _: () = 1; }\n')
            for _ in range(2):
                failed = subprocess.run([CMAKE, '--build', str(build), '--config', 'Debug', '--parallel', '4'], capture_output=True, text=True, timeout=180)
                assert failed.returncode != 0 and 'mismatched types' in failed.stdout+failed.stderr, failed
                assert before_bytes == {n: hashlib.sha256((build/'bin'/n).read_bytes()).hexdigest() for n in before_bytes}
                assert old_dep == (dep.read_bytes(), dep.stat().st_mtime_ns)
            bad.write_text(old_source)
            build_now()
            run(build/'bin/rpp-tests')
            recovered = stamps()
            build_now()
            assert stamps() == recovered
        print('PASS lexer bootstrap', gen, flush=True)


def main():
    with tempfile.TemporaryDirectory(prefix='rpp lexer 🦀 ') as tmp:
        base = Path(tmp)
        sentinel = base/'forbidden'
        sentinel.mkdir()
        cargo = sentinel/'cargo'
        cargo.write_text('#!/usr/bin/env python3\nraise SystemExit("Cargo forbidden")\n')
        cargo.chmod(0o755)
        os.environ['PATH'] = str(sentinel)+os.pathsep+os.environ['PATH']
        oracle(base)
        bootstrap(base)


if __name__ == '__main__':
    main()
