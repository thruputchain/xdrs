# Design: `xdrc`, an XDR compiler in Rust

Status: implemented. How to use xdrc, and how its output differs from
x23c's, is in [README.md](README.md); deferred work is in
[TODO.md](TODO.md).

## Summary

xdrc is an XDR-to-Rust compiler, written in Rust, in this workspace. It
replaces the Rust backend of x23c, the C++ compiler in hogd's `x23/x23c`,
which generated the Rust types in thruput-protocol-rs from the `.x` files in
thruput-protocol.

x23c remains for its C++ (`gen_hh`) and TypeScript (`gen_ts`) backends.

## Background: x23c

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
- **Change history:** of the 35 commits to x23c from May 2026 until xdrc,
  22 touched `gen_rs.cc`, 5 `gen_hh.cc` and 7 `gen_ts.cc`. None touched
  `parse.yy`. The grammar is stable; the Rust backend is what changes.
- **Other consumers:** hogd's CMake runs x23c during its own build,
  including `built_contracts`, to produce C++ headers. gen-ts emits
  `@thruputchain/xdr` imports for the TypeScript SDK.

## Motivation

### Maintainability and output quality

`gen_rs` builds code by concatenating strings. Generating with `quote` and
printing with `prettyplease` produces syntactically valid, consistently
formatted output by construction. And it's a lot easier to maintain.

### Tooling

A repository can install or run a pinned `xdrc` through cargo, instead of
downloading a C++ binary built for each OS and architecture by hogd's release
workflow.

### Version skew across repositories

The generated Rust is written against specific xdrs and thruput-rpc-api
APIs, but the generator lived in a different repository:

- hogd's `x23/tests/rust_check` builds the generated code against a released
  xdrs, so an xdrs change reached it only after a release.
- protocol-rs downloaded the unpinned `latest-x23c` release from hogd, so
  any x23c merge could change its regenerated sources and break its CI.
- Features landed as paired PRs across hogd and xdrs.

### xdrs tests against a hand-written copy of x23c's output

`rpc/rpc-api-fixture` is a hand-written copy of the shape x23c emits for RPC
programs. With the compiler in this workspace, tests can generate from `.x`
fixtures and compile against the in-tree xdrs, so codegen and runtime change
in the same PR.

## Scope

In scope:

- a parser for the x23c dialect, including comment attachment and the
  `#[rust(...)]` markers
- a Rust backend covering everything `gen_rs` emits:
  - types and constants
  - serde, schemars and xdrs derives behind features
  - `Display` for enums
  - union accessors
  - RPC service traits, dispatchers and clients for thruput-rpc-api
- a CLI that accepts the same flags as `x23c --rust`, so a build script
  changes only the command it runs

Out of scope:

- C++ and TypeScript backends. Porting them would make hogd's C++ build
  depend on cargo, for little gain.
- Go (thruput-protocol-go is generated separately).
- Byte-for-byte parity with x23c's output. The goal is the same public API.

## Shape

- The library crate `thruput-xdr-codegen` (`xdr_codegen`) holds the parser,
  the AST and the Rust backend; `xdrc` is a thin binary over it.
- Generated code is checked in where it is used. That keeps diffs
  reviewable and the crates publishable. A `build.rs` entry point can come
  later if wanted.

| Path | Contents |
|---|---|
| `src/diag.rs` | Byte spans, line and column lookup, diagnostics |
| `src/lexer.rs` | Tokens and comment attachment |
| `src/parser.rs` | Recursive-descent parser, with the grammar in its module docs |
| `src/check.rs` | Checks on names and values after parsing |
| `src/ast.rs` | The syntax tree |
| `src/rust/` | The Rust backend: `mod.rs` for types, constants and typedefs; `union.rs`; `rpc.rs`; `comment.rs` |
| `src/bin/xdrc.rs` | The `xdrc` command |
| `tests/` | Unit tests, the corpus test and the golden test |
| `check/` | The crate `thruput-xdrc-check`, which compiles and tests generated code against this workspace |

## Costs and risks

- **Licenses differ.** hogd's `x23` tree, which includes x23c and its `.x`
  test files, is GPL-3 (`x23/COPYING`, copyright David Mazières); this
  repository is Apache-2.0. The lexer's rules are adapted from x23c's
  `lex.ll`, and the check crate's fixtures and tests are from
  `x23/tests/rust_check`.
- **Two parsers for one dialect.** x23c keeps its parser for C++ and TS.
  Because the grammar has been stable, the risk of drift is low. The corpus
  test, over all of thruput-protocol and hogd's `x23/tests/*.x`, keeps them
  aligned.
- **Comment handling.** x23c separates a file's header comment from the
  comments that follow, attaches each comment to the definition after it,
  and carries end-of-line comments onto fields and enumerators. These
  become rustdoc, so they show in the output. `quote` drops comments, so
  they must be emitted as `#[doc = ...]` attributes (see below).
- **One large regeneration diff.** Switching a consumer to xdrc produces one
  big diff. It is checked for equivalence by building and testing the
  consumer with the new output, and by diffing its public API through
  rustdoc JSON. protocol-rs carries a script that does the latter.
- **RPC glue** is a meaningful share of `gen_rs`. It is tied to
  thruput-rpc-api, which lives in this workspace.

## Plan

1. Parser and AST, tested against the full `.x` corpus. Done.
2. Rust backend, starting from rust_check's `check.x` and `plain.x`
   fixtures. Done. Replacing `rpc/rpc-api-fixture` with generated code is
   still to do.
3. Generate protocol-rs with `xdrc`, verify equivalence, then switch its
   generation and CI. Done, with xdrc pinned to a commit of this
   repository.
4. Remove `gen_rs` from x23c, and the `--rust` option with it. To do.

## Decisions

### Hand-written lexer and parser

The lexer and the recursive-descent parser are written by hand, with
`regex-automata` as their only dependency.

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
- **Diagnostics.** x23c reports several RFC deviations as warnings and
  keeps going, and `--strict` turns them into errors. Code written by hand
  handles warnings with exact source positions directly.
- **Cost.** The front end (`diag`, `lexer`, `parser`, `check` and `ast`)
  came to about 1,700 lines, documentation included, with no build-time
  code generation.

Alternatives considered:

- **lalrpop:** closest to bison, but adds build-time code generation, and
  comment attachment still sits outside it.
- **pest:** produces an untyped parse tree that we would convert to the AST
  by hand anyway.
- **chumsky:** has the best error recovery, but brings heavy generics, slow
  compiles and an API that has changed a lot between versions.
- **winnow and nom:** save little over plain code for a grammar this size.

#### The lexer is a rule table

`RULES` in `src/lexer.rs` is a table of regular expressions taken from
x23c's `lex.ll`, with flex's semantics: at each position, the longest
match wins, and ties go to the earlier rule.

- As flex does, the rules are compiled into one DFA, with
  `regex-automata`'s dense DFA and `MatchKind::All`, so that each match
  state lists every rule that matches there. `longest_match` walks the DFA
  until it dies, remembering the last match state; the token's rule is the
  lowest-numbered rule in that state.
- `(?m:^)` anchors a rule to the start of a line, like flex's `^`. The
  DFA's start state depends on the byte before the position, so the anchor
  works mid-file.
- Where no rule matches, a fallback pattern, in a DFA of its own, takes the
  unexpected character and what follows it up to the next space, newline
  or `;`.
- The rules, in order: newline; other whitespace; a leading comment, at the
  start of a line; a `%` line, at the start of a line; a trailing comment;
  each keyword; a reserved word; an identifier; a number; punctuation.
- Each rule's action lives in `Lexer::apply`. The comment attachment there
  corresponds to the token loop in x23c's `x23c.cc`, not to its scanner.
- `regex-automata` is a regex engine, not a parser generator, and it was
  already in the workspace's dependency graph. Building the DFA takes a
  moment at startup, which does not matter for a code generator.

One rule deliberately differs: x23c's trailing-comment rule ends in `\*\/`,
so it rejects `int a; /* x **/`; xdrc's ends in `\*+/`, as x23c's
leading-comment rule already does.

#### The parser is shaped like the grammar

- The module docs of `src/parser.rs` give the grammar in EBNF, about 25
  lines, and each parse function is headed by its production.
- **`seq!`** parses a sequence of symbols and binds the named ones in the
  enclosing scope, so a production is one statement:
  `seq!(self; kw: "const", name: ident, "=", value: number, semi: ";")`.
  A quoted string is a terminal, which `Tok::literal` resolves by running
  the text through the lexer's own DFA, so the grammar and the lexer
  cannot disagree about a keyword. A bare name, or a name with arguments,
  is a call to another parse function.
- **Repetition helpers** stand for EBNF's `*` and `+`: `many_while`,
  `many1_while`, `many1_until`, `comma_list` and `void_or`. Each stops on a
  single token of lookahead.
- **Alternatives** are a `match` on the next token, as in an LL(1) table.
- **What the grammar does not express** is kept out of the productions:
  - Name and value checks run afterwards in `src/check.rs`.
  - Comment attachment is the code after each `seq!`, which picks which
    tokens' comments a node takes. `combine(a, b)` is `a` followed by `b`
    with `b`'s indentation removed, or `b` unchanged if `a` is empty.
  - The three warnings that are about syntax stay in the parser, at the
    point where the grammar allows the deviation: a trailing comma, labels
    on the default arm, and `unsigned` alone.

The parser was compared with x23c on all 108 `.x` files in
thruput-protocol and hogd's `x23/tests`, which give the same 6 warnings at
the same positions, and on small, mostly invalid inputs, which the two
accept and reject alike, at the same line and column, except as the README
lists.

### The compiler has its own AST

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

xdr-repr serves the Rust-first direction, where Rust is the source and XDR
is derived from it, as in contract specs. The compiler serves the XDR-first
direction.

**The compiler has its own AST rather than using xdr-repr's types.** As a
parser output, xdr-repr lacks:

- source positions
- comments and markers
- `%` literals
- `namespace`, except behind a feature
- constants wider than `i32`/`u32`
- `Clone` and `Debug` on its RPC types

It also panics on invalid identifiers rather than returning errors. Adding
all of this would bloat a `no_std` crate that proc macros and runtime code
depend on, and comments would start to count toward its equality.

Lowering the AST to `xdr_repr::Specification`, to check that the source and
the spec derived from the generated Rust agree on the wire, is deferred
(see TODO.md). The derived spec only has to match the source on the wire;
it does not have to reproduce enums or named bounds. If a consumer of the
derived spec appears, the generated Rust can carry more information, for
example through attributes.

### The Rust backend

- **Construction and printing.** Code is built with `quote` and printed
  with `prettyplease`, one top-level item at a time, with a blank line
  between items. The file's header comment is copied verbatim after the
  `Generated by xdrc` line.
- **Comments.** `quote` keeps only doc attributes. A `///` or `/** */`
  comment becomes a `#[doc]` attribute. A `//` or `/* */` comment becomes a
  `#[doc]` attribute whose text starts with `\u{1}`; after printing,
  `comment::finish` turns each such line back into a `//` comment.
  `comment::plain` uses the same mechanism for comments that xdrc itself
  writes, such as the SAFETY note on RPC client methods.
- **Features.** `Gen::gated` emits what a feature covers behind `#[cfg]`,
  unconditionally, or not at all, according to `--rust-feature` and
  `--rust-no-feature`; `gated_any` and `attr_any` do the same for what
  serves several features, such as `use xdrs::*`, which `derive(XDR)`
  under `spec` needs as much as the `xdrs` derives do.
- **The default variant's field.** The derived const writers (xdrs#381)
  write the field directly only when its type is named as a primitive; for
  an alias they look for const methods that a primitive does not have. So
  xdrc resolves an alias of `int`, `unsigned int` or `bool` defined in the
  same file to the primitive. An alias from another file cannot be
  resolved, and keeps this problem.
- **Divergence from x23c.** Where x23c generates code that does not
  compile, or that is wrong, xdrc either generates working code or reports
  an error, choosing the error whenever working code would change the API
  x23c would have emitted. The README lists each case.

## Unions and enums

How xdrc emits unions and enums depends on xdrs's handling of union
switches:

- **#375** names a union's switch type with `#[xdr(discriminant = "T")]`, by
  default `i32`. The type decides how the discriminant is encoded, which
  differs under CXS, and the type of the default arm's field.
- **#379**, not yet merged, makes `derive(XDR)` describe the switch by that
  type: `i32` and `u32` cases by number, `bool` as `TRUE`/`FALSE`, and an
  enum by member name.

As of hogd#4724, #4738 and #4739, x23c follows #375. It:

- emits `discriminant = "<switch type>"` on every union
- writes an `XDRDiscriminant` impl for every XDR enum newtype
- types the default variant's first field with the switch type
- writes case values typed by the switch type, cast to the `i32` tag
- gives the default variant a tag that no case uses

xdrc matches this rather than improving on it.

### XDR enums stay open newtypes

xdrc emits an XDR enum as x23c does: `struct E(pub i32)`, with the members
as associated constants. Decoding then accepts a member that this code
does not know, which matters across protocol versions.

To let the enum switch a union, xdrc writes the same `XDRDiscriminant` impl
that x23c does:

```rust
impl xdrs::XDRDiscriminant for E {
    const ENCODING: xdrs::DiscriminantEncoding = xdrs::DiscriminantEncoding::Signed;
    fn discriminant(&self) -> i32 { self.0 }
    fn from_discriminant(d: i32) -> Self { Self(d) }
}
```

For an open enum every `i32` is a value, so the infallible
`from_discriminant` is correct. Closed enums are deferred (see TODO.md).

### Every union names its switch type

xdrc emits:

- `discriminant = "<switch type>"`, using the source's typedef name, as
  x23c does. rustc resolves the name.
- not `discrim_var`, which x23c does not emit either. So `derive(XDR)`
  names every switch variable `which`, not the source's name (see
  TODO.md).
- never the older `discrim_type`.

We decided against inferring an unsigned switch from `#[repr(u32)]`. A repr
can name only `i32` or `u32`, and enum and `bool` switches need the
attribute anyway.

### Variant tags

A generated union is a `#[repr(i32)]` Rust enum. Each variant's Rust
discriminant, its tag, is the case value as an `i32`. As in x23c:

- **Unsigned cases of 2³¹ and above** keep their bit pattern:
  `4294967295 as u32 as i32`.
- **The default variant's first field** holds the switch type: the enum
  newtype, `u32` or `bool`, or the primitive behind an alias (see above).
- **The default variant's tag** is computed by rustc in a const block, as
  the least `i32` that no case uses.
- **One variant per case label.** An arm with several `case`s becomes one
  variant per label, each with a copy of the payload.

### Fixtures

hogd's `x23/tests/rust_check/check.x`, extended in #4738 and #4739, is
xdrc's main fixture. It covers:

- default arms switched on unsigned int, int and bool
- several cases sharing one arm
- a switch on a typedef'd type
- hex case labels
- an anonymous union inside a struct

xdrc adds cases for its own fixes: an enum member valued by a name
(`Aliased`), lowercase boolean labels (`LowerBool`, `LowerInt`), a default
arm switched on an alias (`ConstAliasDefault`), and a fixed opaque union
arm (`Sig`).
