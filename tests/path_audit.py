#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Offline release path audit, including explicit stock-CMake restrictions."""
import argparse
import json
from pathlib import Path
import shutil
import subprocess
import tempfile
from generators import ROOT

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cmake',action='append',required=True)
    parser.add_argument('--rustc',required=True)
    parser.add_argument('--driver',required=True)
    parser.add_argument('--output',required=True,type=Path)
    args=parser.parse_args();report=[]
    with tempfile.TemporaryDirectory(prefix='rpp path audit ') as tmp:
        base=Path(tmp)
        for cmake in args.cmake:
            version=subprocess.check_output([cmake,'--version'],text=True).splitlines()[0]
            for gen in ('Ninja','Unix Makefiles','Ninja Multi-Config'):
                for label in ('spaces 🦀',"apostrophe'",'dollar$','hash#','quote"'):
                    folder=base/str(len(report))/label;source=folder/'source';build=folder/'build'
                    shutil.copytree(ROOT/'examples/native',source)
                    entry={'cmake':version,'generator':gen,'path':label}
                    command=[cmake,'-S',str(source),'-B',str(build),'-G',gen,
                             f'-DRPLUSPLUS_MODULE_DIR={ROOT}/cmake',f'-DCMAKE_Rpp_COMPILER={args.driver}',f'-DRPP_RUSTC_EXECUTABLE={args.rustc}']
                    steps=[('configure',command),('build',[cmake,'--build',str(build),'--config','Debug','--parallel','4']),
                           ('execute',[str(build/('Debug/MyApp' if gen=='Ninja Multi-Config' else 'MyApp'))])]
                    for step,command in steps:
                        result=subprocess.run(command,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=120)
                        if result.returncode:
                            expected=step=='configure' and ((label=='quote"' and 'Parse error' in result.stdout)
                                or (label=='hash#' and version=='cmake version 3.25.0' and 'containing a "#" is not allowed' in result.stdout))
                            entry.update(status='expected-unsupported' if expected else 'failed',step=step,log=result.stdout);break
                    else:entry['status']='passed'
                    report.append(entry)
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text(json.dumps({'schema':1,'results':report},indent=2)+'\n')
    if any(r['status']=='failed' for r in report):raise SystemExit(1)
if __name__=='__main__':main()
