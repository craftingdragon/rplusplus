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

The `rpp` driver bootstraps with CMake and stable `rustc`, using only Rust's standard library. Native targets support `rlib` dependencies, `staticlib` C++ boundaries, and Rust or C++ executable entry points. No Cargo or network fetching is used.

See [driver and target usage](docs/driver.md) and the `examples/multicrate` diamond graph.
The unreleased v0.2 driver can preprocess a single Rust file or mirror a
declared Rust source tree with the standalone command. See the [preprocessing guide](docs/preprocessing.md) for options,
examples and supported inputs.
```sh
cmake -S . -B build -G Ninja \
  -DRPP_RUSTC_EXECUTABLE=/absolute/path/to/stable/rustc
cmake --build build
cmake --install build --prefix /tmp/rplusplus-install
cmake -S examples/native -B build-example -G Ninja \
  -DRPLUSPLUS_MODULE_DIR=/tmp/rplusplus-install/share/rplusplus/cmake \
  -DRPP_RUSTC_EXECUTABLE=/absolute/path/to/stable/rustc
cmake --build build-example
./build-example/MyApp
RPP_TEST_RUSTC="$(rustup which --toolchain stable rustc)" python3 tests/generators.py
```

Prerequisites: CMake 3.25 or later, Python 3, rustc supporting edition 2024, a C++ compiler, an archiver, Ninja, and Make. The CMake 3.25.0 minimum and CMake 4.4.5 pass the full gate with stable Rust 1.96.0 and 1.97.1. See [validation and build baselines](docs/validation-and-baseline.md) and the [Cargo comparison](docs/cargo-comparison.md). Cargo and network access are not used.

See [v0.1 hardening and release blockers](docs/v0.1-readiness.md) and the
`examples/qt-notes` Widgets application with a Rust backend and system SQLite.

## Direction

Build the driver itself through CMake and rustc; preserve upstream Rust safety checking; add optional Rust-aware C-style preprocessing, explicit local dependencies, version-checked system libraries, and audited C ABI bridges. Keep interop and compiler backends separate for future CXX and deeper C++ integration. Linux comes first, with portable process, path, and platform-policy layers.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
Unicode identifier property data retains the notices in [LICENSES](LICENSES).

Release preparation: [candidate notes](docs/release-0.1.0.md) and [release procedure](docs/release-procedure.md).
