#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Offline Qt/SQLite and isolated installed-consumer regression gate."""
import os
from pathlib import Path
import shutil
import tempfile
from generators import ROOT, run, CMAKE, RUSTC

def main():
    with tempfile.TemporaryDirectory(prefix='rpp qt validation ') as tmp:
        base=Path(tmp)
        sentinel=base/'forbidden'; sentinel.mkdir()
        (sentinel/'cargo').write_text('#!/usr/bin/env python3\nraise SystemExit("Cargo forbidden")\n')
        (sentinel/'cargo').chmod(0o755)
        os.environ['PATH']=str(sentinel)+os.pathsep+os.environ['PATH']
        tools=base/'tools'; prefix=base/'prefix'
        run('cmake','-S',ROOT,'-B',tools,'-G','Ninja','-DBUILD_TESTING=OFF',f'-DRPP_RUSTC_EXECUTABLE={RUSTC}')
        run('cmake','--build',tools)
        run('cmake','--install',tools,'--prefix',prefix)
        moved=base/'relocated tools'; prefix.rename(moved);prefix=moved
        for gen in ('Ninja','Unix Makefiles','Ninja Multi-Config'):
            source=base/gen/'source';shutil.copytree(ROOT/'examples/qt-notes',source)
            consumer=base/gen/'consumer';shutil.copytree(source/'consumer',consumer)
            build=base/gen/'build'
            args=[f'-DRPLUSPLUS_MODULE_DIR={prefix}/share/rplusplus/cmake',f'-DRPP_RUSTC_EXECUTABLE={RUSTC}']
            run('cmake','-S',source,'-B',build,'-G',gen,'-DCMAKE_BUILD_TYPE=Debug',*args)
            for config in ('Debug','Release'):
                if gen!='Ninja Multi-Config':run('cmake','-S',source,'-B',build,f'-DCMAKE_BUILD_TYPE={config}')
                run('cmake','--build',build,'--config',config,'--parallel','4')
                run('ctest','--test-dir',build,'-C',config,'--output-on-failure')
            installed=base/gen/'installed'
            run('cmake','--install',build,'--config','Release','--prefix',installed)
            moved=base/gen/'moved package';installed.rename(moved)
            shutil.rmtree(build);shutil.rmtree(source)
            external=base/gen/'external'
            run('cmake','-S',consumer,'-B',external,'-G',gen,'-DCMAKE_BUILD_TYPE=Release',
                f'-DCMAKE_PREFIX_PATH={prefix};{moved}',f'-DRPP_RUSTC_EXECUTABLE={RUSTC}')
            run('cmake','--build',external,'--config','Release','--parallel','4')
            run('ctest','--test-dir',external,'-C','Release','--output-on-failure')
            print(f'PASS {gen}: Qt Widgets, SQLite, relocated isolated consumer',flush=True)
if __name__=='__main__':main()
