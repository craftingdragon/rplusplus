#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Exercise M3 through the built driver and real rustc and Make."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

from generators import RUSTC


def main(driver, work):
    source = work / "tree 🦀"
    source.mkdir()
    inc = source / "search"
    inc.mkdir()
    root = source / "-root.rs"
    root.write_text(
        '#if FLAG == 3 && defined ENABLE\n#include "frag #$\'.rph"\n'
        '#else\n#include "missing"\n#endif\n'
        'mod inline { pub fn v() -> i32 { 1 } }\n'
        'fn main() { assert_eq!(RESULT + inline::v(), 8); }\n'
    )
    fragment = inc / "frag #$'.rph"
    fragment.write_text('#include "nested.rph"\nconst RESULT: i32 = VALUE;\n')
    nested = inc / "nested.rph"
    nested.write_text("#define VALUE 7\n")
    unused = inc / "unused.rph"
    unused.write_text("#error unused\n")
    output, dep, source_map = map(Path, ("generated #$'🦀.rs", "output.d", "output.json"))
    files = [work / p for p in (output, dep, source_map)]
    options = [
        "-o", output, "--depfile", dep, "--source-map", source_map,
        "--source-root", source, "--include-dir", inc, "-I" + str(source),
        "--input", fragment, "--input", nested, "--input", unused,
        "-DFLAG=2", "-UFLAG", "--define", "FLAG=3", "-D", "ENABLE",
        "--", root.relative_to(work),
    ]

    def command(*args, status=0):
        result = subprocess.run([str(driver), *map(str, args)], cwd=work, capture_output=True, timeout=30)
        assert result.returncode == status, (args, result)
        return result

    def state():
        return [(p.read_bytes(), p.stat().st_mtime_ns) for p in files]

    assert b"preprocess" in command("--help").stdout
    assert b"--source-map" in command("preprocess", "--help").stdout
    for edition in ("2021", "2024"):
        flags = ["--edition", edition, *options]
        command("preprocess", *flags)
        before = state()
        response = work / "response args"
        response.write_bytes(("\0".join(map(str, ["preprocess", *flags])) + "\0").encode())
        command("@" + str(response))
        response.write_bytes(("\0".join(map(str, flags)) + "\0").encode())
        command("preprocess", "@" + str(response))
        assert state() == before
        subprocess.run([RUSTC, "--edition", edition, "--crate-name", "m3_probe", str(output), "-o", "app"], cwd=work, check=True)
        subprocess.run([str(work / "app")], cwd=work, check=True)
    mapping = json.loads(files[2].read_text())
    assert mapping["version"] == 1 and mapping["generated"] == str(output)
    offset = 0
    for item in mapping["mappings"]:
        assert item["generated"]["start"] == offset
        offset = item["generated"]["end"]
    assert offset == len(files[0].read_bytes())
    assert any(m["definitions"] and m["include_stack"] for m in mapping["mappings"])
    assert any(m["original"]["source"].get("path") == "search/" + fragment.name for m in mapping["mappings"])
    assert "unused" not in files[1].read_text()
    (work / "Makefile").write_text(files[1].read_text() + "\t@false\n")
    for expected in (0, 1):
        if expected:
            os.utime(nested, ns=(files[0].stat().st_mtime_ns + 2_000_000_000,) * 2)
        result = subprocess.run(["make", "-q", "-f", "Makefile", str(output)], cwd=work, capture_output=True)
        assert result.returncode == expected, result

    before = state()
    old = fragment.read_bytes()
    fragment.write_bytes('// 🦀\r\n  #error quotes: "x" and \\ slash\r\n'.encode())
    error = json.loads(command("preprocess", "--diagnostic-format", "json", *options, status=1).stderr)
    assert error["kind"] == "ExplicitError" and '"x" and \\ slash' in error["message"]
    assert (error["location"]["line"], error["location"]["column"]) == (2, 4)
    assert error["include_stack"][0]["source"]["path"] == root.name
    assert state() == before
    fragment.write_bytes(old)
    for token in ('mod r#module;', '#[doc=include_str!("text")] fn f() {}',
                  'fn f() { assert_eq!(include_bytes!(concat!("a", "b")), b""); }',
                  'fn f() { let _ = include!("code"); }'):
        root_text = root.read_bytes()
        root.write_text(f"// 🦀\r\n{token}\r\n")
        failure = command("preprocess", "--diagnostic-format", "json", *options, status=1)
        error = json.loads(failure.stderr)
        assert error["location"]["line"] == 2 and "source-tree" in error["message"]
        assert state() == before
        root.write_bytes(root_text)

    outside = work / "outside.rph"
    outside.write_text("")
    link = source / "link.rph"
    link.symlink_to(outside)
    for extra in [("--input", outside), ("--input", link), ("--input", root),
                  ("--input", source / "missing"), ("--include-dir", work)]:
        assert command("preprocess", *extra, *options, status=1).stderr
        assert state() == before
    for extra in [("--edition", "2018"), ("--source", "module.rs"),
                  ("--output-dir", "dir"), ("-D", "bad-name"), ("--unknown",)]:
        command("preprocess", *extra, *options, status=2)
        assert state() == before
    command("preprocess", root, "-o", status=2)
    command("preprocess", "@missing-response", status=1)
    link.unlink()
    link.symlink_to(root)
    for target in (root, link, dep, "missing-parent/generated.rs"):
        flags = [*options]
        flags[1] = target
        command("preprocess", *flags, status=1)
        assert state() == before
    command("preprocess", *options)
    assert state() == before
    assert not list(work.rglob(".rpp-preprocess-*.tmp"))
    plain = work / "-plain.rs"
    plain.write_bytes(b"\xef\xbb\xbf#!/not-executed\r\nfn main() {}\r\n")
    command("preprocess", "-o", "plain.rs", "--", plain.name)
    assert (work / "plain.rs").read_bytes() == plain.read_bytes()


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="rpp M3 ") as tmp:
        main(Path(sys.argv[1]).resolve(), Path(tmp))
