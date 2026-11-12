# R++

Rust, just better. A Cargo-free Rust toolchain built around upstream `rustc` and native CMake integration.

## Why CMake uses `Rpp`

R++ is the project name; **`Rpp` is the public CMake language identifier**:

```cmake
list(PREPEND CMAKE_MODULE_PATH "${RPLUSPLUS_MODULE_DIR}")
project(MyApp LANGUAGES CXX Rpp)
```

CMake embeds language identifiers in generated Ninja rule names. Literal `R++` produces names such as `R++_COMPILER__...`, which Ninja rejects because `+` is invalid in a rule identifier. `Rpp` works with unmodified CMake and Ninja. No upstream patch or generated-file rewriting is required. See [the compatibility investigation](docs/cmake-compatibility.md).

## Current milestone

Native language modules and a reproducible mixed Rust/C++ compatibility gate. The initial path supports one `no_std` Rust crate root per target, emitted as a native object and archived or linked with C++. Rust modules are compiler-tracked dependencies. This is not yet a general Rust build system: Rust executables, the standard-library runtime, cross-crate Rust metadata, preprocessing, registry resolution, and generated interop bridges remain future work.

```sh
cmake -S . -B build -G Ninja
cmake --install build --prefix /tmp/rplusplus-install
cmake -S examples/native -B build-example -G Ninja \
  -DRPLUSPLUS_MODULE_DIR=/tmp/rplusplus-install/share/rplusplus/cmake
cmake --build build-example
./build-example/MyApp
python3 tests/generators.py
```

Prerequisites: CMake 3.25 or later, Python 3, rustc supporting edition 2024, a C++ compiler, an archiver, Ninja, and Make. CMake's tested versions are listed in the compatibility document; the minimum is not yet a tested compatibility guarantee. Cargo and network access are not used.

## Direction

Build the driver itself through CMake and rustc; preserve upstream Rust safety checking; add optional Rust-aware C-style preprocessing, explicit local dependencies, version-checked system libraries, and audited C ABI bridges. Keep interop and compiler backends separate for future CXX and deeper C++ integration. Linux comes first, with portable process, path, and platform-policy layers.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
