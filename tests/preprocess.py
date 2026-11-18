#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""M2 pure-library oracle: generated Rust compiles with the selected stable rustc."""
from pathlib import Path
import os
import subprocess
import tempfile
from generators import ROOT, RUSTC, run


def main():
    with tempfile.TemporaryDirectory(prefix="rpp M2 🦀 space ' $ ") as tmp:
        base = Path(tmp)
        forbidden = base/'forbidden'
        forbidden.mkdir()
        cargo = forbidden/'cargo'
        cargo.write_text('#!/usr/bin/env python3\nraise SystemExit("Cargo forbidden")\n')
        cargo.chmod(0o755)
        os.environ['PATH'] = str(forbidden)+os.pathsep+os.environ['PATH']
        probe = base/'preprocessing probe'
        run(RUSTC, '--edition=2024', ROOT/'tests/preprocess_probe.rs', '-o', probe)
        (base/'search directory').mkdir()
        # A protected macro deliberately uses a name also defined by #define.
        # Rust includes below remain ordinary Rust macros, never preprocessing
        # includes. Their filesystem/tree validation belongs to M4.
        text = '''\ufeff#!/not/executed\r
#define VALUE 7
#define ALIAS VALUE
#define JOIN crate::helpers
#define CALL answer!
#define EMPTY
#define NAME 999
#include "é fragment.rph"
#if defined(CONFIG) && CONFIG == 1 && (0xff >= 255) && !MISSING
const RESULT: i32 = ALIAS;
#elif 1
#error wrong branch
#endif
#undef VALUE
#define VALUE 11
const LATE: i32 = ALIAS;
mod helpers { pub fn value() -> i32 { 3 } }
macro_rules! answer { (NAME) => { 19 }; }
fn main() {
    let café = RESULT + LATE + fragment() + JOIN::value() + CALL(NAME);
    let NAMÉ = 2;
    let r#NAME = 1;
    let s = r###"NAME VALUE #error not a directive"###;
    let ch = 'N'; let bytes = b"NAME";
    assert_eq!(café + NAMÉ + r#NAME, 48);
    assert_eq!(s.len(), 33);
    assert_eq!(ch, 'N'); assert_eq!(bytes, b"NAME");
    assert_eq!(include!("rust.inc"), 5);
    assert_eq!(include_str!("data.txt"), "Grüezi 🦀");
    assert_eq!(include_bytes!("data.bin"), b"\\x00\\xff");
    let _: EMPTY () = ();
}
'''
        files = {
            'root.rs': text,
            'é fragment.rph': '#include "value.rph"\nfn fragment() -> i32 { FRAGMENT }',
            'search directory/value.rph': '#define FRAGMENT 5\n',
        }
        for name, content in files.items():
            (base/name).write_bytes(content.encode())
        (base/'rust.inc').write_text('5')
        (base/'data.txt').write_text('Grüezi 🦀')
        (base/'data.bin').write_bytes(b'\x00\xff')

        def process(edition):
            return subprocess.run([str(probe), edition, 'root.rs', *files],
                                  cwd=base, capture_output=True, timeout=30)

        for edition in ('2021', '2024'):
            first, second = process(edition), process(edition)
            assert first.returncode == 0, first.stderr.decode()
            assert (first.stdout, first.stderr) == (second.stdout, second.stderr)
            assert b'include!("rust.inc")' in first.stdout
            assert b'CALL(NAME)' not in first.stdout and b'answer! (NAME)' in first.stdout
            assert b'dependencies: {"root.rs", "search directory/value.rph", "' in first.stderr
            generated = base/'generated.rs'
            generated.write_bytes(first.stdout)
            program = base/'program'
            run(RUSTC, '--edition='+edition, generated, '-o', program)
            run(program)
            print('PASS M2 generated Rust', edition, flush=True)

        # The library returns no partial successful output after late errors.
        for fragment, expected in [
            ('// 🦀\r\n#define X X\r\n  X\r\n', 'é fragment.rph:3:3: Recursion'),
            ('#error failed after earlier output', 'é fragment.rph:1:2: ExplicitError'),
            ('#include "not declared"', 'é fragment.rph:1:2: Include'),
            ('#if 1\nfn f() {}', 'é fragment.rph:1:2: Conditional'),
        ]:
            (base/'é fragment.rph').write_bytes(fragment.encode())
            failed = process('2024')
            assert failed.returncode == 1 and not failed.stdout, failed
            assert expected in failed.stderr.decode(), failed.stderr.decode()
            assert 'included from root.rs:8:2' in failed.stderr.decode()
        print('PASS M2 original diagnostics and failure isolation', flush=True)


if __name__ == '__main__':
    main()
