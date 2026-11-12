# SPDX-License-Identifier: GPL-3.0-or-later
"""Permanent v0.1/depfile-loss regression, called by the installed-module gate."""
from pathlib import Path
import shutil
import subprocess
from benchmark import wrapper

def verify(base, modules, driver, rustc, run):
    trace=base/'depfile trace';trace.mkdir()
    tools=base/'depfile tools';tools.mkdir()
    rpp=wrapper(tools/'rpp', Path(driver), 'rpp', trace)
    cxx=wrapper(tools/'cxx', Path(shutil.which('c++')).resolve(), 'cxx', trace)
    for gen in ('Ninja','Unix Makefiles','Ninja Multi-Config'):
        source=base/gen/'depfile source';source.mkdir(parents=True)
        (source/'CMakeLists.txt').write_text('''cmake_minimum_required(VERSION 3.25)
list(PREPEND CMAKE_MODULE_PATH "${RPLUSPLUS_MODULE_DIR}")
project(DepfileLoss LANGUAGES CXX Rpp)
add_library(dependency STATIC dependency.rs)
set_target_properties(dependency PROPERTIES RPP_CRATE_TYPE rlib)
add_library(leaf STATIC leaf.rs)
set_target_properties(leaf PROPERTIES RPP_CRATE_TYPE rlib)
target_link_libraries(leaf PRIVATE dependency)
add_library(bridge STATIC bridge.rs)
target_link_libraries(bridge PRIVATE leaf)
add_executable(Cpp main.cpp)
target_link_libraries(Cpp PRIVATE bridge)
add_executable(Rust main.rs)
target_link_libraries(Rust PRIVATE leaf)
''')
        files={'dependency.rs':'pub fn value()->i32 { 1 }',
               'module.rs':'pub mod nested; pub fn value()->i32 { 2 + nested::value() }',
               'module/nested.rs':'pub fn value()->i32 { 3 }',
               'text data.txt':'4', 'byte data.txt':'5'}
        for name,text in files.items():
            path=source/name;path.parent.mkdir(parents=True,exist_ok=True);path.write_text(text)
        (source/'leaf.rs').write_text('mod module; pub fn value()->i32 { dependency::value()+module::value()+include_str!("text data.txt").parse::<i32>().unwrap()+i32::from(include_bytes!("byte data.txt")[0]-b\'0\') }')
        (source/'bridge.rs').write_text('#[unsafe(no_mangle)]pub extern "C" fn value()->i32 {leaf::value()}')
        (source/'main.rs').write_text('fn main(){println!("{}",leaf::value());}')
        (source/'main.cpp').write_text('#include <cstdio>\nextern "C" int value(); int main(){std::printf("%d\\n",value());}')
        build=base/gen/'depfile build'
        def build_now(config):run('cmake','--build',build,'--config',config,'--parallel','4')
        def values(config,expected):
            for name in ('Rust','Cpp'):
                exe=build/config/name if gen=='Ninja Multi-Config' else build/name
                assert subprocess.check_output([str(exe)],text=True).strip()==str(expected),(gen,config,name,expected)
        def noop(config):
            for p in trace.iterdir():p.unlink()
            build_now(config)
            assert not list(trace.iterdir()),f'{gen}/{config}: compiler or driver invoked on no-op'
        for config in ('Debug','Release'):
            run('cmake','-S',source,'-B',build,'-G',gen,f'-DCMAKE_BUILD_TYPE={config}',
                f'-DRPLUSPLUS_MODULE_DIR={modules}',f'-DCMAKE_Rpp_COMPILER={rpp}',
                f'-DCMAKE_CXX_COMPILER={cxx}',f'-DRPP_RUSTC_EXECUTABLE={rustc}')
            build_now(config);values(config,15)
            # First deletion is before any consolidation/no-op pass.
            for name,old in files.items():
                path=source/name
                # Explicit values keep each edit's expected arithmetic unambiguous.
                if name=='dependency.rs':changed='pub fn value()->i32 { 11 }'
                if name=='module.rs':changed='pub mod nested; pub fn value()->i32 { 12 + nested::value() }'
                if name=='module/nested.rs':changed='pub fn value()->i32 { 13 }'
                if name=='text data.txt':changed='14'
                if name=='byte data.txt':changed='6'
                expected=16 if name=='byte data.txt' else 25
                for d in build.rglob('*.d'):d.unlink()
                path.write_text(changed);build_now(config);values(config,expected)
                if gen=='Unix Makefiles':assert list(build.rglob('*.rs.rlib.d'))
                path.write_text(old);build_now(config);values(config,15)
                noop(config)  # subsequent deletion also exercises consolidated caches
            # New dependency introduced by a root edit must survive immediate loss.
            leaf=source/'leaf.rs';old=leaf.read_text()
            (source/'introduced.rs').write_text('pub const VALUE:i32=0;')
            leaf.write_text(old.replace('mod module;', 'mod introduced; mod module;').replace('dependency::value()', 'introduced::VALUE+dependency::value()'))
            build_now(config)
            for d in build.rglob('*.d'):d.unlink()
            (source/'introduced.rs').write_text('pub const VALUE:i32=10;')
            build_now(config);values(config,25)
            leaf.write_text(old);build_now(config);values(config,15)
            run('cmake','--build',build,'--config',config,'--target','clean')
            build_now(config);values(config,15);noop(config)
            print(f'PASS v0.1/depfile-loss {gen}/{config}: modules, nested, text, bytes, extern, introduced input, clean recovery, zero-invocation no-op',flush=True)
