# R++ v0.2.0 — experimental release candidate

R++ adds optional Rust-aware preprocessing to its Cargo-free compiler driver and native CMake language.

- Use `rpp preprocess` for single files and declared crate trees. The supported directives cover object-like macros, conditionals, quoted relative includes and explicit errors.
- Enable preprocessing on an ordinary native `Rpp` target with `RPP_PREPROCESS ON`. CMake tracks mirrored files and configuration-specific outputs. The compiler verifies the snapshot before using it.
- Read original-source notes for supported compiler diagnostic spans. Explicit rustc JSON output retains its original format.
- Run the Qt Notes example with `-DRPP_NOTES_PREPROCESS=ON` for a SQLite-backed note count. The ordinary build keeps preprocessing off.

The minimums are CMake 3.25 and stable Rust 1.85 with Linux as the validated platform. The build uses upstream stable rustc, stock CMake and Ninja, and Rust's standard library. Preprocessing requires explicitly declared inputs. Function-like macros, token pasting, broad C preprocessor compatibility, Cargo importing and registries are outside this release.

[Build instructions](../README.md) · [Preprocessing guide and limitations](preprocessing.md) · [Release validation procedure](release-procedure.md)

GPL-3.0-or-later. See the candidate's signed release manifest for approval status.
