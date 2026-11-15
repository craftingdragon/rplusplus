# v0.2/M0: preprocessing feasibility and contract

Status: **PASS for M0 feasibility, with the approved explicit-OUTPUT adjustment.**
No production preprocessing has been implemented. The released compile/link,
language modules and runtime metadata are unchanged.
This records M0's scoped feasibility approval, not v0.2 release approval.
The subsequent [M1 scanner](lexer.md) is internal infrastructure only.

## Reproduce

Run the complete adjusted M0 gate from the repository root with preinstalled
tools (no downloads or Cargo):

```sh
python3 tests/preprocess_graph.py \
  --cmake /absolute/path/to/cmake-3.25.0/bin/cmake \
  --cmake /absolute/path/to/cmake-4.5.0/bin/cmake \
  --rustc /absolute/path/to/rust-1.85.0/bin/rustc \
  --rustc /absolute/path/to/rust-1.100.0/bin/rustc \
  --output build-v02-m0/custom-graph.json
```

All four tool pairs must pass for this command to return zero. It includes the
original native co-output negative control and source/diagnostic feasibility
probes. For the separate unchanged-v0.1 baseline run tests/compatibility_matrix.py
with the same repeated --cmake/--rustc arguments and an --output JSON path.

The original standalone probe below remains a negative control:

```sh
python3 tests/preprocess_feasibility.py \
  --cmake /absolute/path/to/cmake-3.25.0/bin/cmake \
  --cmake /absolute/path/to/cmake-4.5.0/bin/cmake \
  --rustc /absolute/path/to/rust-1.85.0/bin/rustc \
  --rustc /absolute/path/to/rust-1.100.0/bin/rustc \
  --output build-v02-m0/report.json
```

That original command deliberately exits nonzero: native compile co-outputs still
fail on Ninja, and the accepted design does not use them for generated payloads.
The report retains every missing-output observation; restoring a failed probe's
fixture only permits the other independent cases to run, and never changes the
failed observation to a pass. Missing tools are blocked; unexpected test failures
are failed. This is an explicit offline investigation runner, not a replacement
for the existing passing v0.1 CTest gates.

## Observations (2026-11-15)

Tested stable rustc 1.85.0 and 1.100.0, CMake 3.25.0 and 4.5.0,
Ninja, Unix Makefiles and Ninja Multi-Config, Debug and Release.

### Mirrored source resolution: PASS for the tested subset

The fixture removes the original source tree before compiling the mirror.
Both editions 2021 and 2024 compile and execute a nested module, relative path
attribute, Rust include, nested Rust include, Unicode text include and byte
include. All declared files occur in rustc's emitted dependency information.
This demonstrates the layout approach, not a complete Rust module resolver.
Production validation still needs inline-module path attributes, symlinks,
collisions and rejection of undeclared files.

### Macro-generated reads: observable paths, incomplete semantics

An external rlib's exported macro introduces an absolute include_str read that
is invisible in the consuming source's literal include syntax. Another exported
macro introduces an absolute #[path] module. Successful rustc compilation reports
both files in dep-info; a publication-time allowlist check can reject these
tested escapes. A source-only scanner cannot infer these reads without expanding
the macro or inspecting its implementation.

The same file is accepted as include!, include_str! and include_bytes! input.
Each depfile names the file but supplies no source-versus-data role. In
particular, a macro can read a declared transformed source as data, or a declared
asset as Rust. A path allowlist alone cannot establish that these operations
preserve original bytes or that all Rust input was preprocessed. Do not call
this full hermeticity, do not infer roles from filename extensions, and do not
silently claim macro-generated loading support.

The v0.2 contract remains directly written literal includes with explicit input
roles; recognized computed forms are rejected. Macro-generated filesystem loading
is unsupported, with the limits of detection stated explicitly. Dependencies
reported after a successful compile must be audited before publication. No claim
is made about complete dep-info availability after failed compilation, arbitrary
compiler filesystem access, or external proc-macro execution (unsupported).

### Structured diagnostics: usable, mapping is not yet implemented

Real JSON diagnostics cover E0308, E0425 and E0499, a format error with two
primary spans, and a Rust macro expansion with expansion metadata. Byte positions
and Unicode-scalar columns agree with the fixture's UTF-8 source. The format-error
case checks that multiple primaries belong to one diagnostic, rather than merely
receiving several independent errors.

A deliberately substituted missing identifier has an exact byte range that can
be associated with the original NAME use-site. Identity mappings for copied
text can process every primary independently. These are fixture-level feasibility
checks, not a production source-map implementation. Expansion metadata and the
original rendered diagnostic remain available for ambiguous fallback. M5 must
still demonstrate real scanner-generated maps, transformed type/borrow errors,
multi-segment ambiguity, JSON compatibility and unchanged fix-it suggestions.

### Native compile co-outputs: FAIL on both Ninja generators

The test wraps the real driver, writes three representative generated outputs
after a successful compile, and attaches them through source OBJECT_OUTPUTS.
It deletes each output, builds again and records whether compilation restored it.

| Generator | CMake 3.25.0 | CMake 4.5.0 |
| --- | --- | --- |
| Unix Makefiles | recovered | recovered |
| Ninja | missing output; no work | missing output; no work |
| Ninja Multi-Config | missing output; no work | missing output; no work |

Results agree for both Rust versions and configurations: 24 successful recovery
cases and 48 failing cases. Normal no-ops after each case invoke no driver.
Ninja emits a separate phony edge shaped like:

```text
build state/Debug/module.rs state/Debug/map state/Debug/complete: phony CMakeFiles/probe.dir/main.rs.rlib
```

These paths are not real outputs of the compiler edge. Their absence therefore
does not invalidate the existing object. Treating them as reliable owned compile
outputs, as the initial plan proposed, would leave missing state silently stale.

This does not invalidate v0.1's Unix Makefiles depfile fix: that fix relies on
Make's multiple-output-pair handling, which the probe confirms still recovers.
Do not remove it or generalize its behavior to Ninja.

### Existing architecture baseline: PASS

With stable Rust 1.85.0, tests/generators.py passed independently under both
CMake versions. This includes all three generators, mixed Rust/C++ entry points,
installed and relocated modules, isolated native consumers after deletion of
producer sources/builds, Debug/Release coverage, dependency-loss recovery,
selective rebuilds, failure recovery and zero-invocation no-ops. The existing
driver unit and publication-failure CTests also passed under CMake 4.5.0.
This is baseline preservation evidence, not validation of a future preprocessing
implementation. Qt and the complete release matrix have not been rerun for M0.

```sh
RPP_TEST_CMAKE=/absolute/path/to/cmake \
RPP_TEST_RUSTC=/absolute/path/to/rust-1.85.0/bin/rustc \
python3 tests/generators.py
```

Local raw logs are build-v02-m0/baseline-3.25.log and
build-v02-m0/baseline-4.5.log; the probe's JSON includes compiler versions,
structured diagnostic evidence and all 72 missing-output observations.

## Approved architectural adjustment: validated

Give the generated tree a separate CMake add_custom_command edge with real
OUTPUT entries and its own DEPFILE. Preserve the original native Rpp compile/link
architecture; compilation consumes the completed preprocessing snapshot through
an explicit prerequisite. Keep preprocessing optional, per-target/configuration,
and standard-library-only. No unconditional scan or compiler invocation is allowed.

The custom-command gate now proves:

1. Real custom-command outputs recover after each deletion on all six
   generator/configuration combinations and both CMake versions.
2. Ordering and dependency wiring force the native compile to consume the new
   snapshot after a source/definition change, while preserving zero-command no-ops.
3. Separate preprocessing and compiler depfiles each survive deletion followed
   immediately by a newly introduced dependency edit, including Make's
   before-consolidation window.
4. Atomic completion-state publication and interrupted regeneration cannot expose
   an incomplete tree to compile or final recompile.
5. Installed modules can generate this graph in an unrelated consumer without
   knowledge of R++'s source/build tree.

This changes output ownership and scheduling, not preprocessing features or
mandatory recovery guarantees. See the [graph design and results](preprocessing-graph-design.md)
and [versioned observations](baselines/v0.2-m0-custom-graph.json).

The prototype is test-only Python scaffolding, using the existing Rust driver and
installed modules. It deliberately copies files and inserts a numeric fixture
value instead of implementing preprocessing syntax. Production behavior, a Rust
scanner, source maps, CLI and CMake preprocessing properties remain later work.
