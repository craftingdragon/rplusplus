#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Create a deterministic local candidate archive. Never tags or publishes."""
import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path
import re
import subprocess
from generators import ROOT

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',required=True,type=Path)
    args=parser.parse_args()
    if subprocess.check_output(['git','status','--porcelain'],cwd=ROOT):
        raise SystemExit('Commit the candidate before creating a source bundle')
    revision=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip()
    subprocess.run(['git', 'verify-commit', revision], cwd=ROOT, check=True)
    version = re.search(r'^project\(Rplusplus VERSION (\d+\.\d+\.\d+) LANGUAGES NONE\)$',
                        (ROOT/'CMakeLists.txt').read_text(), re.M)
    if not version:
        raise SystemExit('Cannot determine the release version from CMakeLists.txt')
    version = version[1]
    tar=subprocess.check_output(['git','archive','--format=tar',f'--prefix=rplusplus-{version}/',revision],cwd=ROOT)
    def compress():
        stream=io.BytesIO()
        with gzip.GzipFile(filename='',fileobj=stream,mode='wb',mtime=0) as output:output.write(tar)
        return stream.getvalue()
    data=compress();assert data==compress()
    args.output.mkdir(parents=True,exist_ok=True)
    name=f'rplusplus-{version}.tar.gz'
    (args.output/name).write_bytes(data)
    checksum=hashlib.sha256(data).hexdigest()
    (args.output/'SHA256SUMS').write_text(f'{checksum}  {name}\n')
    recipe=(ROOT/'packaging/PKGBUILD.in').read_text().replace('@VERSION@',version).replace('@SOURCE_SHA256@',checksum)
    (args.output/'PKGBUILD').write_text(recipe)
    (args.output/'manifest.json').write_text(json.dumps({'schema':1,'version':version,'revision':revision,'source_archive':name,'source_sha256':checksum,'status':'candidate','release_ready':False},indent=2)+'\n')
if __name__=='__main__':main()
