# Depfile loss: root cause and recovery

Unix Makefiles runs CMake's dependency pass before compilation. The compiler's
new `.d` file is normally consumed on a later build and consolidated into
`compiler_depend.make` and `compiler_depend.internal`. `DependInfo.cmake` registers
the source/object/format/depfile mapping; it does not by itself make a missing
compiler depfile invalidate its object.

Deleting a depfile before this consolidation can therefore lose newly introduced
module or data dependencies while leaving a seemingly current Rust object.
Already consolidated dependencies survived raw depfile deletion in experiments.
The issue reproduced on CMake 3.25.0 and 4.4.5, and with a stock C++ header control
without Rpp. It is not a rustc module-discovery failure or a reason to patch CMake.

Rpp closes the gap by appending the Rust object's `.d` file to the crate root's
`OBJECT_OUTPUTS` for Unix Makefiles only. CMake then generates
`CMAKE_MULTIPLE_OUTPUT_PAIRS`: if a secondary output disappears, its dependency
pass invalidates the primary object and ordinary compilation regenerates both.
The object path comes from `$<TARGET_OBJECTS:target>`, filtered to the single Rust
`.rlib` object, so mixed C++ objects and custom source paths are handled without
hard-coded object-directory layouts. Existing extra outputs are preserved.
Ninja and Ninja Multi-Config retain their normal dependency-log behavior.

No compiler wrapper changes, unconditional scans, new per-build commands,
upstream patches or shell orchestration are needed. Normal no-op builds retain
zero compiler invocations. The release blocker remains active until the permanent
dependency-loss matrix has passed.
