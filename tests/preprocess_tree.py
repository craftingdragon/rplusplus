#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Exercise tree generation, isolation, Rust resolution and snapshot recovery."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
from generators import RUSTC


def main(driver, base):
    root = base / "original 🦀"
    out = root / "build tree"
    sources = {
        "src/main.rs": ('#include "defs.rph"\nmod outer;\n'
                        '#[path = "../shared.rs"] mod shared;\n'
                        'mod inline { #[path = "alternate.rs"] pub mod alternate; }\n'
                        '#[cfg_attr(any(), path = "other.rs")] mod dormant;\n'
                        'include!(r#"chunks/code.rs"#,);\n'
                        'fn main() { let x = ROOT; assert_eq!(x + outer::value() + shared::N'
                        ' + inline::alternate::N + side(), 29);'
                        ' assert_eq!(include_bytes!("../data/bytes.bin"), b"\\x00\\xff");'
                        ' assert_eq!(include_str!("../data/Gr\\u{fc}ezi.txt"), "hé"); }\n'),
        "src/outer.rs": ('#ifdef ROOT\n#error macro leaked from entry\n#endif\n'
                         'mod nested; pub fn value() -> i32 { nested::N }\n'),
        "src/outer/nested.rs": "pub const N: i32 = 5;\n",
        "shared.rs": "pub const N: i32 = 4;\n",
        "src/inline/alternate.rs": "pub const N: i32 = 6;\n",
        "src/dormant.rs": "",
        "src/chunks/code.rs": 'include!("inner.rs"); fn side() -> i32 { let x = CHUNK; x }\n',
        "src/chunks/inner.rs": "#define PRIVATE 7\nconst CHUNK: i32 = PRIVATE;\n",
    }
    assets = {"data/bytes.bin": b"\x00\xff", "data/Grüezi.txt": "hé".encode()}
    fragments = {"src/defs.rph": "#define ROOT 7\n", "unused.rph": "#error inactive\n"}
    for name, content in {**sources, **assets, **fragments}.items():
        p = root / name
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_bytes(content if isinstance(content, bytes) else content.encode())

    standard = ["preprocess", "--source-root", root, "--output-dir", out, root / "src/main.rs"]
    declarations = [arg for kind, names in (("--source", list(sources)[1:]), ("--data", assets), ("--input", fragments))
                    for name in names for arg in (kind, root / name)]
    dep = out / ".rpp/tree.d"
    command = [*standard, *declarations, "--depfile", dep, "-o", out / "src/main.rs"]

    def run(args, code=0):
        result = subprocess.run([driver, *map(str, args)], cwd=base, capture_output=True, timeout=30)
        assert result.returncode == code, (args, result)
        return result

    def files():
        return {str(p.relative_to(out)): (p.read_bytes(), p.stat().st_mtime_ns)
                for p in out.rglob("*") if p.is_file()}

    def verify():
        state = files()
        data = state[".rpp/manifest"][0]
        assert state[".rpp/complete"][0].strip().decode() == hashlib.sha256(data).hexdigest()
        magic, entry, edition, mapping, dep_name, *fields = data.decode().split("\0")[:-1]
        assert (magic, entry, dep_name) == ("rpp-tree-v2", "src/main.rs", ".rpp/tree.d")
        assert edition in ("2021", "2024") and mapping == ".rpp/source-map.json"
        assert list(fields[::2]) == sorted([*sources, *assets, ".rpp/source-map.json"], key=lambda p: Path(p).parts)
        assert all(hashlib.sha256(state[p][0]).hexdigest() == h for p, h in zip(fields[::2], fields[1::2]))
        assert {v["generated"] for v in json.loads(state[".rpp/source-map.json"][0])["sources"]} == set(sources)
        assert "defs.rph" in state[dep_name][0].decode() and "unused.rph" not in state[dep_name][0].decode()
        assert state["data/bytes.bin"][0] == assets["data/bytes.bin"]
        return state

    for edition in ("2021", "2024"):
        run([*command, "--edition", edition])
        before = verify()
        duplicate = base / f"independent {edition}"
        duplicate.mkdir()
        for name in [*sources, *assets, *fragments]:
            dest = duplicate / name
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_bytes((root / name).read_bytes())
        duplicate_out = duplicate / out.name
        duplicate_args = [
            duplicate / item.relative_to(root) if isinstance(item, Path) else item
            for item in command
        ]
        run([*duplicate_args, "--edition", edition])
        for name, (content, _) in before.items():
            if name != ".rpp/tree.d":
                assert (duplicate_out / name).read_bytes() == content, name
        response = base / "tree args"
        response.write_bytes(("\0".join(map(str, [*command, "--edition", edition])) + "\0").encode())
        run(["@" + str(response)])
        assert files() == before
        mirror = base / f"mirror {edition}"
        out.rename(mirror)
        temporary = base / "unavailable-original"
        root.rename(temporary)
        assert not root.exists()
        exe = base / "app"
        subprocess.run([RUSTC, "--edition", edition, mirror / "src/main.rs", "-o", exe], check=True)
        subprocess.run([exe], check=True)
        temporary.rename(root)
        mirror.rename(out)

    entry = root / "src/main.rs"
    old = entry.read_bytes()
    before = files()
    for text, message in [
        ('include_str!(concat!("a", "b"));', "literal"),
        ('include!("missing.rs");', "--source"),
        ('include_str!("outer.rs");', "--data"),
        ('include!("../data/bytes.bin");', "--source"),
        ('include_bytes!("../../escape");', "escapes"),
        ('#[path="/tmp/abs.rs"] mod abs;', "inside the mirror"),
        ('#[path=concat!("a", "b")] mod abs;', "literal"),
        ('mod inline { #[path="../../foo.rs"] mod x; }', "inside the mirror"),
        ('#include "outer.rs"', "--input"),
    ]:
        entry.write_bytes(("// 🦀\r\n" + text + "\r\n").encode())
        error = json.loads(run([*command, "--diagnostic-format", "json"], 1).stderr)
        assert message in error["message"] and error["location"]["line"] == 2, error
        assert files() == before

    data_rs = root / "asset.rs"
    data_rs.write_text("")
    for text in ('#[cfg(any())] mod asset;', 'mod inline { #[path="asset.rs"] mod renamed; }',
                 '#[cfg_attr(any(), path="../asset.rs")] mod renamed;'):
        entry.write_text(text)
        assert b"--data" in run([*command, "--data", data_rs], 1).stderr
        assert files() == before
    entry.write_bytes(old)
    outside = base / "escaped.rs"
    outside.write_text("")
    symlink = root / "escape.rs"
    symlink.symlink_to(outside)
    for extra in (["--source", symlink], ["--data", root / "src/outer.rs"],
                  ["--input", root / "missing"], ["--include-dir", out],
                  ["--data", out / "data/bytes.bin"]):
        run([*command, *extra], 1)
        assert files() == before
    run(["preprocess", "--output-dir", out, entry], 2)
    run([*standard, "-o", base / "wrong.rs"], 1)
    for output in (root, base):
        run(["preprocess", "--source-root", root, "--output-dir", output, entry], 1)

    unknown = out / "unowned.rs"
    unknown.write_text("untouched")
    assert b"unowned" in run(command, 1).stderr
    assert unknown.read_text() == "untouched"
    unknown.unlink()
    for name in (".rpp/complete", ".rpp/manifest", ".rpp/owned", ".rpp/tree.d",
                 ".rpp/source-map.json", "src/outer/nested.rs", "data/bytes.bin"):
        (out / name).unlink()
        mark = (out / ".rpp/complete").stat().st_mtime_ns if name == ".rpp/tree.d" else None
        run(command)
        if mark is not None:
            assert (out / ".rpp/complete").stat().st_mtime_ns == mark
        assert {k: v[0] for k, v in verify().items()} == {k: v[0] for k, v in before.items()}
    (out / "src/outer.rs").write_bytes(b"tampered")
    run(command)
    verify()

    stale = root / "stale" / "payload.bin"
    stale.parent.mkdir()
    stale.write_bytes(b"stale")
    run([*command, "--data", stale])
    assert (out / "stale/payload.bin").read_bytes() == b"stale"
    run(command)
    assert not (out / "stale").exists()
    verify()


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="rpp M4 ") as tmp:
        main(Path(sys.argv[1]).resolve(), Path(tmp))
