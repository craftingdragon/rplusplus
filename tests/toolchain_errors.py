#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Actionable errors for invalid bootstrap and consumer toolchains."""
import subprocess
import tempfile
from pathlib import Path
from generators import ROOT,CMAKE,RUSTC,run

def main():
    with tempfile.TemporaryDirectory(prefix='rpp toolchain errors ') as tmp:
        base=Path(tmp)
        def rejected(compiler, phrase, name):
            p=subprocess.run([CMAKE,'-S',str(ROOT),'-B',str(base/name),'-G','Ninja',
                              f'-DRPP_RUSTC_EXECUTABLE={compiler}'],text=True,
                             stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=60)
            assert p.returncode and phrase in p.stdout,p.stdout
        rejected(base/'absent rustc','Cannot run selected rustc','missing')
        fake=base/'old rustc'
        fake.write_text('#!/usr/bin/env python3\nprint("release: 1.84.0")\n')
        fake.chmod(0o755)
        rejected(fake,'requires stable Rust >=1.85','old')
        fake.write_text('#!/usr/bin/env python3\nimport sys\nprint("release: 1.85.0" if "-vV" in sys.argv else "/nonexistent-rpp-sysroot")\n')
        rejected(fake,'no usable standard library sysroot','sysroot')
        print('PASS v0.1/toolchains: missing compiler, unsupported release, missing sysroot')
if __name__=='__main__':main()
