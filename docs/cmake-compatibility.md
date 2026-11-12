# CMake language identifier decision

Public language identifier: `Rpp`. Project name: R++. Use `CMAKE_Rpp_*` variables and `LANGUAGE Rpp` consistently.

## Investigation

With CMake 4.4.5, Ninja 1.13.2, and GNU Make 4.4.1 on Linux, external language modules accepted `project(MyApp LANGUAGES CXX R++)`. Unix Makefiles compiled a small no_std Rust library, archived it, linked a mixed C++/Rust executable, and rebuilt after a Rust module changed. Ninja and Ninja Multi-Config failed to parse the emitted `R++_COMPILER__...` rules. A real Ninja compiler check also failed during configuration.

CMake inserts the language verbatim in compiler and linker rule constructors while encoding target names. Ninja identifiers match `[a-zA-Z0-9_.-]+`, excluding plus. Language modules cannot override those constructors. Encoding only internal rule names made the diagnostic experiment build, but rewriting generated files would break regeneration and is not a supported workaround.

Sources:

- [CMake compiler rule constructors](https://github.com/Kitware/CMake/blob/v4.4.5/Source/cmNinjaTargetGenerator.cxx)
- [CMake linker rule constructors](https://github.com/Kitware/CMake/blob/v4.4.5/Source/cmNinjaNormalTargetGenerator.cxx)
- [Ninja lexer](https://github.com/ninja-build/ninja/blob/v1.13.2/src/lexer.in.cc)

## Resolution

Use `Rpp` rather than carry a CMake patch or change Ninja. This was an explicit project decision. The renamed probe passed compiler checks, static-library creation, mixed linking, execution, no-op builds, and module-triggered rebuilds on Ninja, Unix Makefiles, and Ninja Multi-Config.

External language modules use internal CMake interfaces with no compatibility guarantee. Currently verified: CMake 4.4.5 and the tools above. Do not infer support for every version from the configured minimum. The available rustc was 1.98.0-nightly; the probe used no nightly features, but stable toolchain validation remains required.

## Gate boundaries

The example intentionally uses no_std Rust without allocation or runtime dependencies. It proves identifier compatibility and depfile plumbing, not full Rust runtime aggregation, crate metadata linkage, or safe arbitrary C++ interoperability. Compiler checks must compile a real Rust fixture, not force COMPILER_WORKS. Native modules must remain usable after installation in an unrelated consumer build.

The regression script tests all three Linux generators, installation, a build path containing spaces, compiler checks, execution, no-op behavior, and selective module rebuilds. Windows, macOS, other generators, and comprehensive version coverage remain unverified.
