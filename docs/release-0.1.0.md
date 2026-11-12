# R++ v0.1.0 — draft experimental release

R++ builds Rust with stable rustc and ordinary CMake targets, without Cargo or
network fetching. Enable the public language with `project(App LANGUAGES CXX Rpp)`.

Features include explicit local Rust dependencies, edition selection, rlib
intermediates, staticlib C++ boundaries, Rust/C++ executable entry points, compiler
depfiles and installed-package consumption. A Qt Widgets notes application
exercises system SQLite, UTF-8 persistence, callbacks and explicit C ABI ownership.

Native Linux is the release target. Rust 1.85.0 is the minimum under validation.
Rust 1.99.1/1.100.0 and CMake 3.25.0/4.4.5 form the required frozen matrix.
No support claim is made for Windows, macOS, cross-compilation, shared Rust crate
types, procedural macros, registries, preprocessing or automatic C++ imports.
Rust metadata is compiler-specific; rebuild the full graph with one toolchain.
The FFI requires valid foreign pointers, matching frees and single-threaded handles.

GPL-3.0-or-later. Cargo appears only in the optional offline comparison benchmark.
Timing differences are workload-specific; no universal performance claim is made.

This document is a draft. No v0.1.0 tag or publication is authorized by its presence.
