#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""v0.1/publication: failed tools cannot replace successful outputs."""
from pathlib import Path
import subprocess
import sys
import tempfile
import os
import signal
import time
import shutil

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
            command=[*args,'-o',str(output)]
            response=root/'response args'
            for mode in ('direct','response','mixed'):
                if mode=='response':
                    response.write_bytes(('\0'.join(command)+'\0').encode())
                    invocation=['@'+str(response)]
                elif mode=='mixed':
                    response.write_bytes(('\0'.join(command[1:])+'\0').encode())
                    invocation=[command[0],'@'+str(response)]
                else:invocation=command
                p=subprocess.run([str(driver),*invocation],capture_output=True,text=True,timeout=30)
                assert p.returncode==17,(p.returncode,p.stderr)
                assert output.read_bytes()==b'previous valid artifact'
                assert not list(root.glob('*.rpp-stage*')),list(root.iterdir())
        # Linux release platform: terminate the entire tool process group at a
        # deterministic barrier after partial output exists, before publication.
        ready=root/'ready'
        fake.write_text(f'#!{sys.executable}\nimport pathlib,sys,time\na=sys.argv[1:]\nout=a[a.index("-o")+1] if "-o" in a else a[1]\npathlib.Path(out).write_bytes(b"partial")\npathlib.Path({str(ready)!r}).touch()\nwhile True: time.sleep(0.01)\n')
        rustc=os.environ.get('RPP_TEST_RUSTC',shutil.which('rustc'))
        source=root/'source.rs';source.write_text('pub fn value()->i32 {42}\n')
        cpp=root/'main.cpp';cpp.write_text('int main(){return 0;}\n')
        for op in ('compile','link','archive'):
            output.write_bytes(b'previous valid artifact')
            ready.unlink(missing_ok=True)
            if op=='compile':
                args=['compile','--rustc',str(fake),'--depfile',str(root/'deps.d'),str(source)]
                retry=['compile','--rustc',rustc,'--depfile',str(root/'deps.d'),str(source)]
            elif op=='link':
                args=['link','--cxx',str(fake),'--libraries']
                retry=['link','--cxx',shutil.which('c++'), '--libraries',str(cpp)]
            else:
                args=['link','--ar',str(fake),'--rpp-archive','--libraries']
                retry=['link','--ar',shutil.which('ar'),'--rpp-archive','--libraries']
            process=subprocess.Popen([str(driver),*args,'-o',str(output)],start_new_session=True)
            try:
                deadline=time.monotonic()+15
                while not ready.exists():
                    if process.poll() is not None or time.monotonic()>deadline:raise RuntimeError('Interruption barrier failed')
                    time.sleep(0.01)
                os.killpg(process.pid,signal.SIGTERM)
                process.wait(timeout=10)
            finally:
                if process.poll() is None:
                    os.killpg(process.pid,signal.SIGKILL);process.wait()
            assert output.read_bytes()==b'previous valid artifact',op
            subprocess.run([str(driver),*retry,'-o',str(output)],check=True,timeout=30)
            assert output.read_bytes()!=b'previous valid artifact',op
            assert not list(root.glob('*.rpp-stage*'))
        print('PASS v0.1/publication: compile, linker and archiver failures preserve output and status')
if __name__=='__main__':main()
