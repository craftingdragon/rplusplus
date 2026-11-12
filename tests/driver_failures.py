#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""v0.1/publication: failed tools cannot replace successful outputs."""
from pathlib import Path
import subprocess
import sys
import tempfile

def main():
    driver=Path(sys.argv[1]).absolute()
    with tempfile.TemporaryDirectory(prefix='rpp failed publication ') as tmp:
        root=Path(tmp);output=root/'successful output';output.write_bytes(b'previous valid artifact')
        fake=root/'failing tool'
        fake.write_text(f'#!{sys.executable}\nimport pathlib,sys\na=sys.argv[1:]\nif "-o" in a:pathlib.Path(a[a.index("-o")+1]).write_bytes(b"partial output")\nraise SystemExit(17)\n')
        fake.chmod(0o755)
        for args in (
            ['compile','--rustc',str(fake),'--depfile',str(root/'deps.d'),'source.rs'],
            ['link','--cxx',str(fake),'--libraries'],
            ['link','--ar',str(fake),'--rpp-archive','--libraries'],
        ):
            p=subprocess.run([str(driver),*args,'-o',str(output)],capture_output=True,text=True,timeout=30)
            assert p.returncode==17,(p.returncode,p.stderr)
            assert output.read_bytes()==b'previous valid artifact'
            assert not list(root.glob('*.rpp-stage*')),list(root.iterdir())
        print('PASS v0.1/publication: compile, linker and archiver failures preserve output and status')
if __name__=='__main__':main()
