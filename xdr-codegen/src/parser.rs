//! Parses the tokens of a `.x` file into a [`Spec`].
//!
//! The parser is recursive descent, one function per nonterminal.
//! Following is the accepted grammar in EBNF, where terminals are quoted
//! and `ID` and `NUMBER` are the identifier and number tokens:
//!
//! ```text
//! spec        = item* EOF
//! item        = namespace | typedef | type_def | constant | program
//! namespace   = "namespace" ID "{" item* "}"
//! typedef     = "typedef" declaration ";"
//! type_def    = ("enum" ID enum_body | "struct" ID struct_body
//!               | "union" ID union_body) ";"
//! constant    = "const" ID "=" NUMBER ";"
//! program     = "program" ID "{" version+ "}" "=" value ";"
//! version     = "version" ID "{" procedure+ "}" "=" value ";"
//! procedure   = ("void" | scalar_type) ID "(" args ")" "=" value ";"
//! args        = "void" | scalar_type ("," scalar_type)*
//! enum_body   = "{" enum_member ("," enum_member)* [","] "}"
//! enum_member = ID "=" value
//! struct_body = "{" member+ "}"
//! union_body  = "switch" "(" scalar_type ID ")" "{" arm+ "}"
//! arm         = ("case" value ":")* ["default" ":"] member
//! member      = ("void" | declaration) ";"
//! declaration = "opaque" ID length
//!             | "string" ID "<" [value] ">"
//!             | type_spec "*" ID
//!             | type_spec ID [length]
//! length      = "[" value "]" | "<" [value] ">"
//! type_spec   = "enum" enum_body | "struct" struct_body
//!             | "union" union_body | scalar_type
//! scalar_type = "int" | "hyper" | "float" | "double" | "quadruple"
//!             | "bool" | "unsigned" ["int" | "hyper"] | ID
//! value       = ID | NUMBER
//! ```
//!
//! Beyond the EBNF, an arm has at least one label, and only the last arm
//! can be the default. `%` lines may appear between any two tokens; they
//! become items of their own (see [`Parser::items`]).
//!
//! Three deviations from RFC 4506 are accepted with a warning, where the
//! grammar allows them: a trailing comma in an enum, `case` labels on the
//! default arm, and `unsigned` alone. Checks on names and values are made
//! after parsing, in [`crate::check`].
//!
//! Each node takes the comments of particular tokens; see [`crate::ast`].

use crate::ast::*;
use crate::diag::{Diagnostic, Span};
use crate::lexer::{Tok, Token};

/// Parses a sequence of grammar symbols, in order, binding the named ones:
///
/// ```text
/// seq!(self; kw: "const", name: ident, "=", value: number, semi: ";");
/// ```
///
/// - `"text"` is a terminal, a keyword or punctuation, and its value is
///   the [`Token`]: `self.lit("text")?`.
/// - `method` or `method(args)` is a nonterminal: `self.method(args)?`.
/// - `name: symbol` binds the symbol's value to `name` in the enclosing
///   scope.
macro_rules! seq {
    (@one $p:ident; $name:tt; $lit:literal $(, $($rest:tt)*)?) => {
        let $name = $p.lit($lit)?;
        seq!($p; $($($rest)*)?);
    };
    (@one $p:ident; $name:tt; $method:ident $(($($arg:expr),*))? $(, $($rest:tt)*)?) => {
        let $name = $p.$method($($($arg),*)?)?;
        seq!($p; $($($rest)*)?);
    };
    ($p:ident;) => {};
    ($p:ident; $name:ident : $($rest:tt)*) => {
        seq!(@one $p; $name; $($rest)*);
    };
    ($p:ident; $($rest:tt)*) => {
        seq!(@one $p; _; $($rest)*);
    };
}

/// Parsing stops at the first syntax error, which has been recorded.
struct Stop;

type PResult<T> = Result<T, Stop>;

pub(crate) struct Parser<'s, 'd> {
    src: &'s str,
    tokens: Vec<Token>,
    pos: usize,
    /// `%` lines read but not yet placed in the tree.
    literals: String,
    diags: &'d mut Vec<Diagnostic>,
}

impl<'s, 'd> Parser<'s, 'd> {
    pub(crate) fn new(src: &'s str, tokens: Vec<Token>, diags: &'d mut Vec<Diagnostic>) -> Self {
        Self {
            src,
            tokens,
            pos: 0,
            literals: String::new(),
            diags,
        }
    }

    /// `spec = item* EOF`
    ///
    /// Returns `None` after a syntax error.
    pub(crate) fn parse(mut self) -> Option<Vec<Item>> {
        self.items(Tok::Eof).ok()
    }

    // ---- tokens -----------------------------------------------------------

    /// The next token, after setting aside any `%` lines.
    fn peek(&mut self) -> &Token {
        while self.tokens[self.pos].kind == Tok::Literal {
            let span = self.tokens[self.pos].span;
            // Drop the `%`.
            self.literals.push_str(&self.src[span.start + 1..span.end]);
            self.literals.push('\n');
            self.pos += 1;
        }
        &self.tokens[self.pos]
    }

    fn peek_kind(&mut self) -> Tok {
        self.peek().kind
    }

    /// The token most recently read.
    fn prev(&self) -> &Token {
        &self.tokens[self.pos - 1]
    }

    fn bump(&mut self) -> Token {
        let token = self.peek().clone();
        if token.kind != Tok::Eof {
            self.pos += 1;
        }
        token
    }

    fn text(&self, token: &Token) -> &'s str {
        &self.src[token.span.start..token.span.end]
    }

    /// Whether the next token is the terminal `text`.
    fn at(&mut self, text: &str) -> bool {
        self.peek_kind() == Tok::literal(text)
    }

    /// Reads the terminal `text` if it is next.
    fn eat(&mut self, text: &str) -> Option<Token> {
        self.at(text).then(|| self.bump())
    }

    /// Reads the terminal `text`, which must be next.
    fn lit(&mut self, text: &str) -> PResult<Token> {
        self.expect(Tok::literal(text))
    }

    fn expect(&mut self, kind: Tok) -> PResult<Token> {
        if self.peek_kind() == kind {
            Ok(self.bump())
        } else {
            self.unexpected(kind.describe())
        }
    }

    /// Records a syntax error at the next token.
    fn unexpected<T>(&mut self, expected: &str) -> PResult<T> {
        let found = self.peek().clone();
        let what = match found.kind {
            Tok::Ident | Tok::Number => format!("`{}`", self.text(&found)),
            kind => kind.describe().to_owned(),
        };
        self.diags.push(Diagnostic::error(
            found.span,
            format!("expected {expected}, found {what}"),
        ));
        Err(Stop)
    }

    fn warn(&mut self, span: Span, message: impl Into<String>) {
        self.diags.push(Diagnostic::warning(span, message));
    }

    // ---- repetition -------------------------------------------------------

    /// `x*`, where each `x` starts with the terminal `start`.
    fn many_while<T>(
        &mut self,
        start: &str,
        mut x: impl FnMut(&mut Self) -> PResult<T>,
    ) -> PResult<Vec<T>> {
        let mut xs = Vec::new();
        while self.at(start) {
            xs.push(x(self)?);
        }
        Ok(xs)
    }

    /// `x+`, where each `x` after the first starts with the terminal
    /// `start`.
    fn many1_while<T>(
        &mut self,
        start: &str,
        mut x: impl FnMut(&mut Self) -> PResult<T>,
    ) -> PResult<Vec<T>> {
        let mut xs = vec![x(self)?];
        while self.at(start) {
            xs.push(x(self)?);
        }
        Ok(xs)
    }

    /// `x+`, followed by the terminal `end`, which is left unread.
    fn many1_until<T>(
        &mut self,
        end: &str,
        mut x: impl FnMut(&mut Self) -> PResult<T>,
    ) -> PResult<Vec<T>> {
        let mut xs = vec![x(self)?];
        while !self.at(end) {
            xs.push(x(self)?);
        }
        Ok(xs)
    }

    /// `x ("," x)*`
    fn comma_list<T>(&mut self, mut x: impl FnMut(&mut Self) -> PResult<T>) -> PResult<Vec<T>> {
        let mut xs = vec![x(self)?];
        while self.eat(",").is_some() {
            xs.push(x(self)?);
        }
        Ok(xs)
    }

    /// `"void" | x`, as `None` for `void`.
    fn void_or<T>(&mut self, x: impl FnOnce(&mut Self) -> PResult<T>) -> PResult<Option<T>> {
        match self.eat("void") {
            Some(_) => Ok(None),
            None => x(self).map(Some),
        }
    }

    // ---- definitions ------------------------------------------------------

    /// `item*`, up to the token `end`, which is left unread.
    ///
    /// `%` lines become items of their own: those read within a definition
    /// go before it, and those before a namespace stay outside it.
    fn items(&mut self, end: Tok) -> PResult<Vec<Item>> {
        let mut items = Vec::new();
        while self.peek_kind() != end {
            if self.at("namespace") {
                self.flush_literals(&mut items);
            }
            let item = self.item()?;
            self.flush_literals(&mut items);
            items.push(item);
        }
        self.flush_literals(&mut items);
        Ok(items)
    }

    /// Places the `%` lines read so far in `items`.
    fn flush_literals(&mut self, items: &mut Vec<Item>) {
        if !self.literals.is_empty() {
            items.push(Item {
                comment: String::new(),
                kind: ItemKind::Literal(core::mem::take(&mut self.literals)),
                span: Span::default(),
            });
        }
    }

    /// `item = namespace | typedef | type_def | constant | program`
    fn item(&mut self) -> PResult<Item> {
        match self.peek_kind() {
            Tok::Namespace => self.namespace(),
            Tok::Typedef => self.typedef(),
            Tok::Enum | Tok::Struct | Tok::Union => self.type_def(),
            Tok::Const => self.constant(),
            Tok::Program => self.program(),
            _ => self.unexpected("a definition"),
        }
    }

    /// `namespace = "namespace" ID "{" item* "}"`
    fn namespace(&mut self) -> PResult<Item> {
        seq!(self; kw: "namespace", name: ident, "{", body: items(Tok::literal("}")), close: "}");
        Ok(Item {
            comment: kw.leading,
            kind: ItemKind::Namespace(Named { name, body }),
            span: kw.span.to(close.span),
        })
    }

    /// `typedef = "typedef" declaration ";"`
    fn typedef(&mut self) -> PResult<Item> {
        seq!(self; kw: "typedef", decl: declaration, semi: ";");
        Ok(Item {
            comment: combine(&kw.leading, &semi.trailing),
            kind: ItemKind::Typedef(decl),
            span: kw.span.to(semi.span),
        })
    }

    /// ```text
    /// type_def = ("enum" ID enum_body | "struct" ID struct_body
    ///            | "union" ID union_body) ";"
    /// ```
    fn type_def(&mut self) -> PResult<Item> {
        let (kw, kind) = match self.peek_kind() {
            Tok::Enum => {
                seq!(self; kw: "enum", name: ident, body: enum_body);
                (kw, ItemKind::Enum(Named { name, body }))
            }
            Tok::Struct => {
                seq!(self; kw: "struct", name: ident, body: struct_body);
                (kw, ItemKind::Struct(Named { name, body }))
            }
            _ => {
                seq!(self; kw: "union", name: ident, body: union_body);
                (kw, ItemKind::Union(Named { name, body }))
            }
        };
        seq!(self; semi: ";");
        Ok(Item {
            comment: kw.leading,
            kind,
            span: kw.span.to(semi.span),
        })
    }

    /// `constant = "const" ID "=" NUMBER ";"`
    fn constant(&mut self) -> PResult<Item> {
        seq!(self; kw: "const", name: ident, "=", value: number, semi: ";");
        Ok(Item {
            comment: combine(&kw.leading, &semi.trailing),
            kind: ItemKind::Const(Const { name, value }),
            span: kw.span.to(semi.span),
        })
    }

    /// `program = "program" ID "{" version+ "}" "=" value ";"`
    fn program(&mut self) -> PResult<Item> {
        seq!(self;
            kw: "program", name: ident,
            "{", versions: many1_while("version", Self::version), "}",
            "=", value: value, semi: ";",
        );
        Ok(Item {
            comment: kw.leading,
            kind: ItemKind::Program(Program {
                name,
                value,
                versions,
            }),
            span: kw.span.to(semi.span),
        })
    }

    /// `version = "version" ID "{" procedure+ "}" "=" value ";"`
    fn version(&mut self) -> PResult<Version> {
        seq!(self;
            "version", name: ident,
            "{", procs: many1_until("}", Self::procedure), "}",
            "=", value: value, ";",
        );
        Ok(Version { name, value, procs })
    }

    /// `procedure = ("void" | scalar_type) ID "(" args ")" "=" value ";"`
    fn procedure(&mut self) -> PResult<Procedure> {
        let leading = self.peek().leading.clone();
        seq!(self;
            result: void_or(Self::scalar_type), name: ident,
            "(", args: args, ")",
            "=", value: value, semi: ";",
        );
        Ok(Procedure {
            comment: combine(&leading, &semi.trailing),
            name,
            value,
            result,
            args,
        })
    }

    /// `args = "void" | scalar_type ("," scalar_type)*`
    fn args(&mut self) -> PResult<Vec<TypeSpec>> {
        let args = self.void_or(|p| p.comma_list(Self::scalar_type))?;
        Ok(args.unwrap_or_default())
    }

    // ---- types ------------------------------------------------------------

    /// `enum_body = "{" enum_member ("," enum_member)* [","] "}"`
    fn enum_body(&mut self) -> PResult<EnumBody> {
        seq!(self; "{", first: enum_member);
        let mut members = vec![first];
        while let Some(comma) = self.eat(",") {
            // A comment after the comma trails the member before it.
            let prev = members.last_mut().expect("an enum has a member");
            prev.comment = combine(&prev.comment, &comma.trailing);
            if self.at("}") {
                self.warn(comma.span, "RFC 4506 does not allow a trailing comma");
                break;
            }
            members.push(self.enum_member()?);
        }
        seq!(self; "}");
        Ok(EnumBody { members })
    }

    /// `enum_member = ID "=" value`
    fn enum_member(&mut self) -> PResult<EnumMember> {
        let leading = self.peek().leading.clone();
        seq!(self; name: ident, "=", value: value);
        Ok(EnumMember {
            comment: combine(&leading, &self.prev().trailing),
            name,
            value,
        })
    }

    /// `struct_body = "{" member+ "}"`
    fn struct_body(&mut self) -> PResult<StructBody> {
        seq!(self; "{", fields: many1_until("}", Self::member), "}");
        Ok(StructBody { fields })
    }

    /// `union_body = "switch" "(" scalar_type ID ")" "{" arm+ "}"`, where
    /// only the last arm can be the default.
    fn union_body(&mut self) -> PResult<UnionBody> {
        seq!(self;
            "switch", "(", switch_type: scalar_type, switch_name: ident, ")",
            "{", arms: arms, "}",
        );
        Ok(UnionBody {
            switch_type,
            switch_name,
            arms,
        })
    }

    /// `arm+`, up to `}` or the default arm.
    fn arms(&mut self) -> PResult<Vec<Arm>> {
        let mut arms = Vec::new();
        loop {
            let arm = self.arm()?;
            let is_default = arm.is_default;
            arms.push(arm);
            if is_default || self.at("}") {
                return Ok(arms);
            }
        }
    }

    /// `arm = ("case" value ":")* ["default" ":"] member`, with at least one
    /// label.
    fn arm(&mut self) -> PResult<Arm> {
        let cases = self.many_while("case", |p| {
            seq!(p; "case", value: value, ":");
            Ok(value)
        })?;
        let is_default = match self.eat("default") {
            Some(kw) => {
                if !cases.is_empty() {
                    self.warn(
                        kw.span,
                        "RFC 4506 does not allow case labels on the default arm",
                    );
                }
                seq!(self; ":");
                true
            }
            None if cases.is_empty() => return self.unexpected("`case` or `default`"),
            None => false,
        };
        seq!(self; member: member);
        Ok(Arm {
            cases,
            is_default,
            member,
        })
    }

    /// `member = ("void" | declaration) ";"`
    fn member(&mut self) -> PResult<Member> {
        let first = self.peek().clone();
        seq!(self; decl: void_or(Self::declaration), semi: ";");
        Ok(Member {
            comment: combine(&first.leading, &semi.trailing),
            decl,
            span: first.span.to(semi.span),
        })
    }

    /// ```text
    /// declaration = "opaque" ID length
    ///             | "string" ID "<" [value] ">"
    ///             | type_spec "*" ID
    ///             | type_spec ID [length]
    /// ```
    fn declaration(&mut self) -> PResult<Declaration> {
        let (name, ty) = match self.peek_kind() {
            Tok::Opaque => {
                seq!(self; "opaque", name: ident);
                let ty = match self.length()? {
                    Some(Length::Fixed(len)) => DeclType::Opaque { len },
                    Some(Length::Max(max)) => DeclType::VarOpaque { max },
                    None => return self.unexpected("`[` or `<`"),
                };
                (name, ty)
            }
            Tok::String => {
                seq!(self; "string", name: ident, "<", max: opt_value(">"), ">");
                (name, DeclType::String { max })
            }
            _ => {
                seq!(self; elem: type_spec);
                if self.eat("*").is_some() {
                    seq!(self; name: ident);
                    (name, DeclType::Optional(elem))
                } else {
                    seq!(self; name: ident);
                    let ty = match self.length()? {
                        Some(Length::Fixed(len)) => DeclType::Array { elem, len },
                        Some(Length::Max(max)) => DeclType::VarArray { elem, max },
                        None => DeclType::Plain(elem),
                    };
                    (name, ty)
                }
            }
        };
        Ok(Declaration { name, ty })
    }

    /// `[length]`, where `length = "[" value "]" | "<" [value] ">"`.
    fn length(&mut self) -> PResult<Option<Length>> {
        if self.at("[") {
            seq!(self; "[", len: value, "]");
            Ok(Some(Length::Fixed(len)))
        } else if self.at("<") {
            seq!(self; "<", max: opt_value(">"), ">");
            Ok(Some(Length::Max(max)))
        } else {
            Ok(None)
        }
    }

    /// ```text
    /// type_spec = "enum" enum_body | "struct" struct_body
    ///           | "union" union_body | scalar_type
    /// ```
    fn type_spec(&mut self) -> PResult<TypeSpec> {
        match self.peek_kind() {
            Tok::Enum => {
                seq!(self; "enum", body: enum_body);
                Ok(TypeSpec::Enum(body))
            }
            Tok::Struct => {
                seq!(self; "struct", body: struct_body);
                Ok(TypeSpec::Struct(body))
            }
            Tok::Union => {
                seq!(self; "union", body: union_body);
                Ok(TypeSpec::Union(Box::new(body)))
            }
            _ => self.scalar_type(),
        }
    }

    /// ```text
    /// scalar_type = "int" | "hyper" | "float" | "double" | "quadruple"
    ///             | "bool" | "unsigned" ["int" | "hyper"] | ID
    /// ```
    fn scalar_type(&mut self) -> PResult<TypeSpec> {
        let ty = match self.peek_kind() {
            Tok::Int => TypeSpec::Int,
            Tok::Hyper => TypeSpec::Hyper,
            Tok::Float => TypeSpec::Float,
            Tok::Double => TypeSpec::Double,
            Tok::Quadruple => TypeSpec::Quadruple,
            Tok::Bool => TypeSpec::Bool,
            Tok::Unsigned => return self.unsigned(),
            Tok::Ident => return Ok(TypeSpec::Named(self.ident()?)),
            _ => return self.unexpected("a type"),
        };
        self.bump();
        Ok(ty)
    }

    /// `"unsigned" ["int" | "hyper"]`
    fn unsigned(&mut self) -> PResult<TypeSpec> {
        seq!(self; kw: "unsigned");
        if self.eat("int").is_some() {
            Ok(TypeSpec::UnsignedInt)
        } else if self.eat("hyper").is_some() {
            Ok(TypeSpec::UnsignedHyper)
        } else {
            self.warn(
                kw.span,
                "RFC 4506 requires `int` or `hyper` after `unsigned` (assuming `unsigned int`)",
            );
            Ok(TypeSpec::UnsignedInt)
        }
    }

    // ---- names and values -------------------------------------------------

    /// `ID`
    fn ident(&mut self) -> PResult<Ident> {
        let token = self.expect(Tok::Ident)?;
        Ok(Ident {
            name: self.text(&token).to_owned(),
            span: token.span,
        })
    }

    /// `value = ID | NUMBER`
    fn value(&mut self) -> PResult<Value> {
        match self.peek_kind() {
            Tok::Ident => Ok(Value::Ident(self.ident()?)),
            Tok::Number => Ok(Value::Number(self.number()?)),
            _ => self.unexpected("a number or identifier"),
        }
    }

    /// `[value]`, followed by the terminal `end`, which is left unread.
    fn opt_value(&mut self, end: &str) -> PResult<Option<Value>> {
        if self.at(end) {
            Ok(None)
        } else {
            self.value().map(Some)
        }
    }

    /// `NUMBER`, which must be a valid 64-bit integer.
    fn number(&mut self) -> PResult<Number> {
        let token = self.expect(Tok::Number)?;
        let text = self.text(&token);
        match parse_number(text) {
            Some(value) => Ok(Number {
                text: text.to_owned(),
                value,
                span: token.span,
            }),
            None => {
                self.diags.push(Diagnostic::error(
                    token.span,
                    format!("`{text}` is not a valid number, or does not fit 64 bits"),
                ));
                Err(Stop)
            }
        }
    }
}

/// A `length` in a declaration.
enum Length {
    /// `"[" value "]"`
    Fixed(Value),
    /// `"<" [value] ">"`
    Max(Option<Value>),
}

/// The value of an integer literal: decimal, hexadecimal after `0x`, or
/// octal after a leading `0`, with `'` and `_` separators ignored. `None` if
/// it has no digits, has a digit invalid for its base, or fits neither an
/// `i64` nor a `u64`.
pub(crate) fn parse_number(text: &str) -> Option<i128> {
    let (negative, unsigned) = match text.as_bytes().first()? {
        b'-' => (true, &text[1..]),
        b'+' => (false, &text[1..]),
        _ => (false, text),
    };
    let (radix, digits) = match unsigned.strip_prefix("0x") {
        Some(hex) => (16, hex),
        None if unsigned.len() > 1 && unsigned.starts_with('0') => (8, &unsigned[1..]),
        None => (10, unsigned),
    };
    let digits: String = digits
        .chars()
        .filter(|c| !matches!(c, '\'' | '_'))
        .collect();
    if digits.is_empty() {
        // `0` followed only by separators is still zero.
        return (radix == 8).then_some(0);
    }
    let magnitude = i128::from_str_radix(&digits, radix).ok()?;
    let value = if negative { -magnitude } else { magnitude };
    (i128::from(i64::MIN)..=i128::from(u64::MAX))
        .contains(&value)
        .then_some(value)
}

/// A leading comment followed by a trailing one, whose indentation is
/// dropped when it follows another comment.
fn combine(leading: &str, trailing: &str) -> String {
    if leading.is_empty() {
        trailing.to_owned()
    } else {
        format!("{leading}{}", trailing.trim_start_matches([' ', '\t']))
    }
}
