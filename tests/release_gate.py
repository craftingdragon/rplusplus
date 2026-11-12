#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Offline v0.1 release gate. Supply preinstalled, pinned tool paths."""
import argparse
import json
from pathlib import Path
import subprocess
import sys
from generators import ROOT

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for role in ('minimum-cmake','current-cmake','minimum-rustc','previous-rustc','current-rustc'):
        parser.add_argument('--'+role,required=True,type=Path)
    parser.add_argument('--output',required=True,type=Path)
    opts=parser.parse_args()
    roles={k:v for k,v in vars(opts).items() if k!='output'}
    failures=[f'Missing preinstalled tool: {k}={v}' for k,v in roles.items() if not v.is_file()]
    if not failures:
        for role,path in roles.items():
            version=subprocess.check_output([str(path),'--version'],text=True).splitlines()[0]
            if role=='minimum_cmake' and not version.startswith('cmake version 3.25.0'):failures.append('Minimum CMake must be 3.25.0')
            if role=='minimum_rustc' and not version.startswith('rustc 1.85.0 '):failures.append('Minimum Rust must be 1.85.0')
            if 'rustc' in role and ('nightly' in version or 'beta' in version):failures.append(f'{role} must be stable')
    if failures:
        opts.output.parent.mkdir(parents=True,exist_ok=True)
        opts.output.write_text(json.dumps({'schema':1,'status':'blocked','blockers':failures},indent=2)+'\n')
        print('\n'.join(failures),file=sys.stderr);return 1
    cmd=[sys.executable,str(ROOT/'tests/compatibility_matrix.py'),'--require-qt','--output',str(opts.output)]
    for role,path in roles.items():cmd.extend(['--cmake' if 'cmake' in role else '--rustc',str(path)])
    status=subprocess.call(cmd)
    if status:return status
    report=json.loads(opts.output.read_text())
    report.update(status='blocked',blockers=['v0.1/depfile-loss: Unix Makefiles loses data-input dependencies after compiler depfile deletion'])
    opts.output.write_text(json.dumps(report,indent=2)+'\n')
    return 1
if __name__=='__main__':sys.exit(main())
