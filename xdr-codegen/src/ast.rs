//! The syntax tree of an XDR specification, in the dialect x23c accepts.
//!
//! The dialect is RFC 4506 XDR plus:
//!
//! - RFC 5531 `program` definitions, with `version` and procedure
//!   definitions
//! - `namespace NAME { ... }` around definitions
//! - `%` lines, passed through verbatim to generated C++
//!
//! # Comments
//!
//! Comments carry documentation and code-generation markers such as
//! `#[rust(newtype)]`, so the tree keeps them. Each comment is kept as
//! source text, including its `//` or `/* */` delimiters and leading
//! indentation, with a trailing newline after each line. A node's comment
//! is assembled from two kinds of source comments:
//!
//! - **Leading comments**: whole-line comments before the node's first
//!   token. A blank line between the comment and the node is kept as an
//!   extra trailing newline.
//! - **Trailing comments**: a comment after the node's last token on the
//!   same line, along with any `//` lines directly below it (but not `///`
//!   lines). It is indented to the column where it starts.
//!
//! Which tokens' comments a node takes is listed on each comment field.
//! Comments elsewhere, such as those before a `case` label, are dropped.
//!
//! The leading comments of a file that a blank line sets off from what
//! follows become the file's [`header`](Spec::header), typically a license
//! notice.

use crate::diag::Span;

/// A parsed `.x` file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spec {
    /// Comments opening the file, set off from the rest by a blank line.
    pub header: String,
    /// The file's definitions, in source order.
    pub items: Vec<Item>,
}

/// A top-level definition, or one inside a namespace.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    /// The definition's comment.
    ///
    /// - `typedef` and `const`: the leading comment, followed by the
    ///   trailing comment after the `;`. This is how a marker such as
    ///   `typedef unsigned int Protocol; // #[rust(newtype)]` reaches a
    ///   typedef.
    /// - `enum`, `struct`, `union`, `namespace` and `program`: the leading
    ///   comment only.
    /// - `%` literals: empty.
    pub comment: String,
    /// What is defined.
    pub kind: ItemKind,
    /// From the first token through the closing `;` or `}`.
    pub span: Span,
}

/// The kinds of definition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemKind {
    /// `typedef DECLARATION;`: the declaration's name is the new type's name.
    Typedef(Declaration),
    /// `enum NAME { ... };`
    Enum(Named<EnumBody>),
    /// `struct NAME { ... };`
    Struct(Named<StructBody>),
    /// `union NAME switch (...) { ... };`
    Union(Named<UnionBody>),
    /// `const NAME = NUMBER;`
    Const(Const),
    /// `program NAME { ... } = VALUE;`
    Program(Program),
    /// `namespace NAME { ... }`
    Namespace(Named<Vec<Item>>),
    /// One or more consecutive `%` lines, each without its `%` and with a
    /// trailing newline.
    ///
    /// A `%` line inside a definition is moved before that definition.
    Literal(String),
}

/// A name and what it names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Named<T> {
    /// The name.
    pub name: Ident,
    /// What it names.
    pub body: T,
}

/// An identifier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ident {
    /// The identifier.
    pub name: String,
    /// Where it appears.
    pub span: Span,
}

/// An integer literal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Number {
    /// The literal as written. It may contain `'` or `_` digit separators,
    /// as C++ and Rust accept respectively.
    pub text: String,
    /// The literal's value. A literal with a leading `0` is octal, as in
    /// RFC 4506 and C++, and a `0x` prefix makes it hexadecimal. Every
    /// value fits an `i64` or a `u64`.
    pub value: i128,
    /// Where it appears.
    pub span: Span,
}

/// A constant: an integer literal, or the name of a constant or enum member.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    /// An integer literal.
    Number(Number),
    /// The name of a constant or enum member.
    Ident(Ident),
}

impl Value {
    /// Where the value appears.
    pub fn span(&self) -> Span {
        match self {
            Value::Number(n) => n.span,
            Value::Ident(i) => i.span,
        }
    }
}

/// `const NAME = NUMBER;`
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Const {
    /// The constant's name.
    pub name: Ident,
    /// Its value.
    pub value: Number,
}

/// The members of an enum.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnumBody {
    /// The members, in source order.
    pub members: Vec<EnumMember>,
}

/// `NAME = VALUE` in an enum.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EnumMember {
    /// The member's comment: its leading comment, followed by the trailing
    /// comment after its value or after the `,` that follows it.
    pub comment: String,
    /// The member's name.
    pub name: Ident,
    /// Its value.
    pub value: Value,
}

/// The fields of a struct.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StructBody {
    /// The fields, in source order. A struct has at least one, which may
    /// be `void`.
    pub fields: Vec<Member>,
}

/// A union: `switch (TYPE NAME) { ARMS }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnionBody {
    /// The type of the discriminant. The grammar admits any scalar or
    /// named type; whether it can switch a union is left to code
    /// generation.
    pub switch_type: TypeSpec,
    /// The name of the discriminant.
    pub switch_name: Ident,
    /// The arms, in source order. There is at least one, and only the
    /// last can be the default.
    pub arms: Vec<Arm>,
}

/// One or more `case` labels, or `default:`, followed by a declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Arm {
    /// The `case` values, in source order.
    pub cases: Vec<Value>,
    /// Whether the arm is the `default:` arm. RFC 4506 does not allow
    /// `case` labels on the default arm; x23c accepts them with a warning.
    pub is_default: bool,
    /// The arm's declaration.
    pub member: Member,
}

/// A struct field or union arm: a declaration, or `void`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    /// The leading comment of the declaration's first token, followed by
    /// the trailing comment after its `;`.
    pub comment: String,
    /// The declaration, or `None` for `void`.
    pub decl: Option<Declaration>,
    /// From the first token through the `;`.
    pub span: Span,
}

/// A name and its type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Declaration {
    /// The declared name.
    pub name: Ident,
    /// Its type.
    pub ty: DeclType,
}

/// The type in a declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeclType {
    /// `TYPE NAME`
    Plain(TypeSpec),
    /// `TYPE *NAME`
    Optional(TypeSpec),
    /// `TYPE NAME[LEN]`
    Array {
        /// The element type.
        elem: TypeSpec,
        /// The number of elements.
        len: Value,
    },
    /// `TYPE NAME<MAX>`, or `TYPE NAME<>` without a bound.
    VarArray {
        /// The element type.
        elem: TypeSpec,
        /// The maximum number of elements.
        max: Option<Value>,
    },
    /// `opaque NAME[LEN]`
    Opaque {
        /// The number of bytes.
        len: Value,
    },
    /// `opaque NAME<MAX>`, or `opaque NAME<>` without a bound.
    VarOpaque {
        /// The maximum number of bytes.
        max: Option<Value>,
    },
    /// `string NAME<MAX>`, or `string NAME<>` without a bound.
    String {
        /// The maximum number of bytes.
        max: Option<Value>,
    },
}

/// A type: a built-in type, a named type, or an inline definition.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeSpec {
    /// `int`
    Int,
    /// `unsigned int`, or `unsigned` alone, which x23c accepts with a
    /// warning.
    UnsignedInt,
    /// `hyper`
    Hyper,
    /// `unsigned hyper`
    UnsignedHyper,
    /// `float`
    Float,
    /// `double`
    Double,
    /// `quadruple`
    Quadruple,
    /// `bool`
    Bool,
    /// A type defined elsewhere, possibly in another file.
    Named(Ident),
    /// `enum { ... }`
    Enum(EnumBody),
    /// `struct { ... }`
    Struct(StructBody),
    /// `union switch (...) { ... }`
    Union(Box<UnionBody>),
}

/// `program NAME { VERSIONS } = VALUE;`
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    /// The program's name.
    pub name: Ident,
    /// The program number.
    pub value: Value,
    /// The versions, in source order. There is at least one.
    pub versions: Vec<Version>,
}

/// `version NAME { PROCEDURES } = VALUE;`
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    /// The version's name.
    pub name: Ident,
    /// The version number.
    pub value: Value,
    /// The procedures, in source order. There is at least one.
    pub procs: Vec<Procedure>,
}

/// `RESULT NAME(ARGS) = VALUE;`
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Procedure {
    /// The leading comment of the result type, followed by the trailing
    /// comment after the `;`.
    pub comment: String,
    /// The procedure's name.
    pub name: Ident,
    /// The procedure number.
    pub value: Value,
    /// The result type, or `None` for `void`. Only scalar and named types
    /// are allowed.
    pub result: Option<TypeSpec>,
    /// The argument types, in order. `(void)` is no arguments. Only scalar
    /// and named types are allowed.
    pub args: Vec<TypeSpec>,
}
