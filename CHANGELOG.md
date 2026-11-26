# Changelog

## 0.2.0 — experimental candidate

- Add Rust-aware preprocessing with object-like macros, quoted includes, conditional directives and source maps.
- Mirror explicitly declared crate sources and data for standalone and native CMake builds.
- Verify generated snapshots before compilation and final linking, audit dependencies and map supported diagnostics to original sources.
- Recover generated outputs and depfiles across Ninja, Unix Makefiles and Ninja Multi-Config.
- Add an optional preprocessed SQLite note count to the Qt example.

## 0.1.0 — experimental release

- Bootstrap the standard-library-only Rust compiler driver without Cargo.
- Add native `Rpp` CMake targets, explicit local crate graphs and Rust/C++ linking.
- Add Qt Widgets validation with a Rust backend and system SQLite.
- Embed native runtime requirements in static archives and support relocated consumers.
- Recover Make builds after depfile loss; preserve outputs on tool failures.
- Fix response-file arguments bypassing output staging.
