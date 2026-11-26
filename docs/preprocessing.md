# Rust-aware preprocessing

## Standalone command (v0.2 M3)

Run `rpp preprocess --help` for the command and options. For example, with
`#include "settings.rph"` in `app/main.rs`:

```sh
rpp preprocess app/main.rs -o generated.rs \
  -I app/config --input app/config/settings.rph \
  -DDEBUG --edition 2024 --depfile generated.d --source-map generated.json
```

Paths on the command line are relative to the working directory. The entry’s
directory is the default source root. Use `--source-root` to pick a wider
root for inputs. Every preprocessing fragment must be declared with
`--input`. The entry, fragment paths and include directories must stay under
the root, including when resolving symlinks. All declared files must be
readable UTF-8. Only actively used fragments appear in the depfile.
Plain `-DNAME` sets a value of `1`. Definitions and undefinitions are applied
in argument order. To pass a filename starting with `-`, use `./` or place the
entry after `--`. The driver accepts its existing NUL-separated UTF-8
response files for the whole command or part of it.

Use single-file mode for Rust files that can compile without separate Rust
modules or Rust include files. Inline modules and preprocessing `#include`
fragments work. The command flags external `mod` declarations and visible
`include!`, `include_str!` and `include_bytes!` calls, including calls nested in
attributes or macro arguments. This is a lexical check, including when a
user-defined macro has one of those names. Rust macro definitions are opaque
to the check. Reads introduced by Rust macro expansion require the later
source-tree and compiler integration. Use the command explicitly to generate
a file for a separate compiler invocation.

Exit status is 0 for success, 2 for argument misuse, and 1 for preprocessing,
validation or I/O errors. Errors go to stderr. `--diagnostic-format json` emits
one object for operational errors after argument parsing. For source
diagnostics it includes the kind, message, location, notes and include stack.
Argument syntax and response-file read errors use human diagnostics.

The source map is JSON with `version: 1`, the generated path and a list of
mappings. Byte ranges are zero-based and half-open. Original file paths are
relative to the source root. Original line and scalar column are one-based.
Definition and include provenance retain their original locations. The
depfile uses paths relative to the working directory where possible, with
absolute paths otherwise. GNU Make escaping covers spaces, hashes and dollar
signs. Paths with ambiguous Make syntax, such as backslashes, colons, percent
signs and newlines, are rejected when writing a depfile.

Output directories must already exist. Existing regular output files can be
replaced. Byte-identical files retain their timestamps. The command stages
every changed file before replacing any destination, then publishes the
optional sidecars before the Rust file. Ordinary failures preserve the
previous Rust output and clean pending temporary files. If a rename fails
after a sidecar was replaced, rerun the command to regenerate the set.
Publishing several paths takes several renames. A process termination can
leave a temporary file behind.

M3 passed all eight CTest gates with CMake 3.25.0, 4.5.0 and 4.5.1, each
paired with stable Rust 1.85.0 and 1.100.0. The installed command also ran
from a separate directory with an empty `PATH`.
[Compatibility evidence](baselines/v0.2-m3-preprocess-cli.json)

## Declared source trees (v0.2 M4)

```sh
rpp preprocess app/src/main.rs --source-root app --output-dir app/build/generated \
  --source app/src/worker.rs --source app/src/embedded.rs \
  --data app/assets/banner.txt --input app/src/settings.rph \
  --depfile app/build/generated/.rpp/tree.d
rustc --edition=2024 app/build/generated/src/main.rs -o app/build/example
```

The positional input is the entry Rust file. Repeat `--source` for additional
Rust files, including Rust `include!` inputs. Declare preprocessing fragments
with `--input` and assets with `--data`. Each Rust file starts with the same
command-line macros. Preprocessing `#include` shares the receiving file's macros,
while Rust `include!` reads a separately preprocessed file. Assets retain their
original bytes. All paths on the command line are relative to the invocation
directory. If supplied, `-o` must name the implied mirrored entry.

The output directory can be a dedicated subtree of the source root. Declared
inputs and include search directories cannot be inside it. Tree mode rejects
conflicting input roles and aliases, root and symlink escapes, and unowned files
in the output directory. Choose a fresh output directory if a previously
generated file path needs to become a directory or vice versa. Use one writer
per output directory.

Direct Rust `include!`, `include_str!` and `include_bytes!` calls accept a
single cooked or raw string literal and an optional trailing comma. The path is
relative to the receiving Rust file. Declare the path as `--source` for
`include!` and `--data` for the other two. Computed arguments, including
`concat!` and `env!`, require conversion to a literal or an ordinary Rust build.

Rust still chooses module files. Tree mode checks visible external `mod`
declarations and `#[path]` attributes, including conditional alternatives,
for possible data/source role conflicts. The check can reject a data file in
another directory if its name matches a module candidate, or if its path
matches an attribute suffix whose base is ambiguous. Unicode identifier matches
are conservative. Use a nonconflicting asset path or declare the file as Rust.
Parent traversal in path attributes is allowed when its base is statically
bounded. In nested or included contexts, a relative path must stay beneath
its base without assuming how far that base is from the root. Rust `cfg`
evaluation and unresolved-module errors are handled by `rustc`. Reads introduced
through Rust macro expansion are outside the lexical checks. See the
[M0 dependency-reporting limits](preprocessing-m0.md).

The default aggregate map is `.rpp/source-map.json`. It contains a `sources`
array of M3 mapping records with generated paths relative to the output
directory. Override the map with `--source-map FILE`. The optional `--depfile FILE`
lists all declared Rust files and assets and the preprocessing fragments
actually read. Its target is `.rpp/complete`. Both sidecars must be inside
the output directory.

The versioned manifest records the entry, optional depfile path and SHA-256
hashes of the generated Rust, assets and map. The completion marker identifies
that manifest. The ownership record tracks both prior and pending output paths
during an update. Once publication begins, R++ invalidates the marker, replaces
changed files through sibling staging files, removes stale owned paths, and
publishes completion last. Rerun the same command to recover after an
interruption. Validation failures before publication preserve the prior
snapshot. An unchanged run preserves file timestamps, and recreating a
consumed depfile leaves the completion marker unchanged. Snapshot integrity
covers the payload and map. The depfile is disposable.

M4 passed all nine CTest gates for CMake 3.25.0 and 4.5.0 with stable Rust
1.85.0 and 1.100.0. The suite compiles the standalone mirror after removing
access to the original sources and verifies interruption and deletion recovery.
The M0 graph replay also passed, including its original failure controls.
[Compatibility evidence](baselines/v0.2-m4-preprocess-tree.json)

## Preprocessing language and internal library (v0.2 M2)

M2 implements the pure, standard-library-only Rust library in `src/preprocess`
using the [M1 scanner](lexer.md). M3 uses the library from the standalone
command. Compilation, linking and CMake keep their existing entry points.

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
The command validates the selected filesystem paths and declared roles. Rust
macro expansion and compiler reads remain outside these lexical checks.

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

These internal provenance records feed the M3 source-map serializer. Ambiguous
generated ranges stay marked inexact. M5 must demonstrate downstream compiler
diagnostic mapping separately using M0's JSON probes.

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
