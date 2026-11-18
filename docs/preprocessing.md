# v0.2 M2: directives and object-like expansion

M2 implements an internal, standard-library-only Rust library in
`src/preprocess`. It does not add a public `rpp preprocess` command, change
compilation or linking, mirror a source tree, write files, or enable preprocessing
in CMake. Those are later milestones. Ordinary v0.1 builds continue through the
existing driver unchanged. The library uses the [M1 scanner](lexer.md).

## Library boundary

Callers supply `Sources`, an explicit map of logical relative paths to UTF-8
text, an entry path, and `Options`. The options select Rust 2021 or 2024 (default
2024), ordered include directories, and ordered initial define/undef operations.
The library performs no filesystem access, compiler invocation or dependency
fetching. Every call starts with a fresh macro environment; processing a second
Rust module or Rust `include!` file will require a separate call with the same
initial options. Textual preprocessing includes share the current environment.

The result contains generated text, byte-range mappings and a sorted set of
actually processed input paths. Unselected includes do not become dependencies.
Errors return no successful partial result. Publication and preserving previous
on-disk outputs belong to M3/M4, not this library.

Logical paths are normalized and checked for absolute paths, root escape and
duplicate aliases. Include directories can name the logical root (`.`).
Physical filesystem validation, symlink escape checks, destination collisions
and Rust source/data declarations remain mandatory M4 work. Logical validation
is not a filesystem sandbox.

## Directives

The scanner recognizes `#` only as the first non-space/tab character on a
physical line, outside protected tokens. Spaces/tabs may separate `#` from an
ASCII directive word. Attributes `#[...]`/`#![...]`, literals, comments, Rust
macro definitions and invocation paths/token trees are protected. An initial
BOM and shebang are preserved in the entry file. Included fragments' initial
BOM/shebang bytes are blanked as fragment metadata, retaining line endings.

```text
directive := #define NAME replacement?
           | #undef NAME
           | #include "relative-path"
           | #if expression | #elif expression | #else | #endif
           | #ifdef NAME | #ifndef NAME
           | #error literal-message
NAME      := [A-Za-z_][A-Za-z0-9_]*
```

Names are case sensitive. `#define` requires a single ASCII name followed by
optional replacement text. An immediately adjacent `(` after that name is a
rejected function-like definition; a spaced `(` begins an object-like replacement.
`#undef`, `#ifdef` and `#ifndef` require exactly one name. `#else` and `#endif`
allow no non-comment trailing tokens. Arguments permit ordinary Rust whitespace
and comments where tokenization is required. `#error` takes the trimmed remainder
literally, with no Rust tokenization or macro expansion.

There is no backslash continuation, function-like expansion, token pasting or
stringification. Unknown directives error in active regions. Replacement text
is never rescanned as a directive, even when it introduces a line-leading `#`.

Conditional nesting is file local. Duplicate/unmatched branches, `#elif` after
`#else` and unclosed conditionals error. Structural directives are checked even
inside inactive regions: branch expressions must be nonempty, name arguments
must be valid, and `#else`/`#endif` must have no trailing tokens. Ineligible
`#if`/`#elif` expressions are not expanded or evaluated. Other inactive directive
bodies are ignored entirely, including definitions, unknown directives, includes
and `#error`. Ordinary skipped Rust text must still be lexically traversable;
an unterminated literal/comment/macro tree is an error.

## Expansion

An eligible occurrence is a complete ordinary identifier outside scanner-protected
regions. Rust paths participate; raw identifiers, lifetimes, labels, literal
suffixes, Unicode identifier substrings, strings and comments do not. Original
Rust macro definitions, invocation paths and token trees, and attributes retain
their tokens. This deliberately differs from traditional C preprocessing.

Replacement bodies are fragment-tokenized when defined. Comments become
same-byte-width spaces and leading/trailing whitespace is removed. This prevents
a definition's `//` from commenting out code at its use site. Identical normalized
replacement bytes may be redefined; different bytes require `#undef`. Internal
whitespace remains significant for redefinition, unlike C's token comparison.

Bodies are expanded at use time, with the current environment. Forward references
and later redefinitions therefore work. An undefined identifier remains unchanged
in code. Empty definitions disappear, with separators retaining token boundaries.
Each insertion has surrounding spaces: adjacent identifiers, literal suffixes,
operators and comment openers cannot form by implicit token pasting.

Expansion proceeds left to right, rescanning after each replacement. A replacement
that forms a Rust macro invocation with the surrounding tokens in that code
segment protects its invocation before further substitutions. Recursion is
tracked per expansion ancestry, not by a global already-expanded-name set.
Direct/indirect cycles error with the chain, use site and definition notes;
separate occurrences may expand independently. Expansion does not retroactively
reprocess text already emitted before a directive or an included file. Rust macro
syntax synthesized across those processing boundaries must not be assumed to
have the protection of an originally written complete macro invocation.

This implementation favors correctness over throughput: repeated leftmost
expansion rescans the current segment and can be quadratic for many independent
uses. M2 makes no preprocessing performance claim and adds no optimization gate.

## Conditional expressions

```text
expression := integer | NAME | defined NAME | defined(NAME)
            | (expression) | !expression | +expression | -expression
            | expression comparison expression
            | expression && expression | expression || expression
comparison := == | != | < | <= | > | >=
```

Precedence, highest first: unary; relational; equality; `&&`; `||`. Binary
operators associate left. Results are signed 64-bit integers; comparisons and
logical operations yield 0/1. There is no binary arithmetic, bitwise operation,
ternary, float, integer suffix or Rust expression evaluation. Unsuffixed decimal,
hex (`0x`), octal (`0o`) and binary (`0b`) literals accept Rust digit separators.
Unary minus permits the signed minimum `-9223372036854775808`; checked negation
rejects overflow. Literal magnitudes greater than 2^63 are always rejected.
The positive 2^63 intermediate is accepted only where evaluation can consume it
under unary minus, or where short-circuiting suppresses value evaluation.

Object macros expand first, except the `defined` operator and its name operand.
`defined` is reserved as an operator in expressions. Remaining ASCII names
evaluate to zero. Empty replacements where an operand is needed are errors.
Expression syntax is parsed in full, but `&&`/`||` skip unnecessary value
evaluation (including checked-negation overflow). This does not suppress macro
expansion cycles: `0 && RECURSIVE` still fails during expansion.

## Preprocessing includes

Only an unescaped, nonempty quoted relative filename is accepted. No macro path
expansion, raw string, absolute path, backslash, NUL or multiline filename is
supported. Search the including file's directory first, then explicit include
directories in order, selecting only an explicitly supplied input. Normalized
root escapes fail rather than being silently skipped. Missing inputs report the
original include directive and include stack.

An included file executes in textual order with shared definitions. It must close
its own conditional blocks. Repeated nonrecursive inclusion works, including
ordinary guards. Active-stack cycles fail before processing the repeated file;
guards do not license cycles. Rust `include!`, `include_str!` and `include_bytes!`
remain protected ordinary Rust macros. M2 neither discovers their inputs nor
validates their paths. Source mirroring and compiler depfile auditing remain M4/M5
work. Rust resolution of text inserted by a preprocessing include uses the
receiving Rust file's context, not the fragment's directory.

## Limits and source provenance

| Resource | Limit |
| --- | --- |
| Active included files, counting entry | 128 |
| Conditional nesting per file | 256 |
| Macro expansion ancestry | 128 |
| Expression prefix/parenthesis nesting | 256 |
| Generated bytes per preprocessing call | 64 MiB |

Excess produces an ordinary error, never success with truncated output. Include
processing uses an explicit heap stack; deeply nested valid includes must not
overflow a Rust test thread's stack. Expression parsing does not construct a
recursively dropped AST for long flat operator chains.

Mappings cover generated bytes in order without gaps. Copied ranges have exact
original byte offsets. Blanked ranges and replacements are explicitly inexact.
Replacement provenance retains the original use-site range and definition chain;
included ranges retain their include stack. Columns count Unicode scalar values,
not UTF-8 bytes, grapheme clusters or display width. Raw byte spans preserve CRLF;
column computation excludes a leading BOM and the CR of CRLF.

These are internal provenance records, not a versioned on-disk source-map format
or downstream rustc diagnostic rewriting. Ambiguous generated ranges must not
be advertised as exact. M3 owns serialization; M5 must demonstrate the required
compiler diagnostic mapping separately using M0's JSON probes.

## Regression validation

The existing `driver-unit` gate now covers expansion, integer/operator boundaries,
short-circuit syntax/value separation, all directives, inactive regions, include
order/cycles, limits, deterministic results and original Unicode/CRLF locations.
`preprocess-regression` compiles a test-only adapter directly with selected stable
rustc, compiles and runs generated Rust in both editions, checks Rust macro and
include preservation, compares repeated text/maps/dependencies, and checks
failure isolation and nested original error locations. This adapter is not a
public command or a production filesystem layer.

`lexer-regression` extends bootstrap edit checks to preprocessing implementation
and test modules, retaining dependency-loss, failed-code-generation recovery,
deleted-binary and no-op assertions on all three generators. Native integration,
Qt/SQLite and installed consumers continue through existing gates. The
[M0 output ownership prototype](preprocessing-graph-design.md), including its
negative controls, is unchanged. Later integration must use that validated
separate custom-command graph, not the original compile co-output proposal.

For focused validation (pin a stable compiler; the system default may be nightly):

```sh
cmake -S . -B build -G Ninja -DRPP_RUSTC_EXECUTABLE=/path/to/stable/rustc
cmake --build build
RPP_TEST_RUSTC=/path/to/stable/rustc ctest --test-dir build \
  -R 'driver-unit|preprocess-regression|lexer-regression' --output-on-failure
```

## M2 validation (2026-11-18)

| CMake | Stable Rust | Result |
| --- | --- | --- |
| 3.25.0 | 1.85.0 | all seven CTest gates passed |
| 3.25.0 | 1.100.0 | all seven CTest gates passed |
| 4.5.0 | 1.85.0 | all seven CTest gates passed |
| 4.5.0 | 1.100.0 | all seven CTest gates passed |

Each pair runs 43 Rust unit tests: 18 preprocessing, 22 lexical and three existing
driver tests. The generated-Rust oracle covers both editions. Existing native
gates retain Ninja, Unix Makefiles, Ninja Multi-Config, Debug/Release, parallel
builds, dependency loss, failure recovery and installed consumers. Qt/SQLite
installed validation passes unchanged. Bootstrap changes to preprocessing modules
rebuild both binaries; preprocessing test-only edits rebuild the test binary.

The [versioned development baseline](baselines/v0.2-m2-preprocessing.json) records
tool identities, CTest results and source hashes. The complete M0 graph matrix is
also rerun with both negative controls retained. Historical M0/M1 baselines are
unchanged. This does not substitute for M8's clean signed release-candidate gates;
no M3 command, production integration, tag or publication is included in M2.
