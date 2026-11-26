# Experimental v0.2.0 release procedure

No tag or publication is performed by the release tools. Mandatory failures and
missing evidence block release approval; elapsed benchmark times do not.

## Frozen tools

The offline release gate pins CMake 3.25.0/4.5.1 and Rust
1.85.0/1.99.1/1.100.0. The frozen inventory was checked on 2026-11-26.
CMake 4.5.0 remains part of the historical M0 graph replay. An inventory
change requires new candidate validation.
[Kitware release](https://github.com/Kitware/CMake/releases/tag/v4.5.1) ·
[Rust release announcements](https://blog.rust-lang.org/releases/)
Gate invocations require absolute installed paths.
The build and correctness gates never invoke Cargo. Qt/SQLite are system packages.

## Required evidence

Run `python3 tests/release_gate.py --help`, supply all five roles, and save its
JSON outside the source tree. Pass `--source-manifest` with the bundle manifest
next to the source archive. The runner verifies its checksum and requires all
eleven CTest gates in each pair. Each pair exercises
three generators, both configurations, dependencies, no-ops, recovery, installation
and external consumption. Only stable compilers are allowed. Missing tools are
BLOCKED; executed failures are FAIL. Keep historical reports unchanged.

Driver failure tests exercise direct/response/mixed argument forms and Linux
process-group termination after partial output, followed by retry without clean.
This does not yet establish interruption safety at every publication boundary.

The manual Qt check requires a tester, date, exact candidate and environment:
launch RppNotes, create/edit/delete Unicode notes, restart to check persistence,
resize the window, and inspect error presentation. Offscreen results cannot stand
in for this visual signoff. Keep it BLOCKED until performed.

Record the full source revision, source archive SHA-256, executable SHA-256,
source/build paths, configuration, CMake/rustc versions, Qt/SQLite versions,
operating system and desktop/display backend. Build the visual application from
that extracted archive, not an unspecified existing build. Record the tester and
actual test date. Use a fresh application data directory; create a note containing
`Grüezi — 日本語 🦀`, edit it to `Überarbeitet — 中文 🦀`, restart with the same
data directory and verify the exact text, delete it and check it stays deleted
after restart. Verify the note count updates in the enabled build and stays
hidden in the default build. Record resizing and the database-open error with disabled controls
in a separate deliberately unwritable data directory. Each observation must be
explicitly confirmed; an unperformed check remains BLOCKED. Store this record
beside the candidate evidence. Previous conversational observations without the
candidate/environment/Unicode details do not satisfy this gate.

Run the existing Cargo comparison sequentially, without concurrent build jobs,
with explicit Rust/Cargo 1.100.0 and CMake 4.5.1. Use five repetitions, first
instrumented and then `--timing-only`; retain JSON, counts and hardware metadata.
Rpp tracing redirects rustc calls only in the test driver wrapper so sysroot
pinning cannot bypass instrumentation. Never alter compiler version output.

## Candidate source and optional Arch package

Commit and sign all source changes. `python3 tests/source_bundle.py --output DIR`
requires a clean signed checkout and generates `rplusplus-0.2.0.tar.gz`, a
checksum, a candidate manifest and a checksum-filled PKGBUILD. No network is used.
The local PKGBUILD is optional, uses a preprovided archive and direct rustc, and
must be tested without makepkg dependency-install/download options.

Extract the archive into unrelated paths, compare a second generated archive's
checksum and rerun the release matrix and installed consumption there. Run the release gate from the extracted source with the original manifest.
Record that revision and archive SHA-256 in the supplemental path, archive,
benchmark and visual evidence as well. Build success
means functional reproducibility, not byte-identical compiler outputs.

## Approval and later tag procedure

Approval requires every mandatory gate PASS, including final archive validation,
visual signoff, path/archive/toolchain audits, signatures and benchmark evidence.
Record every report's source commit and archive SHA-256. Do not label an incomplete
candidate as a final release. Optional Arch packaging is reported separately.

Once approved, the later manual procedure is: verify the candidate commit signature,
verify evidence and asset checksums, create a signed annotated `v0.2.0` tag pointing
to that exact commit, verify the tag, and sign the finalized archive/checksum files.
Never move or overwrite an existing tag. Publish only through a separately selected
remote destination. This milestone prepares local assets and does not run those
commands. Use the existing configured signing identity; never disable signing.

The local handoff has one upload directory, one authoritative signed
`SHA256SUMS`, and a publication file generated from the manifest's upload list.
The GitHub template supplies the release body and stays in the local handoff.
