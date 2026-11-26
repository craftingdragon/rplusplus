#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Run the real snapshot driver, Rust toolchain and Ninja dependency graph."""
import json
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile

from generators import RUSTC


def run(args, cwd, code=0, env=None):
    result = subprocess.run(list(map(str, args)), cwd=cwd, capture_output=True, timeout=40, env=env)
    assert result.returncode == code, (args, result.stdout, result.stderr)
    return result


def driver_cases(driver, base, edition):
    src = base / "original 🦀"
    src.mkdir(parents=True)
    entry = src / "main.rs"
    fragment = src / "frag.rph"
    asset = src / "data #$ é.txt"
    tree = base / "generated tree"
    dep = base / "result.d"
    output = base / "result.rlib"
    program = base / "program"
    asset.write_text("data")
    fragment.write_text("#define VALUE 3\n")
    good = (
        '\ufeff// café\r\n#include "frag.rph"\r\n'
        'fn main() { let value = VALUE; assert_eq!(value,3); '
        'assert_eq!(include_str!("data #$ é.txt"), "data"); }\r\n'
    )
    entry.write_bytes(good.encode())
    prep = [driver, "preprocess", entry, "--source-root", src, "--output-dir", tree,
            "--source-map", tree / "custom map.json", "--input", fragment, "--data", asset, "--edition", edition]
    def compile_args(*extra, kind="bin", rustc=RUSTC, entrypoint="Rpp"):
        return [driver, "compile", "--rustc", rustc, "--rpp-snapshot", tree, "--depfile", dep,
                "--name", "test_crate", "--kind", kind, "--entry", entrypoint,
                "--edition", edition, "-o", output, *extra, entry]

    def link_args(*objects, rustc=RUSTC, destination=program):
        return [driver, "link", "--rustc", rustc, "-o", destination, output, *objects]

    def compile_link():
        run(prep, base)
        args = compile_args()
        response = base / "args.rsp"
        response.write_bytes(("\0".join(map(str, args[1:])) + "\0").encode())
        run([driver, "@" + str(response)], base)
        run(link_args(), base)
        run([program], base)

    compile_link()
    deptext = dep.read_text()
    assert deptext.startswith(str(output).replace(" ", "\\ ") + ":")
    assert "generated\\ tree/main.rs" in deptext and "generated\\ tree/.rpp/manifest" in deptext
    assert "custom\\ map.json" in deptext and "data\\ \\#$$\\ é.txt" in deptext
    assert "original" not in deptext and "frag.rph" not in deptext and ".rpp/complete" not in deptext
    previous = output.read_bytes(), dep.read_bytes(), program.read_bytes()

    bad_compile = compile_args("--error-format=json")
    wrong_edition = list(bad_compile)
    wrong_edition[wrong_edition.index("--edition") + 1] = "2021" if edition == "2024" else "2024"
    assert b"does not match" in run(wrong_edition, base, 1).stderr
    for path in (tree / ".rpp/complete", tree / "main.rs", tree / "custom map.json"):
        saved = path.read_bytes()
        path.unlink()
        assert b"regenerate the tree and rebuild" in run(link_args(), base, 1).stderr
        assert b"regenerate the tree and rebuild" in run(compile_args(), base, 1).stderr
        path.write_bytes(saved)
    fragment.write_text("#define VALUE 4\n")
    run(prep, base)
    assert b"changed since compilation" in run(link_args(), base, 1).stderr
    assert (output.read_bytes(), dep.read_bytes(), program.read_bytes()) == previous
    fragment.write_text("#define VALUE 3\n")

    bad_cases = [
        ('fn main(){ let café: &str = 1; }', b"E0308", b"original source", 2),
        ('fn main(){ let _ = MISSING; }', b"E0425", b"macro defined at", 2),
        ('fn main(){ let mut x=0; let a=&mut x; let b=&mut x; println!("{} {}",a,b); }',
         b"E0499", b"original source", 2),
        ('fn main(){ println!("{} {}"); }', b"positional arguments", b"original source", 2),
    ]
    for text, message, mapping, line in bad_cases:
        fragment.write_text("#define MISSING unresolved_name\n")
        entry.write_bytes(('\ufeff// café\r\n' + text + '\r\n#include "frag.rph"\r\n').encode()
                          if "MISSING" not in text else ('\ufeff#include "frag.rph"\r\n' + text + '\r\n').encode())
        run(prep, base)
        error = run(compile_args(), base, 1).stderr
        assert message in error and mapping in error and f"main.rs:{line}:".encode() in error, error
        if message == b"E0308":
            assert b"main.rs:2:29" in error, error
        if message == b"positional arguments":
            assert error.count(b"rpp: original source") >= 2, error
        if mapping == b"macro defined at":
            assert b"frag.rph:1:" in error
            raw = run(bad_compile, base, 1).stderr
            assert b"original source" not in raw
            diagnostics = list(map(json.loads, raw.splitlines()))
            assert any(d.get("code", {}).get("code") == "E0425" for d in diagnostics if d.get("code"))
            direct = [RUSTC, f"--edition={edition}", "--crate-name=test_crate", "--crate-type=rlib",
                      "--cap-lints=allow", "--error-format=json", tree / "main.rs",
                      "--emit=link,dep-info=direct.d", "-o", "direct.rlib"]
            assert raw == run(direct, base, 1).stderr

    fragment.write_text('fn f(){ let _ = missing; }\n')
    entry.write_text('#include "frag.rph"\nfn main() {}\n')
    run(prep, base)
    error = run(compile_args(), base, 1).stderr
    assert b"original source" in error and b"preprocessing include at" in error and b"frag.rph:1:" in error
    fragment.write_text("#define TYPE &str\n")
    entry.write_text('#include "frag.rph"\nfn main(){ let _: TYPE = 42; }')
    run(prep, base)
    error = run(compile_args(), base, 1).stderr
    assert b"E0308" in error and b"original source" in error
    remapped = run(compile_args("--remap-path-prefix", f"{tree}=/virtual"), base, 1).stderr
    assert b"/virtual/main.rs" in remapped and b"original source" not in remapped
    fragment.write_text("#define NUM 5\n")
    entry.write_text('#include "frag.rph"\nfn main(){ let _: &str = NUM + 1; }')
    run(prep, base)
    ambiguous = run(compile_args(), base, 1).stderr
    assert b"E0308" in ambiguous and b"original source" not in ambiguous, ambiguous

    entry.write_text('fn main(){ let _: &str = String::from("s"); }')
    run(prep, base)
    raw = run(bad_compile, base, 1).stderr
    diagnostics = list(map(json.loads, raw.splitlines()))
    suggestions = [s["suggested_replacement"] for d in diagnostics for child in d.get("children", [])
                   for s in child.get("spans", []) if s.get("suggested_replacement")]
    assert suggestions
    mapped = run(compile_args(), base, 1).stderr
    assert all(d["rendered"].encode() in mapped for d in diagnostics if d.get("rendered"))
    assert b"original source" in mapped

    entry.write_text('macro_rules! bad {()=>{ missing }} fn main() { bad!(); }')
    run(prep, base)
    assert b"original source" not in run(compile_args(), base, 1).stderr

    ext = base / "ext.rs"
    ext.write_text('#[macro_export] macro_rules! read { ($file:expr) => { include_str!($file) }; }')
    extern = base / "libext.rlib"
    run([RUSTC, "--crate-type=rlib", "--crate-name=ext", ext, "-o", extern], base)
    forbidden = base / "outside.txt"
    forbidden.write_text("outside")
    entry.write_text(f'fn main() {{ let _ = ext::read!({json.dumps(str(forbidden))}); }}')
    run(prep, base)
    error = run(compile_args("--extern", f"ext={extern}"), base, 1).stderr
    assert b"unexpected rustc dependency" in error and b"outside.txt" in error, error
    symlink = base / "alias.txt"
    symlink.symlink_to(tree / asset.name)
    entry.write_text(f'fn main() {{ let _ = ext::read!({json.dumps(str(symlink))}); }}')
    run(prep, base)
    assert b"unexpected rustc dependency" in run(compile_args("--extern", f"ext={extern}"), base, 1).stderr
    assert (output.read_bytes(), dep.read_bytes(), program.read_bytes()) == previous

    wrapper = base / "rustc wrapper"
    wrapper.write_text(f"#!{sys.executable}\nimport os, sys\na=sys.argv[1:]\n"
                       "if '--crate-type=bin' in a: a += ['--cfg', 'm5_final']\n"
                       f"os.execv({str(Path(shutil.which(RUSTC)).resolve())!r}, [{str(Path(shutil.which(RUSTC)).resolve())!r}, *a])\n")
    wrapper.chmod(0o755)
    entry.write_text('fn main() {}\n#[cfg(m5_final)] fn broken() { let _ = missing; }\n')
    run(prep, base)
    run(compile_args(rustc=wrapper), base)
    error = run(link_args(rustc=wrapper), base, 1).stderr
    assert b"E0425" in error and b"original source" in error and b"main.rs:2:" in error
    assert program.read_bytes() == previous[2]
    entry.write_text('fn main() {}\n' + f'#[cfg(m5_final)] fn broken() {{ let _ = ext::read!({json.dumps(str(forbidden))}); }}')
    run(prep, base)
    run(compile_args("--extern", f"ext={extern}", rustc=wrapper), base)
    assert b"libext.rlib" in dep.read_bytes()
    assert b"unexpected rustc dependency" in run(link_args(rustc=wrapper), base, 1).stderr
    assert program.read_bytes() == previous[2]

    cpp = base / "main.cpp"
    cpp.write_text('extern "C" int answer(); int main(){ return answer() != 17; }')
    obj = base / "cpp.o"
    run(["c++", "-c", cpp, "-o", obj], base)
    entry.write_text('#define ANSWER 17\n#[unsafe(no_mangle)] pub extern "C" fn answer()->i32 { ANSWER }\n')
    run(prep, base)
    for flags in ((), ("--error-format=json",), ("--error-format=short", "--color=always")):
        run(compile_args(*flags, entrypoint="CXX"), base)
        run(link_args(obj), base)
        run([program], base)
    library = base / "libinstalled.a"
    run(compile_args(kind="staticlib"), base)
    run(link_args(destination=library), base)
    assert library.is_file()
    archived = base / "archived"
    tree.rename(archived)
    run([driver, "link", "-o", program, obj, "--libraries", library], base)
    run([program], base)


def ninja_cases(driver, base):
    (base / "source").mkdir()
    entry = base / "source/main.rs"
    entry.write_text('#if 0\ncomment1\n#endif\nfn main(){ let _ = ext::value() + 1; }\n')
    (base / "extern.rs").write_text("pub fn value() -> i32 { 1 }")
    log = base / "commands"
    helper = base / "trace.py"
    helper.write_text('import subprocess,sys\nwith open("commands","a") as f: print(sys.argv[1],file=f)\n'
                      'raise SystemExit(subprocess.call(sys.argv[2:]))\n')
    prep = [driver, "preprocess", "source/main.rs", "--source-root", "source",
            "--output-dir", "tree", "--depfile", "tree/.rpp/preprocess.d"]
    compile = [driver, "compile", "--rustc", RUSTC, "--rpp-snapshot", "tree", "--depfile",
               "compile.d", "--kind", "rlib", "--extern", "ext=libext.rlib", "-o", "app.rlib", "source/main.rs"]
    extern = [RUSTC, "--crate-type=rlib", "--crate-name=ext", "extern.rs", "-o", "libext.rlib"]
    def command(label, args):
        return shlex.join([sys.executable, str(helper), label, *map(str, args)]).replace("$", "$$")
    (base / "build.ninja").write_text(
        f"rule prep\n  command = {command('prep', prep)}\n  depfile = tree/.rpp/preprocess.d\n  restat = 1\n"
        f"rule compile\n  command = {command('compile', compile)}\n  depfile = compile.d\n  deps = gcc\n"
        f"rule extern\n  command = {command('extern', extern)}\n"
        "build tree/.rpp/complete tree/main.rs tree/.rpp/manifest tree/.rpp/source-map.json tree/.rpp/owned: prep source/main.rs\n"
        "build libext.rlib: extern extern.rs\n"
        "build app.rlib: compile tree/main.rs tree/.rpp/manifest tree/.rpp/source-map.json libext.rlib || tree/.rpp/complete\n"
        "default app.rlib\n"
    )
    def build(*expected):
        log.write_text("")
        run(["ninja"], base)
        assert sorted(log.read_text().splitlines()) == sorted(expected), log.read_text()
    build("prep", "extern", "compile")
    build()
    os.utime(entry)
    build("prep")
    build()
    entry.write_text(entry.read_text().replace("comment1", "comment2"))
    build("prep")
    build()
    entry.write_text(entry.read_text().replace(" + 1", " + 2"))
    build("prep", "compile")
    build()
    (base / "tree/main.rs").unlink()
    build("prep", "compile")
    build()
    (base / "extern.rs").write_text("pub fn value() -> i32 { 2 }")
    build("extern", "compile")
    build()


if __name__ == "__main__":
    driver = Path(sys.argv[1]).resolve()
    with tempfile.TemporaryDirectory(prefix="rpp-m5-") as tmp:
        root = Path(tmp)
        for edition in ("2021", "2024"):
            base = root / edition
            base.mkdir()
            driver_cases(driver, base, edition)
        base = root / "ninja"
        base.mkdir()
        ninja_cases(driver, base)
