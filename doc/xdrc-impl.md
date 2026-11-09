# xdrc: implementation notes

How xdrc's parser and Rust backend work, where they differ from x23c, and
how they have been checked. The rationale is in
[xdrc-design.md](xdrc-design.md).

## Layout

The crate `thruput-xdr-codegen` (`xdr-codegen/`, library `xdr_codegen`) is
unpublished until xdrc can replace x23c's Rust backend. It contains:

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

`xdr_codegen::parse` returns a `Spec` and its warnings, or every
diagnostic if there was an error. `xdr_codegen::rust::generate` turns a
`Spec` into a Rust module.

## Parser

### Tokens

The lexer is a table of regular expressions, `RULES` in `src/lexer.rs`,
taken from x23c's `lex.ll`. As flex does, it compiles all the rules into
one DFA, using `regex-automata`'s dense DFA with `MatchKind::All`, so that
each match state lists every rule that matches there. At each position
`longest_match` walks the DFA until it dies, remembering the last match
state. The token is that longest match, and its rule is the lowest-numbered
rule in that state. `(?m:^)` anchors a rule to the start of a line, like
flex's `^`; the DFA's start state depends on the byte before the position,
so the anchor works mid-file. The rules, in order:

1. newline
2. spaces, tabs, carriage returns and form feeds
3. a leading comment, at the start of a line only (see below)
4. a `%` line, at the start of a line only
5. a trailing comment
6. each keyword
7. a reserved word
8. an identifier
9. a number
10. punctuation

Where no rule matches, a fallback pattern, in a DFA of its own, takes the
unexpected character and what follows it up to the next space, newline or
`;`.

Each rule's action lives in `Lexer::apply`. The comment attachment there
corresponds to the token loop in x23c's `x23c.cc`, not to its scanner.

These follow x23c's flex rules, which give some unusual results:

- **Numbers** match `[+-]?([0-9'_]+|0x[0-9a-fA-F'_]+)`. So `'` alone is a
  number token, and `_1'` lexes as a number rather than an identifier. The
  parser then rejects such a number as invalid.
- **Keyword prefixes:** `constant` is an identifier, because the
  identifier match is longer than the keyword `const`.
- **Reserved words:** `char`, `short`, `inline` and `sizeof` are errors,
  and the lexer drops them and continues.
- **Unexpected characters:** an unexpected character is an error, and the
  lexer skips to the next space, newline or `;`. x23c's recovery differs in
  detail, but every error is fatal in both.
- **Comments ending in `**/`:** x23c's trailing-comment rule ends in
  `\*\/` rather than `\*+\/`, so x23c rejects `int a; /* x **/`. xdrc's
  rule ends in `\*+/` and accepts it. x23c's leading-comment rule already
  allows it.
- **Columns** in diagnostics count a tab as advancing to the next multiple
  of 8 plus one, as x23c does.

### Comments

Comments are kept, because they carry documentation and `#[rust(...)]`
markers. Every token has a *leading* comment and a *trailing* comment.

- **Leading comment:** one or more whole lines, each holding only a `/* */`
  comment, or one or more whole lines, each holding only a `//` comment,
  through the final newline. It attaches to the next token.
  - Consecutive leading comments are joined with an extra `\n`. Two runs of
    different styles therefore appear separated by a blank line.
  - If a blank line separates a leading comment from its token, the comment
    gets an extra trailing `\n`.
- **Trailing comment:** any other comment, attached to the previous token.
  It is stored indented to its starting column, with a trailing newline.
  - A `//` trailing comment continues onto following lines that start with
    `//`, but not `///` or an empty `//`. So in
    `int a; // one` followed by `       // two`, both lines belong to `a`.
  - A comment that shares its line with later code, such as
    `/* x */ int b;`, is a trailing comment of the token before it. It
    therefore documents the previous field, not `b`.
  - A trailing comment replaces any earlier one on the same token. That
    matters only in odd layouts.
- **Before the first token**, a trailing-style comment becomes a leading
  comment.
- **Header:** before the first token, comments that a blank line sets off
  from what follows become the file's header, typically a license notice.
  Every such block goes into the header except the last, which leads the
  first definition. A `%` line counts as a token here.
- **The end-of-file token** takes any comments after the last definition.
  They are then dropped.

The parser builds each node's comment from specific tokens' comments, as
x23c does. `combine(a, b)` is `a` followed by `b` with `b`'s indentation
removed, or `b` unchanged if `a` is empty.

| Node | Comment |
|---|---|
| `typedef`, `const` | combine(leading comment of the keyword, trailing comment of the `;`). This is how `typedef T X; // #[rust(newtype)]` gets its marker. |
| `enum`, `struct`, `union` | Leading comment of the keyword. A comment after the closing `};` is dropped. |
| Struct field, union arm | combine(leading comment of the declaration's first token, trailing comment of its `;`) |
| Enum member | combine(leading comment of its name, trailing comment of its value), then combined with the trailing comment of the following `,` |
| Procedure | combine(leading comment of its result type, trailing comment of its `;`) |
| `program`, `namespace` | Leading comment of the keyword |

Comments before a `case` label, and on `version` definitions, are dropped.

### Grammar

The module docs of `src/parser.rs` give the whole grammar in EBNF, about
25 lines. The parser has one function per nonterminal, each headed by its
production, and is written to read like it:

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
  - Name and value checks run afterwards in `src/check.rs`. These are
    duplicate type names, `int32_t` and the like as type names, and
    lowercase `true` and `false` as case labels.
  - Comment attachment is the code after each `seq!`, which picks which
    tokens' comments a node takes.
  - The three warnings that are about syntax stay in the parser, at the
    point where the grammar allows the deviation: a trailing comma, labels
    on the default arm, and `unsigned` alone.

### Definitions

- **`%` lines** keep their order relative to definitions. Consecutive lines
  form one `Literal`. A `%` line inside a definition moves before it. One
  before a `namespace` stays outside it.
- **Namespaces** nest in the tree.
- **Duplicate type names** are errors across the whole file, including
  across namespaces. Duplicate `const` names are not, as in x23c. Like the
  other checks in `src/check.rs`, this one runs only on a file that
  parses, so a file with a syntax error reports only that error and any
  lexical errors.
- **A union** has at least one arm. The default arm must be the last.
- **A struct** has at least one field, which may be `void`.
- **Syntax errors** stop parsing at the first one.

### Numbers

Each number keeps its source text and its value as an `i128`.

- `'` and `_` separators are ignored, `0x` makes it hexadecimal, and a
  leading `0` makes it octal, as in RFC 4506 and C++.
- The value must fit an `i64` or a `u64`.

### Diagnostics

Warnings, which `--strict` makes fatal:

- a trailing comma in an enum
- `unsigned` without `int` or `hyper`
- lowercase `true` or `false` as a case label
- `case` labels on the default arm
- `int32_t`, `uint32_t`, `int64_t` or `uint64_t` as a type name

### Differences from x23c's parser

| | x23c | xdrc |
|---|---|---|
| An invalid octal number such as `08` | Accepted; the C++ then fails to compile | Rejected |
| A number that does not fit 64 bits | Accepted; its value is wrong | Rejected |
| A comment after an enum's trailing comma | Dropped | Kept on the last member |
| Comments on `program` and `namespace` | Dropped, and emitted as C++, respectively | Kept on the definition |
| Location of a syntax error at the end of the file | The last token | The end of the file |
| Diagnostic text | | Reworded, with `error:` on errors |
| Where a `%` line lands | x23c's ordering has quirks | As in the table above. `gen_rs` ignores `%` lines, so this does not affect Rust output. |

Checked against x23c (built from hogd at the commit protocol-rs#366
generates with) on 16 small inputs, most of them invalid. Apart from the
first two rows above, the
two compilers accepted and rejected the same inputs at the same line and
column. Across all 108 `.x` files in thruput-protocol and hogd's
`x23/tests`, they give the same 6 warnings, at the same positions.

## Rust backend

The goal is the same public API as x23c's output, with tidier code.
Byte-for-byte equality is not a goal.

### Construction and printing

Code is built with `quote` and printed with `prettyplease`, one top-level
item at a time, with a blank line between items. The file's header comment
is copied verbatim after a `// Generated by xdrc VERSION. Do not edit.`
line.

### Comments

`quote` keeps only doc attributes, so:

- **`///` lines** become `#[doc]` attributes, so rustdoc comments, as in
  x23c.
- **`//` lines and `/* */` comments** become `#[doc]` attributes whose text
  starts with `\u{1}`. After printing, `comment::finish` turns each such
  line back into a `//` comment.
- **`/** */`** becomes a doc comment.

Each comment line becomes its own line, without x23c's relative
re-indentation. An empty comment line, which separates paragraphs, is
kept, except at the start or end of a `/* */` comment. `comment::plain` uses
the same mechanism for comments that xdrc itself writes, such as the
SAFETY note on RPC client methods.

### Markers

Markers are found anywhere in a definition's comment and removed from the
output. A comment line that held only markers is dropped.

| Marker | Where | Effect |
|---|---|---|
| `#[rust(newtype)]` | typedef | A transparent struct wrapping the type, rather than an alias |
| `#[rust(box_ptr)]` | field or arm | Box the element type |
| `#[rust(const=u64)]` | const | Type `u64` (default `u32`) |
| `#[rust(const=u32)]` | const | Type `u32` |
| `#[rust(ConstWriteXDR)]` | enum, struct, union | Also derive `ConstWriteXDR` |
| `#[rust(no_debug)]` | enum, struct, union, newtype | Leave out `Debug`. Inline types defined within do not inherit it. |
| `#[rust(no_eq)]` | enum, struct, union, newtype | Derive only `Clone`, `Debug`, `PartialEq` and `PartialOrd` |
| `#[doc(hidden)]` | anywhere | Emit `#[doc(hidden)]` after the comment |

`--recursive_type T` boxes `T` everywhere, as `#[rust(box_ptr)]` does for a
single field.

### Features

Each of `serde`, `xdrs`, `schema`, `spec` and `std` gates what it covers
with `#[cfg_attr(feature = ...)]` or `#[cfg(feature = ...)]`.
`--rust-feature F` emits what F covers with no condition, and
`--rust-no-feature F` leaves it out. The module prelude repeats x23c's
`#![allow(...)]` list and its imports: `use crate::*`, `Deref`/`DerefMut`,
`Option`, and the gated `std::io`, `xdrs::*`, `xdrs::xdr_repr::*` and
serde imports.

### Output by definition kind

These have the same shapes as x23c's output:

- **const:** `pub const NAME: u32 = VALUE;`
- **typedef:** `pub type NAME = TYPE;`
- **Newtype:** `struct NAME { inner: T }` with `From`, `new`,
  `into_inner`, `Deref` and `DerefMut`. A fixed-length opaque newtype adds
  `AsRef<[u8]>`, `From<&[u8; N]> for &NAME`, `ReadXDR for &NAME`, a
  hex-string `JsonSchema`, serde as hex, `Copy`, and `Default` when N ≤ 32.
- **Enum:** an open newtype `struct NAME(pub i32)` with an associated
  constant per member, `From<i32>`, `XDRDiscriminant` (gated on `xdrs`) and
  `Display` (gated on `std`), which prints `NAME::MEMBER`.
- **Struct:** `#[repr(C)]`, with `pub` fields. A fixed opaque field
  serializes as hex and has a matching schemars attribute. A struct with
  only `void` becomes `pub struct NAME();`.
- **Inline types:** named `PARENT_FIELD` and emitted before their parent.
- **Union:** `#[repr(i32)] #[non_exhaustive]`, with
  `#[xdr(discriminant = "SWITCH")]`.
  - Variants are named as listed in `src/rust/union.rs`: `_5`, `_neg1`,
    `_0x10`, a member name, `TRUE` or `FALSE`, or `_CONST`.
  - The default variant's tag is computed by rustc in a const block, as the
    least `i32` that no case uses.
  - The default variant's first field, which holds the discriminant, has
    the switch type. When that type is an alias, defined in the same file,
    of `int`, `unsigned int` or `bool`, possibly through other aliases, the
    field gets the primitive instead. The derived const writers (since
    xdrs#381) write the field directly only when its type is named as a
    primitive; for an alias they would look for const methods that a
    primitive does not have. An alias from another file cannot be resolved,
    and keeps this problem.
  - An accessor named after the switch variable returns the discriminant.
- **Programs:** for each version, the trait `P_V`, `P_V_Dispatcher` and
  `P_V_Client`, against `thruput_rpc_api`.

### Differences from x23c's output

None of these changes the public API of protocol-rs; see Verification.

- **Comments:**
  - `//` comments stay plain, and `///` comments are doc comments.
  - Markers leave no `// ` lines behind. x23c leaves `#[rust(box_ptr)]` in
    the output.
  - Union variants lose x23c's `// case X:` and `//   TYPE NAME;` lines.
- **Layout:** each union's accessor follows the union, instead of sitting
  under an "XDR internals" banner. It has a one-line doc comment.
- **Numbers:**
  - `'` becomes `_` everywhere, not only in constants. A bound such as
    `<1'000>` therefore compiles.
  - A leading-zero octal becomes `0o...`; Rust would read `010` as ten.
  - Array lengths that are numbers drop the `as usize` cast.
- **Union cases:** for a union switched on a named type, a case label that
  is a constant defined in the same file is treated as a constant, giving
  variant `_K` and value `K as Switch`. x23c takes it for an enum member,
  which does not compile. A label that is not a constant of the file is
  still taken for an enum member.
- **Constructs x23c mishandles, now generated correctly:**
  - `typedef struct { ... } T;` defines `T` itself. x23c emitted
    `pub type T = T;`.
  - An inline type under `*`, `[]` or `<>` is named consistently. x23c
    produced `S_Option<...>`.
- **Newtypes:** a fixed opaque newtype whose length is a named constant gets
  a `JsonSchema` and no `Default`. x23c threw an exception on such a
  length.
- **Errors instead of code that does not compile:**
  - `quadruple`
  - a union switched on a type other than `int`, `unsigned int`, `bool` or
    a named type
  - a `bool` switch with a case other than `TRUE` or `FALSE`
  - a negative length
  - a name that cannot be a Rust identifier: `self`, `Self`, `super`,
    `crate` or `_`
- **Raw identifiers:** xdrc escapes every Rust keyword, including reserved
  ones such as `try`, `gen` and `box`, as a raw identifier. x23c's list was
  shorter.
- **Display** writes a member's name with `f.write_str`. The output is the
  same.
- **RPC:**
  - The client's SAFETY note comes before the method rather than inside
    it.
  - The client struct has `#[allow(non_camel_case_types)]`.
- **Header:** `Generated by xdrc VERSION` replaces x23c's commit hash and
  its `DO NOT CHECK IN` line.

## Verification

### Unit tests

| File | Covers |
|---|---|
| `tests/parse.rs` | Comment attachment, declarations, inline types, unions, programs, namespaces, `%` lines, numbers, warnings and errors |
| `tests/rust.rs` | What the backend rejects, raw identifiers, number forms, comments and markers, union case names, forced features |
| `src/rust/comment.rs` | Comment styles and printing |

### Corpus test

`tests/corpus.rs` parses, and generates Rust from, every `.x` file under
the directories in `XDRC_CORPUS`. It is skipped when `XDRC_CORPUS` is
unset:

```shell
XDRC_CORPUS=../thruput-protocol:../hogd/x23/tests \
  cargo test -p thruput-xdr-codegen --test corpus -- --nocapture
```

### Golden test and check crate

`check/check.x` and `check/plain.x` are hogd's `x23/tests/rust_check`
fixtures, along with its `src/lib.rs`. `tests/golden.rs` regenerates
`check/src/generated.rs` and `check/src/plain.rs`, with the same feature
flags as hogd's `check.sh`, and fails if they differ from the committed
files. After changing the generator, run it with `XDRC_BLESS=1` and review
the diff.

The `thruput-xdrc-check` crate compiles those modules against this
workspace's `xdrs` and `thruput-rpc-api`, then runs their 12 tests, with
all features and with none.

### protocol-rs, regenerated with xdrc

Done once, by hand, on 2026-11-06. To repeat it:

1. Copy protocol-rs.
2. Run the x23c commands from its `scripts/generate.sh`, with `XDRC`
   pointing to xdrc. That regenerates all 63 generated files.
3. Run the checks below.

All passed:

- `cargo check --workspace --all-features --all-targets`, and
  `cargo test --workspace --all-features`
- every `cargo build` combination in `scripts/ci-check.sh`, including the
  `riscv32im-unknown-none-elf` builds and the RPC crates
- `ci-check.sh`'s clippy command. Workspace-wide clippy with all features
  gives the same 25 warnings as on the x23c output.
- **Public API comparison:** rustdoc JSON for all 11 crates, from both
  versions, using `cargo +nightly doc` with `--output-format json`. A
  summary of every public item, field type, enum variant and discriminant,
  constant value, trait impl and method signature: 21,435 lines, all
  identical.
- hogd's `x23/tests/rust_check/check.sh`, run with xdrc: 12 tests pass.

The API comparison script is not in the repository. A `cargo xtask` or a
CI job could make this regeneration repeatable.
