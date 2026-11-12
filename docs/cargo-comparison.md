# Cargo comparison

This opt-in benchmark compares **R++/CMake** with **Cargo plus CMake native orchestration**, using the same Rust 1.97.1 compiler, Cargo 1.97.1, CMake 4.4.5, C++ compiler, and multicrate fixture. It does not introduce a Cargo dependency into R++ itself.

## Workload and fairness

Both variants build the same five-crate diamond graph, a Rust executable calling C++, and a C++ executable calling a Rust staticlib through a native archive. They run the same functional checks after each repetition. All Rust dependencies are local paths, Cargo uses an isolated empty CARGO_HOME, and builds use --frozen --offline. There are no third-party crates or build scripts.

Both use Debug, four jobs, opt-level=0, debuginfo=2, debug assertions, and **256 codegen units explicitly**. Cargo is measured with incremental compilation enabled (its normal dev behavior) and disabled. R++ does not currently enable rustc's incremental cache. Rust editions and the driver_flag cfg are preserved. Cargo receives the cfg globally; R++ supplies it on the executable target.

CMake compiles native sources in both builds. Cargo's Rust executable declares its worker library using a link attribute and CMake creates that small native archive; R++ passes the worker object directly to its final link. Cargo's C++ executable consumes its Rust staticlib through an imported CMake target. The Cargo target evaluates the Cargo graph on each build and copies the staticlib with copy_if_different so native executables are not relinked on no-op builds. Cargo runtime library discovery uses a same-toolchain staticlib probe during untimed setup.

These small orchestration differences mean the comparison is of functioning mixed-language build workflows, not a pure Cargo-versus-CMake frontend race. Standalone Cargo no-op times are measured separately so the CMake integration and archive-copy costs are visible. Package resolution, bootstrap, configuration, cleaning and executable tests are outside timed intervals.

Five repetitions per scenario/generator/system were run sequentially with warm filesystem and toolchain caches on the same workstation as the [original baseline](validation-and-baseline.md). These runs used Linux x86_64, Intel i7-11850H, GCC 16.2.1, Ninja 1.13.2, GNU Make 4.4.1 and Python 3.14.10. No CPU pinning, cache eviction, run-order randomization or background-load isolation was performed. Small differences should not be treated as general performance conclusions.

Timings below use real executables without tracing wrappers. Counts come from a separate instrumented run. The R++ row was **remeasured** with the matched codegen-unit setting; it is not copied from the earlier baseline. All raw results include every sample and median/min/max summaries.

## Ninja: median milliseconds

| Scenario | R++ | Cargo incremental + CMake | Cargo nonincremental + CMake |
|---|---:|---:|---:|
| Clean application build | 238.1 | 320.0 | 304.3 |
| No-op | 8.2 | 36.1 | 34.1 |
| Shared Rust leaf edit | 215.9 | 217.7 | 216.7 |
| Rust executable root edit | 73.8 | 74.3 | 78.2 |
| C++ main edit | 120.6 | 146.5 | 147.9 |
| Final no-op | 7.8 | 35.4 | 34.3 |

Standalone Cargo no-op, excluding the CMake frontend and native artifact copy: 11.8 ms (Cargo incremental), 10.9 ms (Cargo nonincremental).

## Unix Makefiles: median milliseconds

| Scenario | R++ | Cargo incremental + CMake | Cargo nonincremental + CMake |
|---|---:|---:|---:|
| Clean application build | 555.4 | 417.3 | 414.8 |
| No-op | 169.3 | 106.9 | 101.7 |
| Shared Rust leaf edit | 505.3 | 291.3 | 304.8 |
| Rust executable root edit | 213.8 | 141.4 | 144.8 |
| C++ main edit | 295.3 | 226.7 | 228.5 |
| Final no-op | 158.6 | 105.7 | 100.6 |

Standalone Cargo no-op, excluding the CMake frontend and native artifact copy: 11.2 ms (Cargo incremental), 11.1 ms (Cargo nonincremental).

## Ninja Multi-Config: median milliseconds

| Scenario | R++ | Cargo incremental + CMake | Cargo nonincremental + CMake |
|---|---:|---:|---:|
| Clean application build | 233.4 | 313.4 | 293.8 |
| No-op | 8.9 | 35.6 | 34.5 |
| Shared Rust leaf edit | 223.6 | 208.7 | 215.7 |
| Rust executable root edit | 70.5 | 72.2 | 74.7 |
| C++ main edit | 122.4 | 138.7 | 141.6 |
| Final no-op | 8.4 | 34.1 | 35.9 |

Standalone Cargo no-op, excluding the CMake frontend and native artifact copy: 11.0 ms (Cargo incremental), 10.8 ms (Cargo nonincremental).

## Compiler invocation counts

Counts below are actual Rust **compilations**, excluding compiler metadata queries. They were identical across all repetitions and generators, and across both Cargo incremental modes.

| Scenario | R++ rustc compilations | Cargo rustc compilations | C++ compilations, both | Native link-driver calls, both |
|---|---:|---:|---:|---:|
| Clean application build | 7 | 5 | 3 | 2 |
| No-op | 0 | 0 | 0 | 0 |
| Shared Rust leaf edit | 7 | 5 | 0 | 2 |
| Rust executable root edit | 2 | 1 | 0 | 1 |
| C++ main edit | 0 | 0 | 1 | 1 |
| Final no-op | 0 | 0 | 0 | 0 |

Cargo also made **five rustc metadata queries** on each clean build. Thus total rustc process counts on clean builds were 10 for Cargo versus 7 for R++; only 5 versus 7 of those processes compiled code. Later builds, including no-ops, made no metadata queries in this run. Both no-op paths performed zero compilations and zero native links; R++ launched no driver/compiler processes, while the Cargo paths launched one cargo build command. Raw results separately count driver operations, Cargo operations and compiler queries. Native archiver subprocesses are not counted.

The extra R++ compilations are the accepted staging cost: five rlibs followed by a staticlib and binary compilation. Cargo directly builds three intermediate libraries, the staticlib and the binary. Editing the shared leaf rebuilds all five Rust crates in both dependency graphs, but R++ repeats the two boundary roots. Editing only the Rust executable root uses two R++ rustc invocations versus one with Cargo.

## Interpretation

On this fixture, Ninja and Ninja Multi-Config favored R++ for clean and no-op mixed builds. Rust edit times were close; Cargo incremental compilation slightly favored some cases. **Unix Makefiles favored the Cargo-backed workflow**, including clean, no-op and incremental builds. The R++ workflow exposes more native CMake targets and dependency helper targets, while Cargo schedules its Rust graph within one CMake target; this is a plausible contributor to Make overhead, not a measured attribution of time.

The practical optimization targets remain reducing R++ boundary recompilation and reducing recursive Make orchestration overhead. These measurements do not establish that either tool is universally faster.

## Reproduce and raw evidence

```sh
python3 tests/compare_cargo.py \
  --cmake /usr/bin/cmake \
  --rustc /path/to/rust-1.97.1/bin/rustc \
  --cargo /path/to/rust-1.97.1/bin/cargo \
  --repeats 5 --timing-only --output /tmp/cargo-comparison-timing.json

# Repeat without --timing-only for invocation counts.
```

- [Uninstrumented timings](baselines/cargo-comparison-timing.json)
- [Instrumented invocation counts](baselines/cargo-comparison-instrumented.json)

The script generates its local Cargo workspace from the existing fixture in a temporary directory and requires already installed tools. It never downloads dependencies or modifies the checked-in application sources. The source driver and fixture match revision 6043bf3; the comparison harness additionally supplies the Cargo manifests/native linkage declaration and matched codegen-unit option described above.
