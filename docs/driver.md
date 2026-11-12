# Cargo-free driver milestone

## Bootstrap and tool selection

Configure R++ with `-DRPP_RUSTC_EXECUTABLE=/absolute/path/to/stable/rustc`, build, then install. The driver is installed to `bin/rpp`; modules remain in `share/rplusplus/cmake`. Consumers set `RPLUSPLUS_MODULE_DIR` before enabling `project(App LANGUAGES CXX Rpp)`. Alternatively call `find_package(Rplusplus CONFIG REQUIRED)` before `project()` with the installed prefix on CMAKE_PREFIX_PATH. Installed modules discover the sibling installed driver; `CMAKE_Rpp_COMPILER` can explicitly select another installed driver. Select the same underlying rustc for every crate in a build.

The driver uses stable command-line interfaces and standard-library process/filesystem APIs. It does not call Cargo, run build scripts, fetch dependencies, or use rustc internals. GPL-3.0-or-later remains unchanged.

## Ordinary targets

```cmake
add_library(core STATIC lib.rs)
set_target_properties(core PROPERTIES
  RPP_CRATE_TYPE rlib RPP_CRATE_NAME app_core RPP_EDITION 2021)
add_executable(App main.rs worker.cpp)
set_target_properties(App PROPERTIES RPP_ENTRY_LANGUAGE Rpp)
target_link_libraries(App PRIVATE core)
target_compile_options(App PRIVATE "$<$<COMPILE_LANGUAGE:Rpp>:--cfg=custom_flag>")
```

`RPP_CRATE_ROOT` chooses one root when multiple Rust files are listed; otherwise exactly one .rs root is required. Other listed Rust files are not compiled independently. Modules are discovered through rustc depfiles. Each root must be owned by one target in its CMake source directory.

`RPP_CRATE_TYPE` accepts `bin`, `rlib`, and `staticlib`. Executables default to bin; static libraries default to staticlib. `RPP_CRATE_NAME` defaults to the target name with hyphens replaced by underscores. `RPP_EDITION` defaults to 2024. For a C++ main in a mixed target, set `RPP_ENTRY_LANGUAGE CXX` explicitly. An rlib target cannot include C++ sources.

Declare Rust crate dependencies as explicit CMake target names through `target_link_libraries`, with PRIVATE/PUBLIC/INTERFACE visibility. The dependency's crate name is its Rust import name. Direct crate names must be distinct. Rust dependencies need to be rlib targets; staticlib is a native linkage boundary. Dependencies are source-built; prebuilt Rust metadata distribution and complex generator-expression dependency selection are outside this release.

Use language-scoped `target_compile_options` for rustc flags, including `--cfg` and combined `-Copt-level=...` forms. C/C++ -D definitions are not automatically translated. Debug and Release defaults initialize CMAKE_Rpp_FLAGS_<CONFIG>; target options may override them. `target_link_options` supplies native linker options. Standard output directory properties work through CMake's target paths.

## Compile/link staging

The native compile rule emits an rlib containing Rust metadata, compiled code, and a private non-object archive member recording its invocation. One archive owns both code and the record, avoiding separately missing metadata outputs. Compiler depfiles include module files and direct --extern artifacts; build-local artifact paths are relative so Ninja identifies their producing edges correctly.

Publishing an rlib only copies the staged archive. Producing a bin or staticlib recompiles its root using the recorded options and already compiled dependency rlibs. The temporary library compilation of a Rust binary caps lints, avoiding spurious unused-main warnings; the final binary compilation applies the requested lints normally. Eliminating this redundant root stage later requires no public API change.

Rust main uses rustc for final linking, with C++ objects and native dependencies. C++ main creates a single Rust staticlib boundary, obtains native runtime requirements through rustc --print=native-static-libs, then invokes the configured C++ linker driver. Order and duplicate native library arguments are preserved. Multiple separate Rust runtime boundaries in one executable are rejected.

## Installation and exports

Export native staticlib targets with ordinary install(TARGETS ... EXPORT ...). Also install the accompanying `<target archive>.runtime` file beside the archive; it contains toolchain-reported native linker arguments, not build-tree paths. Enable Rpp in the consumer so final linking reads these requirements. The multicrate example demonstrates these install rules and an external C++ consumer. Rust metadata dependencies are removed from the exported native boundary because the staticlib already contains them.

## Tests and current limits

`RPP_TEST_RUSTC=/path/to/stable/rustc python3 tests/generators.py` tests installed modules, paths containing spaces, real compiler checks, parallel builds, both entry points, edition 2021 and 2024, compiler flag propagation, diamond dependencies, selective rebuilding, no-op builds, Debug/Release separation, installation, relocation, and external exports on Ninja, Unix Makefiles, and Ninja Multi-Config. A failing Cargo sentinel is placed first on PATH. Rust unit tests compile with rustc --test.

Verified with CMake 3.25.0 and 4.4.5, each paired with stable Rust 1.96.0 and 1.97.1, using Ninja 1.13.2 and GNU Make 4.4.1 on Linux. See [validation and build baselines](validation-and-baseline.md) for raw results, invocation counts and reproduction commands. Non-Linux linking, shared Rust libraries, procedural macros, preprocessing, registries, automatic header imports, and cross-compilation are deferred. Native archive/runtime policies will need platform-specific extensions before Windows/macOS support is claimed.
