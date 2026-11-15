# v0.2 M1: Rust-aware lexical boundaries

M1 supplies an internal scanner, not a working preprocessor or a new rpp command.
Ordinary compile/link paths never invoke it. It performs no expansion, conditional
evaluation, source mirroring or diagnostic rewriting. The public Rpp language and
the M0 custom-command design remain unchanged.

## Internal interface

`src/lex` contains the token/span interface, scanner, Unicode property data and
unit tests. `scan(&str, Edition)` and `scan_bytes(&[u8], Edition)` return a borrowed
source with tokens, protected regions and source positions, or an explicit error.
Editions 2021 and 2024 are selected explicitly. Invalid UTF-8 is rejected before
scanning. No rustc internals, external Rust crates or Cargo are used.

Every token has a half-open byte span into the original UTF-8 input. Tokens form
a contiguous partition; concatenating their slices reproduces the exact input,
including BOM, CRLF, comments, suffixes and spacing. The scanner never normalizes
or rewrites bytes. Punctuation is represented as individual characters, with
original adjacency retained by spans, rather than performing parser-level
operator gluing. This also preserves macro token-tree punctuation structure.

`position(byte)` gives one-based line and Unicode-scalar column numbers, not
terminal display widths. LF starts a new line; CRLF counts as one newline. Initial
BOM and the CR of CRLF do not advance logical columns. Byte offsets always refer
to the unnormalized source, even where rustc internally removes a BOM or CR.
Invalid UTF-8 boundaries and out-of-range positions return None. This is source
position infrastructure, not a downstream rustc span-remapping implementation.

## Lexical coverage and errors

Recognized boundaries include identifiers/keywords, raw identifiers, lifetime
tokens, numeric literals and suffixes, character/byte literals, cooked/raw/byte/C
strings, line/doc comments, nested block comments and delimiter punctuation.
An initial shebang is preserved, allowing trivia before the bracket of an inner
attribute as rustc does. Rust 2024 guarded-string/hash reservations are distinct
from their Rust 2021 tokenization.

Errors contain a stable category and original byte span; delimiter errors also
identify an opening span. Checks include unterminated comments/literals, invalid
escapes, invalid scalar escapes, malformed character and byte literals, NUL in
C strings, raw delimiters over 255 hashes, malformed numeric forms, reserved
prefixes, unknown characters and malformed protected token trees. Protected
delimiter nesting is iterative and limited to 4096 levels, with an explicit error
rather than stack overflow. Results and errors are deterministic.

This is not a Rust parser or a complete compiler validator. Literal suffix type
checking, keyword grammar, ordinary block balancing, name resolution and Rust
semantics remain rustc's responsibility. In particular:

- Labels and lifetime parameters share the same lexical token. A colon does not
  distinguish a loop label from a lifetime bound; the scanner does not guess.
- Identifier tokens include keywords. M2 must apply its definition-name and
  substitution policy rather than treating this as Rust name validation.
- Unicode normalization is left to rustc. Distinct original byte spellings remain
  distinct slices, including decomposed identifiers.

The implementation follows the [Rust token grammar](https://doc.rust-lang.org/reference/tokens.html),
[input format](https://doc.rust-lang.org/reference/input-format.html) and
[edition reservations](https://doc.rust-lang.org/edition-guide/rust-2024/reserved-syntax.html).

## Protected regions

Attributes, macro_rules definitions and ordinary macro invocation paths and
balanced token trees receive protected spans. Matchers, metavariables, raw names,
nested invocations, comments and literals retain their original tokens. A helper
reports identifier-token eligibility without doing substitutions.

Only preprocessing outside protected constructs can recognize directives. Rust
macro token trees are still lexically checked: being protected does not make
unterminated strings or invalid Rust escapes valid.

An attribute's token tree is protected; the scanner does not resolve whether an
attribute name denotes a procedural macro or identify that macro's entire input
item. Procedural-macro integration remains deferred. Unstable declarative macro
2.0 definitions are not claimed as supported macro definitions. These require
grammar/name information, not guesses based on identifier spelling.

## Directive-position contract for M2

A directive candidate starts with # as the first non-space/tab byte on a physical
line, outside protected constructs and Rust non-code tokens. Optional spaces/tabs
may follow #, then an ASCII identifier name. Attributes remain Rust attributes;
hashes inside comments/strings/macros are never directive candidates. Non-ASCII
whitespace and a preceding comment do not count as a directive line prefix.

The entire candidate physical line is one opaque token, excluding its CRLF/LF;
the name has its own byte span. Unknown names are retained for M2 to diagnose.
There is no directive execution or argument validation. This deliberately permits
arbitrary #error message text containing quotes/apostrophes that would not be a
valid Rust expression. Candidates separate lexical macro-prefix recognition:
directives may surround complete macro constructs, not split a path/bang from
its delimiter and retroactively hide a directive.

M2 must validate definition replacements/expressions as complete Rust-token
fragments or expression grammar as appropriate. In particular, an unterminated
comment/string in a definition cannot silently continue across opaque physical
directive lines. A future fragment-scanning entry point must disable directive
recognition so replacement tokens cannot create new directives. No multiline
directive/line-continuation support has been added by M1.

## Unicode property provenance

Rust's standard library has no public XID_Start/XID_Continue query. Alphabetic
tests miss combining marks and other identifier characters. The checked-in
tables contain Unicode 18.0.0 property ranges only; the binary-search classifier
is project code. Rust's explicit exclusions for U+200C/U+200D are applied.

Ranges were extracted offline from the locally cached unicode-ident 1.0.27
`tests/tables/tables.rs`, whose header identifies UCD 18.0.0 and ucd-generate.
That input's SHA-256 is recorded in `src/lex/unicode.rs`. No crate implementation,
trie, generated algorithm, manifest or Cargo invocation is imported. The cache
and the generating package are not required to bootstrap, test or use R++.
The range data carries MIT AND Unicode-3.0 notices under LICENSES; installation
includes these notices. Project Rust code and the project remain GPL-3.0-or-later.

The property version is pinned independently of host Python/rustc Unicode data.
An older rustc may reject an identifier added in a newer Unicode release; this
scanner recognizes its complete boundary and leaves compiler acceptance to the
selected toolchain. It does not claim to reproduce every compiler version's
Unicode validity decisions. Updating the ranges requires explicitly supplied
offline Unicode property data, reviewing the version/provenance/notices and
rerunning the scanner and stable-toolchain matrix; builds never fetch tables.

## Tests and bootstrap

Rust unit tests run through the existing rustc --test executable. Tests cover
lossless/deterministic tokens, protected collisions, directive lookalikes, literal
families, Unicode ranges, CRLF/BOM positions, malformed inputs, deep nesting and
fixed-seed/prefix regressions. No external Rust test framework is needed.

`tests/lexer.py`, registered as lexer-regression in CTest, compiles a test-only
adapter with stable rustc. It compares valid corpus roundtrips and selected
malformed/edition cases against rustc, then checks actual driver/unit-test
bootstrap rebuilds and no-ops under all three generators. Cargo is forbidden by
a sentinel. Module edits rebuild both binaries, test-only edits rebuild tests
alone, and depfile deletion followed by a module edit recovers without clean.

The bootstrap explicitly depends on the production Rust modules; the test binary
also depends on test modules and emits dep-info. Combined link/dep-info output
lists the binary first under Rust 1.85.0 and the raw depfile first under 1.100.0.
CMake 3.25's Ninja handling requires a consistent primary target. Each bootstrap
command therefore runs a dep-info-only frontend pass, with its single raw .d as
the primary declared output, followed by a link pass. Both outputs have one owner
on all generators. These passes occur only when bootstrap inputs/outputs are
dirty; normal no-ops do not invoke rustc.

Both passes write staging paths. The binary is renamed into place after success,
then the dependency file is committed last. A dep-info-only pass can succeed on
a type-incorrect program, so publishing that primary dependency stamp before
the binary would let Make accept a failed build as current. The regression
injects type errors independently into production and test modules, requires
two consecutive builds to fail, preserves the prior binaries/stamp, then checks
recovery and no-ops without cleaning. Staging files are bootstrap artifacts,
not mirrored or transformed Rust sources.

rustc's depfile target paths are unescaped when absolute paths contain spaces:
output names are therefore relative to the explicit build working directory.
CMake owns the separate transformed depfiles. The bootstrap regression also
deletes each binary individually and requires successful recovery and no-ops.
These fixes address problems exposed under CMake 3.25 without changing native Rpp
compilation rules or the M0 fixture's different depfile format/ownership.

Reproduce with pinned installed tools:

```sh
cmake -S . -B build -G Ninja -DRPP_RUSTC_EXECUTABLE=/absolute/path/to/stable/rustc
cmake --build build
RPP_TEST_RUSTC=/absolute/path/to/stable/rustc RPP_TEST_CMAKE=/absolute/path/to/cmake \
  ctest --test-dir build -R 'driver-unit|lexer-regression' --output-on-failure
```

The complete existing compatibility matrix also runs this gate. The M0 installed
producer-copy fixture now copies LICENSES with the new modules; its graph and
negative-control assertions are unchanged. M2 is not implemented or started.

## M1 validation (2026-11-15)

| CMake | Stable rustc | Result |
| --- | --- | --- |
| 3.25.0 | 1.85.0 | all six CTest gates passed |
| 3.25.0 | 1.100.0 | all six CTest gates passed |
| 4.5.0 | 1.85.0 | all six CTest gates passed |
| 4.5.0 | 1.100.0 | all six CTest gates passed |

Each pair includes the 24 Rust unit tests (21 lexer tests), lexer-regression,
driver publication failures, toolchain errors, generator compatibility and
Qt/SQLite installed consumption. Native projects retain three-generator,
Debug/Release coverage. Lexer corpus tests exercise both editions; bootstrap
tools use configuration-independent flags and are checked on all generators.
The original M0 native co-output negative control was rerun on both CMake
versions and retained its expected Ninja failures/Make recovery. Supplemental
checks roundtripped all 16 repository Rust files in both editions and compared
BOM/CRLF/Unicode/tab source columns with stable rustc JSON on both compiler lines.

[Versioned evidence](baselines/v0.2-m1-scanner.json) records tool identities,
source hashes, coverage and the explicit lexical boundaries above. This is
development-milestone evidence; a clean, signed v0.2 candidate will still need
the later release gates. No tag, publication or M2 work is performed by M1.
