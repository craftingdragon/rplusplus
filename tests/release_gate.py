#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Offline v0.1 release gate. Supply preinstalled, pinned tool paths."""
import argparse
import json
from pathlib import Path
import subprocess
import sys
import re
from generators import ROOT

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    for role in ('minimum-cmake','current-cmake','minimum-rustc','previous-rustc','current-rustc'):
        parser.add_argument('--'+role,required=True,type=Path)
    parser.add_argument('--output',required=True,type=Path)
    opts=parser.parse_args()
    roles={k:v for k,v in vars(opts).items() if k!='output'}
    failures=[f'Missing preinstalled tool: {k}={v}' for k,v in roles.items() if not v.is_file()]
    expected={'minimum_cmake':'3.25.0','current_cmake':'4.4.5',
              'minimum_rustc':'1.85.0','previous_rustc':'1.99.1','current_rustc':'1.100.0'}
    inventory={}
    for role,path in roles.items():
        try:
            version=subprocess.check_output([str(path),'--version'],text=True,stderr=subprocess.STDOUT,timeout=15).splitlines()[0]
            match=re.search(r'(?:rustc|cmake version) ([0-9]+\.[0-9]+\.[0-9]+)(?: |$)',version)
            if not match or match[1]!=expected[role] or 'nightly' in version or 'beta' in version:
                failures.append(f'{role}: expected stable {expected[role]}, found {version}')
            inventory[role]={'path':str(path.absolute()),'version':version}
            if 'cmake' in role:
                ctest=path.with_name('ctest')
                ctest_version=subprocess.check_output([str(ctest),'--version'],text=True,timeout=15).splitlines()[0]
                if ctest_version!='ctest version '+expected[role]:failures.append(f'{role}: matching CTest required')
            else:inventory[role]['identity']=subprocess.check_output([str(path),'-vV'],text=True,timeout=15)
        except (OSError,subprocess.SubprocessError) as error:failures.append(f'{role}: {error}')
    if failures:
        opts.output.parent.mkdir(parents=True,exist_ok=True)
        opts.output.write_text(json.dumps({'schema':1,'status':'blocked','blockers':failures,'inventory':inventory},indent=2)+'\n')
        print('\n'.join(failures),file=sys.stderr);return 1
    cmd=[sys.executable,str(ROOT/'tests/compatibility_matrix.py'),'--require-qt','--output',str(opts.output)]
    for role,path in roles.items():cmd.extend(['--cmake' if 'cmake' in role else '--rustc',str(path)])
    status=subprocess.call(cmd)
    if opts.output.exists():
        report=json.loads(opts.output.read_text())
        report['inventory']=inventory
        report['status']='passed' if status==0 else ('failed' if any(r['status']=='failed' for r in report.get('results',[])) else 'blocked')
        opts.output.write_text(json.dumps(report,indent=2)+'\n')
    return status
if __name__=='__main__':sys.exit(main())
