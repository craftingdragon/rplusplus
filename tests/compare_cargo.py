#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Compare identical mixed-language outputs using Rpp versus CMake + local Cargo."""
import argparse
from datetime import datetime, timezone
import json
import os
from pathlib import Path
import platform
import shutil
import statistics
import subprocess
import tempfile
from benchmark import ROOT, GENERATORS, run, wrapper, measure

SCENARIOS = ('clean','noop','rust_leaf','rust_root','cxx_leaf','post_noop')


def cargo_project(src, target, runtime):
    deps = {'base': [], 'side': ['base'], 'middle': ['base','side'],
            'bridge': ['middle'], 'app': ['middle']}
    (src/'Cargo.toml').write_text('[workspace]\nresolver="3"\nmembers=["base","side","middle","bridge","app"]\n'
                                '[profile.dev]\nopt-level=0\ndebug=2\ncodegen-units=256\n')
    for name, dependencies in deps.items():
        folder=src/name
        folder.mkdir()
        text=f'[package]\nname="{name}"\nversion="0.0.0"\nedition="{2021 if name=="base" else 2024}"\n'
        if name=='app':
            text+='[[bin]]\nname="RustMain"\npath="../app.rs"\n'
        else:
            text+=f'[lib]\npath="../{name}.rs"\ncrate-type=["{"staticlib" if name=="bridge" else "rlib"}"]\n'
        text+='[dependencies]\n'+''.join(f'{d}={{path="../{d}"}}\n' for d in dependencies)
        (folder/'Cargo.toml').write_text(text)
    app=src/'app.rs'
    app.write_text(app.read_text().replace('unsafe extern "C"',
        '#[link(name="worker", kind="static")]\nunsafe extern "C"'))
    # Always ask Cargo to evaluate its graph. BYPRODUCTS and target ordering
    # prevent premature native linking and unnecessary C++ relinks.
    (src/'CMakeLists.txt').write_text('''cmake_minimum_required(VERSION 3.25)
project(CargoGraph LANGUAGES CXX)
add_library(worker STATIC worker.cpp)
set_target_properties(worker PROPERTIES ARCHIVE_OUTPUT_DIRECTORY "${CMAKE_BINARY_DIR}" ARCHIVE_OUTPUT_DIRECTORY_DEBUG "${CMAKE_BINARY_DIR}")
add_custom_target(cargo_build ALL
  COMMAND "${CARGO_EXECUTABLE}" build --workspace --frozen --offline --jobs 4
  COMMAND "${CMAKE_COMMAND}" -E copy_if_different "${CARGO_TARGET_DIR}/debug/libbridge.a" "${CMAKE_BINARY_DIR}/libbridge.a"
  WORKING_DIRECTORY "${CMAKE_CURRENT_SOURCE_DIR}"
  BYPRODUCTS "${CMAKE_BINARY_DIR}/libbridge.a"
  VERBATIM)
add_dependencies(cargo_build worker)
add_library(bridge STATIC IMPORTED GLOBAL)
set_target_properties(bridge PROPERTIES IMPORTED_LOCATION "${CMAKE_BINARY_DIR}/libbridge.a")
add_library(adapter STATIC adapter.cpp)
add_executable(CppMain main.cpp)
add_dependencies(CppMain cargo_build)
target_link_libraries(CppMain PRIVATE adapter bridge ''' + ' '.join(runtime) + ''')
enable_testing()
add_test(NAME cpp-main COMMAND CppMain)
add_test(NAME rust-main COMMAND "${CARGO_TARGET_DIR}/debug/RustMain")
''')


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--cmake',default=shutil.which('cmake'))
    parser.add_argument('--rustc',required=True)
    parser.add_argument('--cargo',required=True)
    parser.add_argument('--repeats',type=int,default=5)
    parser.add_argument('--timing-only',action='store_true')
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    if args.repeats<1: parser.error('positive repeats required')
    cmake,rustc,cargo=map(lambda p:Path(p).absolute(),(args.cmake,args.rustc,args.cargo))
    cxx=Path(shutil.which('c++')).absolute()
    report={'schema':1,'timestamp_utc':datetime.now(timezone.utc).isoformat(),
            'platform':platform.platform(),'cmake':run([cmake,'--version']).splitlines()[0],
            'rustc':run([rustc,'--version']).strip(),'cargo':run([cargo,'--version']).strip(),
            'cxx':run([cxx,'--version']).splitlines()[0],
            'ninja':run(['ninja','--version']).strip(),'make':run(['make','--version']).splitlines()[0],
            'python':platform.python_version(),'logical_cpus':os.cpu_count(),
            'jobs':4,'config':'Debug','codegen_units':256,'instrumented':not args.timing_only,
            'method':'Warm caches; identical mixed outputs. CMake + Cargo always evaluates Cargo graph. Setup, clean and tests excluded. Python process wrappers only in instrumented run.',
            'samples':[]}
    with tempfile.TemporaryDirectory(prefix='rpp cargo comparison ') as tmp:
        base=Path(tmp)
        package,prefix=base/'package',base/'installed'
        run([cmake,'-S',ROOT,'-B',package,'-G','Ninja','-DBUILD_TESTING=OFF',f'-DRPP_RUSTC_EXECUTABLE={rustc}'])
        run([cmake,'--build',package])
        run([cmake,'--install',package,'--prefix',prefix])
        trace=base/'trace';trace.mkdir()
        tools=base/'tools';tools.mkdir()
        cc=wrapper(tools/'cxx',cxx,'cxx',trace)
        rc=wrapper(tools/'rustc',rustc,'rustc',trace)
        rp=wrapper(tools/'rpp',prefix/'bin/rpp','rpp',trace)
        cg=wrapper(tools/'cargo',cargo,'cargo',trace)
        if args.timing_only:cc,rc,rp,cg=cxx,rustc,prefix/'bin/rpp',cargo
        # Native runtime discovery is configuration work, using the same stable
        # compiler and std/panic profile; no extra Cargo compilation is required.
        probe=base/'probe.rs';probe.write_text('pub fn probe() {}\n')
        output=run([rustc,'--crate-type=staticlib','--edition=2024','--print=native-static-libs',
                    probe,'-o',base/'probe.a'])
        runtime=next(line.split('native-static-libs:',1)[1].split()
                     for line in output.splitlines() if 'native-static-libs:' in line)
        report['cargo_runtime_probe']=runtime
        for gen in GENERATORS:
            for system in ('Rpp','Cargo incremental','Cargo nonincremental'):
                home=base/gen/system
                src,build,target=home/'source',home/'build',home/'cargo-target'
                shutil.copytree(ROOT/'examples/multicrate',src)
                env={k:v for k,v in os.environ.items() if not k.startswith('CARGO_')}
                for key in ('RUSTC_WRAPPER','RUSTC_WORKSPACE_WRAPPER','RUSTFLAGS','CARGO_ENCODED_RUSTFLAGS'):
                    env.pop(key,None)
                env.update(CARGO_HOME=str(home/'cargo-home'),RUSTC=str(rc),
                           CARGO_TARGET_DIR=str(target),CARGO_INCREMENTAL='1' if system=='Cargo incremental' else '0')
                flags=['--cfg=driver_flag',f'-Clinker={cc}',f'-Lnative={build}']
                env['CARGO_ENCODED_RUSTFLAGS']='\x1f'.join(flags)
                options=[f'-DCMAKE_CXX_COMPILER={cc}']
                if system=='Rpp':
                    options += [f'-DRPLUSPLUS_MODULE_DIR={prefix}/share/rplusplus/cmake',
                                f'-DCMAKE_Rpp_COMPILER={rp}',f'-DRPP_RUSTC_EXECUTABLE={rc}',
                                '-DCMAKE_Rpp_FLAGS=-Ccodegen-units=256']
                else:
                    cargo_project(src,target,runtime)
                    run([cargo,'generate-lockfile','--offline','--manifest-path',src/'Cargo.toml'],env)
                    options += [f'-DCARGO_EXECUTABLE={cg}',f'-DCARGO_TARGET_DIR={target}']
                run([cmake,'-S',src,'-B',build,'-G',gen,'-DCMAKE_BUILD_TYPE=Debug',*options],env)
                originals={name:(src/name).read_text() for name in ('base.rs','app.rs','main.cpp')}
                command=[cmake,'--build',build,'--config','Debug','--parallel','4']
                for repeat in range(args.repeats):
                    for name,content in originals.items():(src/name).write_text(content)
                    run(command+['--target','clean'],env)
                    if system!='Rpp':run([cargo,'clean','--manifest-path',src/'Cargo.toml'],env)
                    for scenario in SCENARIOS:
                        if scenario=='rust_leaf':(src/'base.rs').write_text(originals['base.rs']+'\npub fn baseline_marker() {}\n')
                        elif scenario=='rust_root':(src/'app.rs').write_text(originals['app.rs']+'\n// root edit\n')
                        elif scenario=='cxx_leaf':(src/'main.cpp').write_text(originals['main.cpp']+'\n// C++ edit\n')
                        sample=measure(command,trace,env)
                        if args.timing_only:sample['invocations']=None
                        elif scenario in ('noop','post_noop'):
                            allowed=('cargo.build','rustc.query') if system!='Rpp' else ()
                            assert all(k in allowed for k in sample['invocations']),sample
                        sample.update(generator=gen,system=system,repeat=repeat,scenario=scenario)
                        report['samples'].append(sample)
                    if system!='Rpp':
                        sample=measure([cg,'build','--workspace','--frozen','--offline','--jobs','4',
                                        '--manifest-path',src/'Cargo.toml'],trace,env)
                        if args.timing_only:sample['invocations']=None
                        else:assert all(k in ('cargo.build','rustc.query') for k in sample['invocations']),sample
                        sample.update(generator=gen,system=system,repeat=repeat,scenario='cargo_only_noop')
                        report['samples'].append(sample)
                    run([cmake.with_name('ctest'),'--test-dir',build,'-C','Debug','--output-on-failure'],env)
                print(f'{gen}: {system} measured ({args.repeats} repetitions)',flush=True)
    report['summary']=[]
    for gen in GENERATORS:
        for system in ('Rpp','Cargo incremental','Cargo nonincremental'):
            for scenario in SCENARIOS + (() if system=='Rpp' else ('cargo_only_noop',)):
                rows=[s for s in report['samples'] if (s['generator'],s['system'],s['scenario'])==(gen,system,scenario)]
                counts=[s['invocations'] for s in rows]
                report['summary'].append({'generator':gen,'system':system,'scenario':scenario,
                    'median_seconds':statistics.median(s['seconds'] for s in rows),
                    'min_seconds':min(s['seconds'] for s in rows),'max_seconds':max(s['seconds'] for s in rows),
                    'invocations':counts[0] if all(c==counts[0] for c in counts) else counts})
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text(json.dumps(report,indent=2)+'\n')
    print(f'Wrote {args.output}',flush=True)


if __name__=='__main__':main()
