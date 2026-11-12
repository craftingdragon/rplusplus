#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Create a deterministic local candidate archive. Never tags or publishes."""
import argparse
import gzip
import hashlib
import io
import json
from pathlib import Path
import subprocess
from generators import ROOT

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',required=True,type=Path)
    args=parser.parse_args()
    if subprocess.check_output(['git','status','--porcelain'],cwd=ROOT):
        raise SystemExit('Commit the candidate before creating a source bundle')
    revision=subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT,text=True).strip()
    tar=subprocess.check_output(['git','archive','--format=tar','--prefix=rplusplus-0.1.0/',revision],cwd=ROOT)
    def compress():
        stream=io.BytesIO()
        with gzip.GzipFile(filename='',fileobj=stream,mode='wb',mtime=0) as output:output.write(tar)
        return stream.getvalue()
    data=compress();assert data==compress()
    args.output.mkdir(parents=True,exist_ok=True)
    name='rplusplus-0.1.0-candidate.tar.gz'
    (args.output/name).write_bytes(data)
    checksum=hashlib.sha256(data).hexdigest()
    (args.output/'SHA256SUMS').write_text(f'{checksum}  {name}\n')
    recipe=(ROOT/'packaging/PKGBUILD.in').read_text().replace('@SOURCE_SHA256@',checksum).replace('rplusplus-${pkgver}.tar.gz','rplusplus-${pkgver}-candidate.tar.gz')
    (args.output/'PKGBUILD').write_text(recipe)
    (args.output/'manifest.json').write_text(json.dumps({'schema':1,'revision':revision,'source_sha256':checksum,'status':'candidate','release_ready':False},indent=2)+'\n')
if __name__=='__main__':main()
