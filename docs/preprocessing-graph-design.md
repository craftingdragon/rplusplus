# M0 custom-command designs (before prototype)

The production driver and CMake modules are not changed by this investigation.
The following candidates will be compared with real generator output, not inferred
from documentation. The existing native OBJECT_OUTPUTS probe remains a negative
control, including its Make success and Ninja failures.

| Design | Expected Ninja behavior | Make concern |
| --- | --- | --- |
| completion OUTPUT, payload BYPRODUCTS | Ninja knows all outputs | missing byproducts may have no recovery recipe |
| completion and every required file OUTPUT | real ownership of every file | secondary-output checking/touching may affect timestamps |

Prototype the second design only if the comparison confirms recovery. Every
target/configuration gets a separate output directory and a single writer. A
command-bearing preprocessing edge is reached through a command-free ordering
target. Native compilation has both an ordering dependency and file dependencies
on generated payload and snapshot metadata. Its source is still an ordinary Rpp
crate; a test-only adapter selects the generated root and checks completion state.

The completion stamp is a scheduling output, touched after every successful
generation so unchanged content cannot leave the command perpetually dirty.
Payload/map/manifest writes compare bytes first. Compilation depends on content
outputs, not just the scheduling stamp. The fixture uses no Rust preprocessing
syntax: it copies sources/assets and inserts one numeric fixture value.

The preprocessing depfile is a required OUTPUT for Make only, analogous to the
existing compiler-depfile co-output fix. For Ninja, measure depfile/dependency-log
behavior: an already consumed depfile must not become a permanently missing
required output. Test editing dependencies with and without on-disk depfiles.

Before updating M0 status, test interruption after completion invalidation,
snapshot rejection at compile and final-link entry, timestamp behavior, genuine
zero-command no-ops, installed-only consumers, and target/configuration ownership.
No conclusion or release-gate relaxation follows from this design note alone.

## Measured results (2026-11-15)

The full four-pair matrix passed with CMake 3.25.0/4.5.0 and stable Rust
1.85.0/1.100.0. Every pair exercised all three generators and both configurations.
The separate existing compatibility matrix also passed all four pairs, including
driver unit/failure tests, toolchain errors, the permanent depfile-loss matrix,
mixed linking, installed-module relocation and isolated native consumers.
Qt was not rerun: this changes only test scaffolding and documentation.

| Graph | Ninja | Unix Makefiles | Ninja Multi-Config |
| --- | --- | --- | --- |
| Original native compile OBJECT_OUTPUTS | fails recovery | recovers | fails recovery |
| Completion OUTPUT, required payload BYPRODUCTS | recovers | fails | recovers |
| Every required file OUTPUT | passes | passes | passes |

The BYPRODUCTS Make control failed parallel initial builds with missing-payload
"No rule to make target" errors. It is not a viable ownership declaration for
files that must recover after deletion. Only dispensable outputs may be optional
byproducts; none of this prototype's required files are declared that way.

The selected graph passed 168 individual output-deletion recoveries: three Rust
files, a copied data asset, map, manifest and completion marker across 24
tool/generator/configuration cases. The generated Rust reads its copied asset
through both include_str! and include_bytes!. Removing an active target's output
does not modify another target or configuration's files. A four-job parallel
build compiles two independent mixed Rust/C++ targets. Both configurations have
distinct specs, generated trees, objects and executables.

### Ordering, file dependencies and timestamps

The command-free ordering target finishes preprocessing before native compilation.
OBJECT_DEPENDS adds file dependencies on all generated source/data/map/manifest
files, separately from that ordering relationship. A map-only content change
forces recompilation even though rustc does not read the map; this tests the
explicit file edge rather than relying solely on compiler-discovered inputs.
Normal source edits affect the owning target only.

Configuration-specific OBJECT_DEPENDS paths use the tested CMAKE_CFG_INTDIR
placeholder on Ninja Multi-Config and CMAKE_BUILD_TYPE on single-config generators;
they do not assume OBJECT_DEPENDS evaluates arbitrary generator expressions.

On a timestamp-only input touch, the preprocessing command runs once. Ninja
preserves unchanged payload timestamps and does not recompile. Make may execute
CMake's secondary-output touch rules, advance otherwise unchanged payload
timestamps and recompile once. This is a measured generator difference, not
content rewriting by the prototype. No generator enters a repeated-build loop.
The scheduling completion stamp advances after success even for identical bytes.

Every deletion, regeneration and unchanged-input case is followed by a no-op
assertion: no preprocessing, rpp, actual rustc or C++ tool invocation, and no
generated-file timestamp changes. Tracing covers every fixture build command;
generator-internal dependency/progress housekeeping is not a preprocessing or
compiler command. Clean followed by rebuild and another zero-command no-op also
passes. These are correctness results, not performance benchmarks.

### Dependency information

The original Make compiler .d co-output fix is preserved unchanged. The separate
preprocessing .d is likewise a required OUTPUT on Make. A selector change
introduces a newly discovered preprocessing input, builds once, then immediately
deletes dependency files before a no-op/consolidation pass and edits that input.
A data edit also tests the compiler dependency path. Regeneration, recompilation
and changed runtime results are required, followed by zero-command no-ops.
The existing baseline independently tests the original compiler-depfile loss fix.

The observations record actual on-disk depfile state for each generator. Here,
Ninja retained the emitter's raw preprocessing .d, but consumed the compiler's
raw .d files; Make retained both. No Ninja rule requires the compiler's raw .d
to exist, and the preprocessing .d is not declared a required Ninja output.
The tests delete existing raw and transformed .d files without deleting Ninja's
dependency log, then verify recovery by runtime behavior and command traces.
Normal edits and no-ops also work with consumed compiler depfiles already absent.
This does not assume all CMake/Ninja versions retain preprocessing depfiles.

### Incomplete trees and final recompilation

The helper invalidates completion before changing any payload, publishes files
individually through staging/rename, then writes a deterministic hash manifest
and matching completion marker. This is recoverable publication, not a claim of
an atomic directory transaction or power-loss durability.

The interruption barrier stops a real build after a changed generated root has
been published but before the remaining snapshot is complete. The whole build
process group is terminated. Ninja may delete modified declared outputs while
handling interruption; recovery cannot assume they survive. No compile or link
starts, and the old executable remains byte-identical. Direct adapter compile
and link calls also reject this incomplete tree before invoking rustc. Retrying
without clean restores a valid tree and executes the changed program.

A second probe corrupts a payload while leaving the old completion marker;
the content hashes still reject it. A marker's mere existence is insufficient.
The trace confirms that both the metadata compilation and the driver's actual
final-link recompilation use the generated crate root.

Snapshot checking here is a test adapter, not a production implementation. Later
driver work must bind the saved invocation to its snapshot identity and apply
the same validation before final recompilation. Full crash-boundary hardening
and actual Rust preprocessing semantics remain later milestones.

### Installed-only consumption and evidence

Each tool pair bootstraps from a temporary producer copy, installs and relocates
R++, then deletes that producer source and build directory before configuring
the unrelated consumers. Consumers use the installed modules/driver; the test
adapter and emitter are copied into their own fixture. Generated build recipes
are checked for references to the original repository. No production module,
compile/link rule, archive metadata policy or public interface changes.

The [versioned summary](baselines/v0.2-m0-custom-graph.json) records all required
deletion cases, both negative controls, timestamp/command observations, versions,
test-file hashes and four baseline results. Local full reports are
build-v02-m0/custom-graph-complete.json and
build-v02-m0/custom-graph-baseline.json. An explicit missing-tool run returned
BLOCKED and nonzero, never PASS.

**M0 feasibility passes for this scoped graph. M1 and production preprocessing
have not started; v0.2 tagging/publication remains unauthorized.** The existing
literal-include, explicit-source and diagnostic contracts and documented
macro-generated-read limitations are unchanged.
