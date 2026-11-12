# Experimental v0.1.0 release procedure

No tag or publication is performed by the release tools. Mandatory failures and
missing evidence block release approval; elapsed benchmark times do not.

## Frozen tools

The offline release gate pins CMake 3.25.0/4.4.5 and Rust
1.85.0/1.99.1/1.100.0. Release selection references:
https://doc.rust-lang.org/stable/releases.html and https://cmake.org/download/.
Reconfirm these references before approval; changes require an explicit inventory
update and new validation. Gate invocations require absolute installed paths.
The build and correctness gates never invoke Cargo. Qt/SQLite are system packages.

## Required evidence

Run `python3 tests/release_gate.py --help`, supply all five roles, and save its
JSON outside the source tree. Every pair requires the complete core and Qt gate:
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

Run the existing Cargo comparison sequentially, without concurrent build jobs,
with explicit Rust/Cargo 1.100.0 and CMake 4.4.5. Use five repetitions, first
instrumented and then `--timing-only`; retain JSON, counts and hardware metadata.
Rpp tracing redirects rustc calls only in the test driver wrapper so sysroot
pinning cannot bypass instrumentation. Never alter compiler version output.

## Candidate source and optional Arch package

Commit and sign all source changes. `python3 tests/source_bundle.py --output DIR`
requires a clean checkout and generates an explicitly unapproved candidate
archive, SHA256SUMS, manifest and checksum-filled PKGBUILD. No network is used.
The local PKGBUILD is optional, uses a preprovided archive and direct rustc, and
must be tested without makepkg dependency-install/download options.

Extract the archive into unrelated paths, compare a second generated archive's
checksum and rerun the release matrix and installed consumption there. Set
RPP_SOURCE_REVISION from the external manifest when running a matrix without Git
metadata; the archive checksum binds the report to the candidate. Build success
means functional reproducibility, not byte-identical compiler outputs.

## Approval and later tag procedure

Approval requires every mandatory gate PASS, including final archive validation,
visual signoff, path/archive/toolchain audits, signatures and benchmark evidence.
Record every report's source commit and archive SHA-256. Do not label an incomplete
candidate as a final release. Optional Arch packaging is reported separately.

Once approved, the later manual procedure is: verify the candidate commit signature,
verify evidence and asset checksums, create a signed annotated `v0.1.0` tag pointing
to that exact commit, verify the tag, and sign the finalized archive/checksum files.
Never move or overwrite an existing tag. Publish only through a separately selected
remote destination. This milestone prepares local assets and does not run those
commands. Use the existing configured signing identity; never disable signing.
