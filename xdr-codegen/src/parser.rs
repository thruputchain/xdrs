//! Parses the tokens of a `.x` file into a [`Spec`].

use std::collections::HashSet;

use crate::ast::*;
use crate::diag::{Diagnostic, Span};
use crate::lexer::{Tok, Token};

/// Parsing stops at the first syntax error, which has been recorded.
struct Stop;

type PResult<T> = Result<T, Stop>;

pub(crate) struct Parser<'s, 'd> {
    src: &'s str,
    tokens: Vec<Token>,
    pos: usize,
    /// `%` lines read but not yet placed in the tree.
    literals: String,
    /// Names given to types, which must be distinct across the file.
    type_names: HashSet<String>,
    diags: &'d mut Vec<Diagnostic>,
}

impl<'s, 'd> Parser<'s, 'd> {
    pub(crate) fn new(src: &'s str, tokens: Vec<Token>, diags: &'d mut Vec<Diagnostic>) -> Self {
        Self {
            src,
            tokens,
            pos: 0,
            literals: String::new(),
            type_names: HashSet::new(),
            diags,
        }
    }

    /// Parses the whole file, or returns `None` after a syntax error.
    pub(crate) fn parse(mut self) -> Option<Vec<Item>> {
        let items = self.items(Tok::Eof).ok()?;
        Some(items)
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

    fn eat(&mut self, kind: Tok) -> Option<Token> {
        (self.peek_kind() == kind).then(|| self.bump())
    }

    fn expect(&mut self, kind: Tok) -> PResult<Token> {
        match self.eat(kind) {
            Some(token) => Ok(token),
            None => self.unexpected(kind.describe()),
        }
    }

    fn expect_punct(&mut self, c: u8) -> PResult<Token> {
        self.expect(Tok::Punct(c))
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

    // ---- definitions ------------------------------------------------------

    /// Definitions up to `end`, which is left unread.
    fn items(&mut self, end: Tok) -> PResult<Vec<Item>> {
        let mut items = Vec::new();
        loop {
            let kind = self.peek_kind();
            if kind == end {
                self.flush_literals(&mut items);
                return Ok(items);
            }
            let item = match kind {
                Tok::Namespace => {
                    // `%` lines before the namespace stay outside it.
                    self.flush_literals(&mut items);
                    self.namespace()?
                }
                Tok::Typedef => self.typedef()?,
                Tok::Enum | Tok::Struct | Tok::Union => self.type_definition()?,
                Tok::Const => self.constant()?,
                Tok::Program => self.program()?,
                _ => return self.unexpected("a definition"),
            };
            // `%` lines read within a definition go before it.
            self.flush_literals(&mut items);
            items.push(item);
        }
    }

    fn namespace(&mut self) -> PResult<Item> {
        let kw = self.expect(Tok::Namespace)?;
        let name = self.ident()?;
        self.expect_punct(b'{')?;
        let items = self.items(Tok::Punct(b'}'))?;
        let close = self.expect_punct(b'}')?;
        Ok(Item {
            comment: kw.leading,
            kind: ItemKind::Namespace(Named { name, body: items }),
            span: kw.span.to(close.span),
        })
    }

    fn typedef(&mut self) -> PResult<Item> {
        let kw = self.expect(Tok::Typedef)?;
        let (_, decl) = self.declaration()?;
        let semi = self.expect_punct(b';')?;
        self.declare_type(&decl.name, "typedef", kw.span);
        Ok(Item {
            comment: combine(&kw.leading, &semi.trailing),
            kind: ItemKind::Typedef(decl),
            span: kw.span.to(semi.span),
        })
    }

    /// `enum`, `struct` or `union NAME BODY;`
    fn type_definition(&mut self) -> PResult<Item> {
        let kw = self.bump();
        let name = self.ident()?;
        let (kind, what) = match kw.kind {
            Tok::Enum => (
                ItemKind::Enum(Named {
                    name: name.clone(),
                    body: self.enum_body()?,
                }),
                "enum",
            ),
            Tok::Struct => (
                ItemKind::Struct(Named {
                    name: name.clone(),
                    body: self.struct_body()?,
                }),
                "struct",
            ),
            _ => (
                ItemKind::Union(Named {
                    name: name.clone(),
                    body: self.union_body()?,
                }),
                "union",
            ),
        };
        let semi = self.expect_punct(b';')?;
        self.declare_type(&name, what, kw.span);
        Ok(Item {
            comment: kw.leading,
            kind,
            span: kw.span.to(semi.span),
        })
    }

    /// Records a type's name, reporting it if it is already taken.
    fn declare_type(&mut self, name: &Ident, what: &str, span: Span) {
        if !self.type_names.insert(name.name.clone()) {
            self.diags.push(Diagnostic::error(
                span,
                format!("duplicate declaration of {what} `{}`", name.name),
            ));
        }
    }

    fn constant(&mut self) -> PResult<Item> {
        let kw = self.expect(Tok::Const)?;
        let name = self.ident()?;
        self.expect_punct(b'=')?;
        let value = self.number()?;
        let semi = self.expect_punct(b';')?;
        Ok(Item {
            comment: combine(&kw.leading, &semi.trailing),
            kind: ItemKind::Const(Const { name, value }),
            span: kw.span.to(semi.span),
        })
    }

    fn program(&mut self) -> PResult<Item> {
        let kw = self.expect(Tok::Program)?;
        let name = self.ident()?;
        self.expect_punct(b'{')?;
        let mut versions = vec![self.version()?];
        while self.peek_kind() == Tok::Version {
            versions.push(self.version()?);
        }
        self.expect_punct(b'}')?;
        self.expect_punct(b'=')?;
        let value = self.value()?;
        let semi = self.expect_punct(b';')?;
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

    fn version(&mut self) -> PResult<Version> {
        self.expect(Tok::Version)?;
        let name = self.ident()?;
        self.expect_punct(b'{')?;
        let mut procs = vec![self.procedure()?];
        while self.peek_kind() != Tok::Punct(b'}') {
            procs.push(self.procedure()?);
        }
        self.expect_punct(b'}')?;
        self.expect_punct(b'=')?;
        let value = self.value()?;
        self.expect_punct(b';')?;
        Ok(Version { name, value, procs })
    }

    fn procedure(&mut self) -> PResult<Procedure> {
        let leading = self.peek().leading.clone();
        let result = match self.eat(Tok::Void) {
            Some(_) => None,
            None => Some(self.scalar_type()?.0),
        };
        let name = self.ident()?;
        self.expect_punct(b'(')?;
        let mut args = Vec::new();
        if self.eat(Tok::Void).is_none() {
            args.push(self.scalar_type()?.0);
            while self.eat(Tok::Punct(b',')).is_some() {
                args.push(self.scalar_type()?.0);
            }
        }
        self.expect_punct(b')')?;
        self.expect_punct(b'=')?;
        let value = self.value()?;
        let semi = self.expect_punct(b';')?;
        Ok(Procedure {
            comment: combine(&leading, &semi.trailing),
            name,
            value,
            result,
            args,
        })
    }

    // ---- types ------------------------------------------------------------

    fn enum_body(&mut self) -> PResult<EnumBody> {
        self.expect_punct(b'{')?;
        let mut members = vec![self.enum_member()?];
        while let Some(comma) = self.eat(Tok::Punct(b',')) {
            let prev = members.last_mut().expect("an enum has a member");
            prev.comment = combine(&prev.comment, &comma.trailing);
            if self.peek_kind() == Tok::Punct(b'}') {
                self.warn(comma.span, "RFC 4506 does not allow a trailing comma");
                break;
            }
            members.push(self.enum_member()?);
        }
        self.expect_punct(b'}')?;
        Ok(EnumBody { members })
    }

    fn enum_member(&mut self) -> PResult<EnumMember> {
        let leading = self.peek().leading.clone();
        let name = self.ident()?;
        self.expect_punct(b'=')?;
        let trailing = self.peek().trailing.clone();
        let value = self.value()?;
        Ok(EnumMember {
            comment: combine(&leading, &trailing),
            name,
            value,
        })
    }

    fn struct_body(&mut self) -> PResult<StructBody> {
        self.expect_punct(b'{')?;
        let mut fields = vec![self.member()?];
        while self.peek_kind() != Tok::Punct(b'}') {
            fields.push(self.member()?);
        }
        self.expect_punct(b'}')?;
        Ok(StructBody { fields })
    }

    fn union_body(&mut self) -> PResult<UnionBody> {
        self.expect(Tok::Switch)?;
        self.expect_punct(b'(')?;
        let (switch_type, _) = self.scalar_type()?;
        let switch_name = self.ident()?;
        self.expect_punct(b')')?;
        self.expect_punct(b'{')?;
        let mut arms = Vec::new();
        loop {
            let mut cases = Vec::new();
            while self.eat(Tok::Case).is_some() {
                let value = self.value()?;
                if let Value::Ident(i) = &value
                    && (i.name == "true" || i.name == "false")
                {
                    let span = i.span;
                    self.warn(span, "RFC 4506 spells the boolean constants TRUE and FALSE");
                }
                cases.push(value);
                self.expect_punct(b':')?;
            }
            let is_default = match self.eat(Tok::Default) {
                Some(kw) => {
                    if !cases.is_empty() {
                        self.warn(
                            kw.span,
                            "RFC 4506 does not allow case labels on the default arm",
                        );
                    }
                    self.expect_punct(b':')?;
                    true
                }
                None if cases.is_empty() => return self.unexpected("`case` or `default`"),
                None => false,
            };
            arms.push(Arm {
                cases,
                is_default,
                member: self.member()?,
            });
            // The default arm comes last, and there is at least one arm.
            if is_default || self.peek_kind() == Tok::Punct(b'}') {
                break;
            }
        }
        self.expect_punct(b'}')?;
        Ok(UnionBody {
            switch_type,
            switch_name,
            arms,
        })
    }

    /// A struct field or union arm: a declaration or `void`, then `;`.
    fn member(&mut self) -> PResult<Member> {
        let first = self.peek().clone();
        let decl = match self.eat(Tok::Void) {
            Some(_) => None,
            None => Some(self.declaration()?.1),
        };
        let semi = self.expect_punct(b';')?;
        Ok(Member {
            comment: combine(&first.leading, &semi.trailing),
            decl,
            span: first.span.to(semi.span),
        })
    }

    /// A declaration, with the span of its first token.
    fn declaration(&mut self) -> PResult<(Span, Declaration)> {
        let first = self.peek().span;
        let decl = match self.peek_kind() {
            Tok::Opaque => {
                self.bump();
                let name = self.ident()?;
                let ty = if self.eat(Tok::Punct(b'[')).is_some() {
                    let len = self.value()?;
                    self.expect_punct(b']')?;
                    DeclType::Opaque { len }
                } else if self.eat(Tok::Punct(b'<')).is_some() {
                    DeclType::VarOpaque { max: self.bound()? }
                } else {
                    return self.unexpected("`[` or `<`");
                };
                Declaration { name, ty }
            }
            Tok::String => {
                self.bump();
                let name = self.ident()?;
                self.expect_punct(b'<')?;
                let max = self.bound()?;
                Declaration {
                    name,
                    ty: DeclType::String { max },
                }
            }
            _ => {
                let elem = self.type_spec()?;
                if self.eat(Tok::Punct(b'*')).is_some() {
                    Declaration {
                        name: self.ident()?,
                        ty: DeclType::Optional(elem),
                    }
                } else {
                    let name = self.ident()?;
                    let ty = if self.eat(Tok::Punct(b'[')).is_some() {
                        let len = self.value()?;
                        self.expect_punct(b']')?;
                        DeclType::Array { elem, len }
                    } else if self.eat(Tok::Punct(b'<')).is_some() {
                        DeclType::VarArray {
                            elem,
                            max: self.bound()?,
                        }
                    } else {
                        DeclType::Plain(elem)
                    };
                    Declaration { name, ty }
                }
            }
        };
        Ok((first, decl))
    }

    /// The rest of `<MAX>` or `<>`, after the `<`.
    fn bound(&mut self) -> PResult<Option<Value>> {
        let max = match self.peek_kind() {
            Tok::Punct(b'>') => None,
            _ => Some(self.value()?),
        };
        self.expect_punct(b'>')?;
        Ok(max)
    }

    fn type_spec(&mut self) -> PResult<TypeSpec> {
        match self.peek_kind() {
            Tok::Enum => {
                self.bump();
                Ok(TypeSpec::Enum(self.enum_body()?))
            }
            Tok::Struct => {
                self.bump();
                Ok(TypeSpec::Struct(self.struct_body()?))
            }
            Tok::Union => {
                self.bump();
                Ok(TypeSpec::Union(Box::new(self.union_body()?)))
            }
            _ => Ok(self.scalar_type()?.0),
        }
    }

    /// A built-in or named type, with its first token.
    fn scalar_type(&mut self) -> PResult<(TypeSpec, Token)> {
        let token = self.peek().clone();
        let ty = match token.kind {
            Tok::Int => TypeSpec::Int,
            Tok::Hyper => TypeSpec::Hyper,
            Tok::Float => TypeSpec::Float,
            Tok::Double => TypeSpec::Double,
            Tok::Quadruple => TypeSpec::Quadruple,
            Tok::Bool => TypeSpec::Bool,
            Tok::Unsigned => {
                self.bump();
                return Ok((
                    match self.peek_kind() {
                        Tok::Int => {
                            self.bump();
                            TypeSpec::UnsignedInt
                        }
                        Tok::Hyper => {
                            self.bump();
                            TypeSpec::UnsignedHyper
                        }
                        _ => {
                            self.warn(
                                token.span,
                                "RFC 4506 requires `int` or `hyper` after `unsigned` \
                                 (assuming `unsigned int`)",
                            );
                            TypeSpec::UnsignedInt
                        }
                    },
                    token,
                ));
            }
            Tok::Ident => {
                let ident = self.ident()?;
                let instead = match ident.name.as_str() {
                    "int32_t" => Some("int"),
                    "uint32_t" => Some("unsigned int"),
                    "int64_t" => Some("hyper"),
                    "uint64_t" => Some("unsigned hyper"),
                    _ => None,
                };
                if let Some(instead) = instead {
                    self.warn(
                        ident.span,
                        format!("use `{instead}` in XDR source to get `{}`", ident.name),
                    );
                }
                return Ok((TypeSpec::Named(ident), token));
            }
            _ => return self.unexpected("a type"),
        };
        self.bump();
        Ok((ty, token))
    }

    // ---- names and values -------------------------------------------------

    fn ident(&mut self) -> PResult<Ident> {
        let token = self.expect(Tok::Ident)?;
        Ok(Ident {
            name: self.text(&token).to_owned(),
            span: token.span,
        })
    }

    fn value(&mut self) -> PResult<Value> {
        match self.peek_kind() {
            Tok::Ident => Ok(Value::Ident(self.ident()?)),
            Tok::Number => Ok(Value::Number(self.number()?)),
            _ => self.unexpected("a number or identifier"),
        }
    }

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
