# Compatibility validation and build baseline

## Validated toolchains

The advertised **CMake 3.25 minimum is validated at 3.25.0**. All four combinations below passed the full CTest gate: driver unit tests plus the installed-module generator tests. No upstream CMake or Ninja changes were used.

| CMake | Stable Rust | Result |
|---|---|---|
| 3.25.0 | 1.96.0 | passed |
| 3.25.0 | 1.97.1 | passed |
| 4.4.5 | 1.96.0 | passed |
| 4.4.5 | 1.97.1 | passed |

Each combination covers Ninja, Unix Makefiles and Ninja Multi-Config; Debug/Release; Rust/C++ entry points; diamond dependencies; flag and edition behavior; rebuilds; no-op builds; installation; relocation; and external consumers. Each run uses its selected CMake **and matching CTest**, including nested bootstrap and negative edition tests. A failing Cargo sentinel is first on PATH. [Raw compatibility results](baselines/compatibility.json).

CMake 3.25.0 was obtained as the pinned PyPI wheel `cmake-3.25.0-py2.py3-none-manylinux_2_17_x86_64.manylinux2014_x86_64.whl` and extracted under `/tmp`, without changing the system installation. Wheel SHA-256: `9b7fd744a90e4d804ff77ac50d3570009911fbfdad29c59fc93d2a82faaeb371`. The test tools were provisioned explicitly; neither test harness fetches tools or dependencies. Intermediate CMake versions and non-Linux platforms were not tested.

## Measurement method

Workload: `examples/multicrate`, using Debug, four parallel build jobs, stable Rust 1.97.1, and five repetitions per generator/scenario. Driver and fixture code correspond to revision `82f6c06`; the validation harness was added subsequently. Hardware: Intel Core i7-11850H, 16 logical CPUs, approximately 31 GiB RAM; Linux x86_64, GCC 16.2.1, Ninja 1.13.2, GNU Make 4.4.1, Python 3.14.10.

Each repetition cleans application artifacts, then builds, runs a no-op build, edits the shared Rust leaf, edits the Rust executable root, edits the C++ main file, and runs another no-op build. The leaf edit adds a public function, changing crate metadata; the executable-root and C++ edits add comments. Executables are tested after each repetition. Source copies and build trees use temporary paths containing spaces.

Reported wall times are **uninstrumented**, with real compiler/driver executables. Counts come from separate instrumented runs using process wrappers; wrapper overhead is excluded from the timing tables. No-op runs in the instrumented suite assert zero driver/compiler calls. Raw samples include median/min/max summaries.

These are small application-build baselines with warm filesystem/toolchain caches. Cleaning, driver bootstrap, configuration, installation and executable tests are excluded from timed intervals. No attempt was made to evict OS caches, pin CPUs, or isolate other workstation processes. Treat these as a reproducible local reference, not cold-cache measurements, broad performance claims, or hard CI time limits.

## CMake 3.25.0: median elapsed milliseconds

| Scenario | Ninja | Unix Makefiles | Ninja Multi-Config |
|---|---:|---:|---:|
| Clean application build | 214.2 | 341.0 | 210.6 |
| No-op | 3.9 | 70.8 | 3.9 |
| Shared Rust leaf edit | 204.0 | 303.3 | 202.9 |
| Rust root edit | 61.2 | 121.5 | 61.1 |
| C++ main edit | 106.5 | 174.0 | 105.2 |
| Final no-op | 3.6 | 69.0 | 3.8 |

[Uninstrumented samples](baselines/cmake-3.25.0-rust-1.97.1-timing.json); [instrumented samples](baselines/cmake-3.25.0-rust-1.97.1-instrumented.json).

## CMake 4.4.5: median elapsed milliseconds

| Scenario | Ninja | Unix Makefiles | Ninja Multi-Config |
|---|---:|---:|---:|
| Clean application build | 224.2 | 516.4 | 222.6 |
| No-op | 7.7 | 153.3 | 8.4 |
| Shared Rust leaf edit | 211.2 | 454.8 | 208.9 |
| Rust root edit | 65.9 | 197.3 | 64.9 |
| C++ main edit | 116.5 | 272.9 | 111.6 |
| Final no-op | 8.3 | 148.6 | 7.9 |

[Uninstrumented samples](baselines/cmake-4.4.5-rust-1.97.1-timing.json); [instrumented samples](baselines/cmake-4.4.5-rust-1.97.1-instrumented.json).

## Invocation counts

Counts were identical across five repetitions, both CMake versions, and all three generators. The C++ link-driver column includes the C++ linker invocation made by rustc for the Rust executable; it is not another C++ source compilation. Archiver subprocesses are not included. The `rpp link` operation also publishes rlibs and creates native archives, so its count exceeds the number of executable links.

| Scenario | rpp compile | rpp link | rustc rlib | rustc staticlib | rustc bin | C++ compile | C++ link driver |
|---|---:|---:|---:|---:|---:|---:|---:|
| Clean application build | 5 | 7 | 5 | 1 | 1 | 3 | 2 |
| No-op | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| Shared Rust leaf edit | 5 | 6 | 5 | 1 | 1 | 0 | 2 |
| Rust root edit | 1 | 1 | 1 | 0 | 1 | 0 | 1 |
| C++ main edit | 0 | 1 | 0 | 0 | 0 | 1 | 1 |
| Final no-op | 0 | 0 | 0 | 0 | 0 | 0 | 0 |

The shared leaf is an ancestor of all five Rust crates, so its edit recompiles all five staged crates plus the staticlib and executable boundaries: seven rustc calls, with no C++ recompilation. Editing only the Rust executable root uses two rustc calls, exposing the accepted staged-recompilation cost. Editing C++ main recompiles one C++ file and relinks its executable without running rustc. No-op builds launch neither rpp nor either compiler.

## Reproduce

Provide absolute paths to already installed tools; the commands below download nothing:

```sh
python3 tests/compatibility_matrix.py \
  --cmake /path/to/cmake-3.25.0/bin/cmake --cmake /usr/bin/cmake \
  --rustc /path/to/rust-1.96.0/bin/rustc --rustc /path/to/rust-1.97.1/bin/rustc \
  --output /tmp/rpp-compatibility.json

python3 tests/benchmark.py --cmake /path/to/cmake/bin/cmake \
  --rustc /path/to/stable/bin/rustc --repeats 5 --timing-only \
  --output /tmp/rpp-times.json

python3 tests/benchmark.py --cmake /path/to/cmake/bin/cmake \
  --rustc /path/to/stable/bin/rustc --repeats 5 \
  --output /tmp/rpp-counts.json
```

For a single compatibility gate, set `RPP_TEST_CMAKE`, `RPP_TEST_RUSTC`, and optionally `RPP_TEST_CTEST` before running `python3 tests/generators.py`. CTest otherwise defaults to the selected CMake binary's sibling. Run benchmarks sequentially, without concurrent compatibility/build jobs, to reduce avoidable timing interference.
