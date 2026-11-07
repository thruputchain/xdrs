# Design: `xdrc`, an XDR compiler in Rust

Status: proposal, under discussion.

## Summary

Write an XDR-to-Rust compiler in Rust and add it to this workspace. It would
replace the Rust backend of `x23c`, the C++ compiler in hogd's `x23/x23c`,
which generates the Rust types in thruput-protocol-rs from the `.x` files in
thruput-protocol.

x23c would remain for its C++ (`gen_hh`) and TypeScript (`gen_ts`) backends.

## Background: x23c today

- **Size:** about 4,800 lines of C++23 with flex/bison:
  - front end (lexer, grammar, driver, AST in `defs.h`): about 1,100 lines
  - `gen_rs.cc`: 1,438 lines
  - `gen_ts.cc`: 1,258 lines
  - `gen_hh.cc`: 874 lines
- **Dialect:** RFC 4506 XDR plus:
  - the RFC 5531 `program`/`version` syntax
  - `namespace`
  - `%` literal pass-through lines
  - Rust markers carried in comments: `#[rust(box_ptr)]`, `newtype`,
    `const=u32`, `const=u64`, `ConstWriteXDR`, `no_debug`, `no_eq`
- **CLI options:** `--rust-feature`, `--rust-no-feature`, `--recursive_type`.
- **Change history:** of the 35 commits to x23c since May 2026, 22 touch
  `gen_rs.cc`, 5 `gen_hh.cc` and 7 `gen_ts.cc`. None touch `parse.yy`. The
  grammar is stable; the Rust backend is what changes.
- **Other consumers:** hogd's CMake runs x23c during its own build, including
  `built_contracts`, to produce C++ headers. gen-ts emits
  `@thruputchain/xdr` imports for the TypeScript SDK.

## Motivation

### Version skew across repositories

The generated Rust is written against specific xdrs and thruput-rpc-api APIs,
but the generator lives in a different repository:

- hogd's `x23/tests/rust_check` builds the generated code against a released
  xdrs, so an xdrs change reaches it only after a release. It was on 0.17
  until recently. A breaking change to a trait that generated code
  implements, such as `XDRDiscriminant`, goes unnoticed in both repositories
  until protocol-rs upgrades.
- protocol-rs's `scripts/generate.sh` downloads the unpinned `latest-x23c`
  release from hogd. Any x23c merge can change protocol-rs's regenerated
  sources and break its "Generated sources" CI check.
- Features land as paired PRs across the two repositories. For example, the
  explicit tag on a union's default variant is #376 here and hogd#4740.

### xdrs tests against a hand-written copy of x23c's output

`rpc/rpc-api-fixture` is a hand-written copy of the shape x23c emits for RPC
programs. With the compiler in this workspace, tests can generate from `.x`
fixtures and compile against the in-tree xdrs. Codegen and runtime would then
change in the same PR.

### Output quality

`gen_rs` builds code by concatenating strings. Visible results in
protocol-rs:

- `use ::serde::{{Serialize, Deserialize}};` (a leftover `std::format` escape)
- `#[doc(hidden)]pub const ...`
- empty `// ` comment lines
- a `Generated Code: DO NOT CHECK IN` header on files that are checked in
- a `perl -pi` fix-up in `generate.sh` for an unescaped `ContractAddress[32]`
  that rustdoc reads as an intra-doc link

Generating with `quote` and printing with `prettyplease` produces
syntactically valid, consistently formatted output by construction.

### Tooling

protocol-rs could install or run a pinned `xdrc` through cargo, instead of
downloading a C++ binary built for each OS and architecture by hogd's release
workflow.

## Scope

In scope:

- a parser for the x23c dialect, including comment attachment and the
  `#[rust(...)]` markers
- a Rust backend covering everything `gen_rs` emits today:
  - types and constants
  - serde, schemars and xdrs derives behind features
  - `Display` for enums
  - union accessors
  - RPC service traits, dispatchers and clients for thruput-rpc-api
- a CLI that accepts the same flags as `x23c --rust`, so `generate.sh` changes
  only the command it runs

Out of scope:

- C++ and TypeScript backends. Porting them would make hogd's C++ build depend
  on cargo, for little gain.
- Go (thruput-protocol-go is generated separately).
- Byte-for-byte parity with x23c's output.

## Proposed shape

- `xdr-codegen`: a library crate holding the parser, AST and Rust backend.
- `xdrc`: a thin binary over it.
- Generated code stays checked in to protocol-rs. That keeps diffs reviewable
  and the crates publishable. A `build.rs` entry point can come later if
  wanted.

## Costs and risks

- **Licenses differ.** hogd's `x23` tree, which includes x23c and its `.x`
  test files, is GPL-3 (`x23/COPYING`, copyright David Mazières). This
  workspace is Apache-2.0. xdrc emulates x23c's behavior and output, and its
  development consults x23c's source. Whether that makes xdrc a derivative
  work of x23c has not been settled. One thing is copied from `x23`, by the
  team's decision: `xdr-codegen/check` contains hogd's
  `x23/tests/rust_check` fixtures (`check.x`, `plain.x`) and its test
  `lib.rs`. The corpus test reads other `.x` files in place, from
  directories named by `XDRC_CORPUS`.
- **Two parsers for one dialect.** x23c keeps its parser for C++ and TS.
  Because the grammar has been stable, the risk of drift is low. A shared
  corpus of `.x` fixtures that both compilers must accept would keep them
  aligned. The corpus would include all of thruput-protocol and hogd's
  `x23/tests/*.x`.
- **Comment handling.** x23c separates a file's header comment from the
  comments that follow, attaches each comment to the definition after it, and
  carries end-of-line comments onto fields and enumerators. These become
  rustdoc, so they show in the output. `quote` drops comments, so they must be
  emitted as `#[doc = ...]` attributes.
- **One large regeneration diff.** Switching protocol-rs produces one big diff.
  Check it for equivalence by:
  - building and testing protocol-rs with the new output
  - diffing its public API, through rustdoc JSON or `cargo public-api`
  - encoding sample values with the old and new types and comparing the bytes
- **RPC glue** is a meaningful share of `gen_rs`. It is tied to
  thruput-rpc-api, which lives in this workspace.

## Estimate

- front end: about 700–1,000 lines of Rust
- Rust backend: about 1,500–2,000 lines, plus fixture tests
- total: a few focused weeks, including the protocol-rs migration

## Plan

0. Interim mitigation, independent of this proposal:
   - pin the x23c release version in protocol-rs's `generate.sh`
1. Parser and AST, tested against the full `.x` corpus.
2. Rust backend, starting from rust_check's `check.x` and `plain.x` fixtures.
   Replace `rpc/rpc-api-fixture` with generated code.
3. Generate protocol-rs with `xdrc`. Verify equivalence as described above,
   then switch `generate.sh` and CI.
4. Remove `gen_rs` from x23c, and the `--rust` option with it.

## Decisions

### Hand-written lexer and parser

The lexer and the recursive-descent parser are written by hand, with no
parsing dependencies. The lexer is a table of regular expressions copied
from x23c's flex rules and matched with `regex-automata` (see below).

- **The grammar is small and LL(1).** `parse.yy` has about 30 rules. Only a
  few constructs need care, and each needs at most one token of lookahead:
  - a bare `unsigned`, accepted with a warning
  - the separate `opaque` and `string` declaration forms
  - a `case` list followed by `default`
- **Comments carry meaning.**
  - Block comments are attached to the definition that follows.
  - End-of-line comments are attached to the token before them.
  - A file's header comment is kept separate.
  - The `#[rust(...)]` markers live inside comments.

  As in x23c, the lexer produces tokens with comments already attached, and
  the parser reads that stream. Parsing libraries treat comments as
  whitespace to skip, so the attachment step would be ours to write either
  way.
- **Diagnostics.** x23c reports several RFC deviations as warnings and keeps
  going: a trailing comma, lowercase `true` or `false` in a case, `unsigned`
  without `int`. `--strict` turns these into errors. Code written by hand
  handles warnings with exact source positions directly.
- **Cost.** The estimate is 500–700 lines, with no build-time code
  generation and no added compile time.
- **The lexer as a rule table.** At first the lexer was written as one
  matching function per rule. That code was harder to read and to change
  than x23c's `lex.ll`, where each rule is one line and flex does the
  matching. The lexer was therefore rewritten as a table of regular
  expressions with flex's semantics, using `regex-automata`: at each
  position, the longest match wins, and ties go to the earlier rule. As in
  flex, the rules are compiled into one DFA, whose match states record which
  rules match.
  - `regex-automata` is a regex engine, not a parser generator, and it was
    already in the workspace's dependency graph.
  - Building the DFA takes a moment at startup, which does not matter for
    a code generator.

Alternatives considered:

- **lalrpop:** closest to bison, but adds build-time code generation, and
  comment attachment still sits outside it.
- **pest:** produces an untyped parse tree that we would convert to the AST
  by hand anyway.
- **chumsky:** has the best error recovery, but brings heavy generics, slow
  compiles and an API that has changed a lot between versions.
- **winnow and nom:** save little over plain code for a grammar this size.

A library would be worth revisiting if we wanted an editor language server
with error recovery.

The parser is tested against all of thruput-protocol and hogd's
`x23/tests/*.x`, and must reject what x23c rejects.

### The compiler has its own AST and lowers to `xdr-repr`

There are three representations of a protocol type:

1. **Source `.x`.** This is the source of truth for the protocol. It carries
   typedef names, named constants used as bounds
   (`opaque serialized_event<kMaxEventLength>`), enumerator names, comments,
   markers and programs.
2. **Generated Rust types.** Their wire format is decided by how xdr-derive
   reads Rust shapes and `#[xdr(...)]` attributes.
3. **The spec that `#[derive(XDR)]` rebuilds from those Rust types.** It is
   built through `<FieldTy as Declare>::declare(...)`, so it sees only what
   the Rust types carry:
   - Named bounds become plain numbers.
   - Rust type aliases such as `pub type GasAmount = u64` disappear.
   - An XDR enum emitted as a newtype loses its enumerators.
   - Comments are gone.

   It describes the same wire format as the source, but it is a smaller
   spec.

xdr-repr serves the Rust-first direction, where Rust is the source and XDR is
derived from it, as in contract specs. The compiler serves the XDR-first
direction.

**The compiler has its own AST rather than using xdr-repr's types.** As a
parser output, xdr-repr lacks:

- source positions
- comments and markers
- `%` literals
- `namespace`, except behind a feature
- constants wider than `i32`/`u32`
- `Clone` and `Debug` on its RPC types

It also panics on invalid identifiers rather than returning errors. Adding all
of this would bloat a `no_std` crate that proc macros and runtime code depend
on, and comments would start to count toward its equality.

**The AST lowers to `xdr_repr::Specification`,** dropping comments, markers
and positions. That gives:

- **A round-trip check between codegen and derive.** Lower a `.x` file to
  xdr-repr (A). Generate Rust from the same file with the `spec` feature,
  compile it, and call `::spec()` (B). Check that A and B describe the same
  wire format. Today nothing checks that the Rust generated from XDR and the
  XDR derived back from that Rust agree.
- **A canonical printed form** of any `.x` file, through xdr-repr's
  `Display`. It drops comments, so it is a canonical form, not a formatter.

**"Same wire format" is defined by a normalizer in xdr-repr.** It:

- resolves typedefs
- replaces named constants with their values
- treats an enum as `int`
- replaces inline types with their bodies
- ignores names

Names may count later if JSON and serde names become part of the contract.
The check may also encode sample values with both sides.

**The derived spec only has to match the source on the wire.** It
does not have to reproduce real `enum`s or named bounds. protocol-rs
generates with `--rust-no-feature spec`, so nothing consumes `::spec()` from
protocol types today. If a consumer appears, the generated Rust can carry
more information, for example through attributes.

## Unions and enums

How xdrc emits unions and enums depends on recent xdrs work:

- **#375** names a union's switch type with `#[xdr(discriminant = "T")]`, by
  default `i32`. The type decides how the discriminant is encoded, which
  differs under CXS, and the type of the default arm's field.
- **#379** makes `derive(XDR)` describe the switch by that type.
  `SwitchType` labels each case: `i32` and `u32` by number, `bool` as
  `TRUE`/`FALSE`, and an enum by member name.

xdrc does not depend on any xdrs change beyond these.

Current x23c already follows #375. As of hogd#4724, #4738 and #4739, which
protocol-rs#366 generates with, it:

- emits `discriminant = "<switch type>"` on every union
- writes an `XDRDiscriminant` impl for every XDR enum newtype
- types the default variant's first field with the switch type
- writes case values typed by the switch type, cast to the `i32` tag
- gives the default variant a tag that no case uses

xdrc's starting point is to match this, not to improve on it.

### XDR enums stay open newtypes

xdrc emits an XDR enum as x23c does: `struct E(pub i32)`, with the members as
associated constants. Decoding then accepts a member that this code does not
know, which matters across protocol versions.

To let the enum switch a union, xdrc writes the same `XDRDiscriminant` impl
that x23c does, against the current trait:

```rust
impl xdrs::XDRDiscriminant for E {
    const ENCODING: xdrs::DiscriminantEncoding = xdrs::DiscriminantEncoding::Signed;
    fn discriminant(&self) -> i32 { self.0 }
    fn from_discriminant(d: i32) -> Self { Self(d) }
}
```

For an open enum every `i32` is a value, so the infallible
`from_discriminant` is correct. This also keeps xdrc's output close to
x23c's, which makes the protocol-rs migration diff easier to check.

#### Deferred: closed enums

xdrc never emits closed enums, so it does not need support for them.

Branch `dbg/switch-newtype`, described in its `PR.md`, has that support
parked. It:

- adds `derive(XDRDiscriminant)`, for a fieldless `#[repr(i32)]` enum and for
  the open newtype
- makes `from_discriminant` return `Option<Self>`, so a union's default arm
  rejects a value that is none of a closed enum's members

The second change breaks every impl written by hand, including the impls
x23c and xdrc generate. Checking protocol-rs against the branch fails with
18 E0053 errors in `thruput-xdr` alone. It should ship only when Rust-first
code needs a closed enum to switch a union. The generators would then change
their impls in the same step, either to `-> Option<Self> { Some(Self(d)) }`
or to the derive.

### Every union names its switch type

xdrc emits:

- `discriminant = "<switch type>"`, using the source's typedef name, as x23c
  does. rustc resolves the name.
- `discrim_var = "<switch id>"`, so that `derive(XDR)` keeps the name of the
  switch variable. x23c does not emit it today.
- never the older `discrim_type`.

We decided against inferring an unsigned switch from `#[repr(u32)]`. A repr
can name only `i32` or `u32`, and enum and `bool` switches need the attribute
anyway.

### Variant tags

A generated union is a `#[repr(i32)]` Rust enum. Each variant's Rust
discriminant, its tag, is the case value as an `i32`. These follow current
x23c:

- **Unsigned cases of 2³¹ and above** keep their bit pattern. x23c writes
  `4294967295 as u32 as i32`; xdrc can write the suffixed literal
  `4294967295u32 as i32`.
- **The default variant's first field** holds the switch type: the enum
  newtype, `u32` or `bool`.
- **The default variant's tag** is one that no case uses. x23c computes it in
  a const block: the lowest `i32` that is not a case value. xdrc can do the
  same, or compute it while generating whenever every case value is a
  literal.

Open points:

- **One variant per case label.** An arm with several `case`s becomes one
  variant per label, each with a copy of the payload. The normalizer treats
  this as equal to the grouped arm in the source.
- **A default variant whose stored value equals a case** decodes as that
  case. This is documented, not checked. xdrc could generate a checked
  constructor; the decision is open.

### Derived spec for enums

`derive(XDR)` does not yet describe the open newtype as an enum. After #379,
if a union derives `XDR` and switches on one, `SwitchType::case_label`
panics. A later `#[xdr(enum)]` attribute, with members that xdrc passes in,
or a `HasTypeDef` impl that xdrc emits, would fix this. protocol-rs generates
without the `spec` feature, so this can wait until something needs
`::spec()` for protocol types.

### Fixtures

hogd's `x23/tests/rust_check/check.x`, extended in #4738 and #4739, becomes
one of xdrc's first fixtures. It covers:

- default arms switched on unsigned int, int and bool
- several cases sharing one arm
- a switch on a typedef'd type
- hex case labels
- an anonymous union inside a struct
