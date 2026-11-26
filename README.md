# R++

Rust, just better. A Cargo-free Rust toolchain built around upstream `rustc` and native CMake integration.

## Why CMake uses `Rpp`

R++ is the project name; **`Rpp` is the public CMake language identifier**:

```cmake
list(PREPEND CMAKE_MODULE_PATH "${RPLUSPLUS_MODULE_DIR}")
project(MyApp LANGUAGES CXX Rpp)
```

CMake embeds language identifiers in generated Ninja rule names. Literal `R++` produces names such as `R++_COMPILER__...`, which Ninja rejects because `+` is invalid in a rule identifier. `Rpp` works with unmodified CMake and Ninja. No upstream patch or generated-file rewriting is required. See [the compatibility investigation](docs/cmake-compatibility.md).

## Experimental v0.2.0 candidate

The `rpp` driver bootstraps with CMake and stable `rustc`, using only Rust's standard library. Native targets support `rlib` dependencies, `staticlib` C++ boundaries, and Rust or C++ executable entry points. No Cargo or network fetching is used.

See [driver and target usage](docs/driver.md) and the `examples/multicrate` diamond graph.
Opt-in Rust-aware preprocessing supports standalone files and declared Rust
source trees. Native CMake targets enable it with `RPP_PREPROCESS ON`.
See the [preprocessing guide](docs/preprocessing.md) for directives, CMake
properties, examples and input limits.
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

Bootstrap requires CMake 3.25 or later, stable Rust 1.85 or later and a
supported generator. Native mixed projects also use a C++ compiler and an
archiver. Python 3 is used by the regression suite. The optional Qt Notes
example uses system Qt 6 Widgets and SQLite. The build is Cargo-free and
offline. See the [release validation procedure](docs/release-procedure.md)
for the exact candidate tool inventory and [Qt Notes instructions](docs/preprocessing.md#qt-notes-example-v02-m7)
for the optional SQLite-backed count.

## Direction

Build the driver itself through CMake and rustc; preserve upstream Rust safety checking; support optional Rust-aware C-style preprocessing, explicit local dependencies, system libraries, and audited C ABI bridges. Keep interop and compiler backends separate for future CXX and deeper C++ integration. Linux comes first, with portable process, path, and platform-policy layers.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
Unicode identifier property data retains the notices in [LICENSES](LICENSES).

[v0.2.0 candidate notes](docs/release-0.2.0.md) · [Release procedure](docs/release-procedure.md)
