//! Splits a `.x` file into tokens, attaching comments to them.
//!
//! At each position the lexer takes the longest match among its rules,
//! preferring the earlier rule on a tie:
//!
//! 1. a newline
//! 2. a run of spaces, tabs, carriage returns and form feeds
//! 3. at the start of a line, a *leading comment*: one or more lines that
//!    each hold only a `/* */` comment, or one or more lines that each hold
//!    only a `//` comment, through the final newline
//! 4. at the start of a line, a `%` literal, through the end of the line
//! 5. any other comment, a *trailing comment*: `/* ... */`, or `//` through
//!    the end of the line together with any following lines that start
//!    with `//` but not `///`
//! 6. a keyword or identifier
//! 7. a number, with an optional sign, which may contain `'` and `_`
//!    separators
//! 8. a punctuation character
//!
//! A leading comment is attached to the next token, and a trailing comment
//! to the previous one. See [`crate::ast`] for how the parser assembles
//! them into a node's comment.

use crate::diag::{Diagnostic, LineIndex, Span};

/// The kind of a token.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tok {
    Const,
    Struct,
    Union,
    Enum,
    Typedef,
    Program,
    Namespace,
    Bool,
    Unsigned,
    Int,
    Hyper,
    Float,
    Double,
    Quadruple,
    Void,
    Version,
    Switch,
    Case,
    Default,
    Opaque,
    String,
    Ident,
    Number,
    /// A `%` line.
    Literal,
    /// One of `=;{}<>[]*,:()`.
    Punct(u8),
    Eof,
}

impl Tok {
    /// How the token is described in a syntax error.
    pub(crate) fn describe(self) -> &'static str {
        match self {
            Tok::Const => "`const`",
            Tok::Struct => "`struct`",
            Tok::Union => "`union`",
            Tok::Enum => "`enum`",
            Tok::Typedef => "`typedef`",
            Tok::Program => "`program`",
            Tok::Namespace => "`namespace`",
            Tok::Bool => "`bool`",
            Tok::Unsigned => "`unsigned`",
            Tok::Int => "`int`",
            Tok::Hyper => "`hyper`",
            Tok::Float => "`float`",
            Tok::Double => "`double`",
            Tok::Quadruple => "`quadruple`",
            Tok::Void => "`void`",
            Tok::Version => "`version`",
            Tok::Switch => "`switch`",
            Tok::Case => "`case`",
            Tok::Default => "`default`",
            Tok::Opaque => "`opaque`",
            Tok::String => "`string`",
            Tok::Ident => "an identifier",
            Tok::Number => "a number",
            Tok::Literal => "a `%` line",
            Tok::Punct(b'=') => "`=`",
            Tok::Punct(b';') => "`;`",
            Tok::Punct(b'{') => "`{`",
            Tok::Punct(b'}') => "`}`",
            Tok::Punct(b'<') => "`<`",
            Tok::Punct(b'>') => "`>`",
            Tok::Punct(b'[') => "`[`",
            Tok::Punct(b']') => "`]`",
            Tok::Punct(b'*') => "`*`",
            Tok::Punct(b',') => "`,`",
            Tok::Punct(b':') => "`:`",
            Tok::Punct(b'(') => "`(`",
            Tok::Punct(b')') => "`)`",
            Tok::Punct(_) => "punctuation",
            Tok::Eof => "the end of the file",
        }
    }
}

const KEYWORDS: &[(&str, Tok)] = &[
    ("const", Tok::Const),
    ("struct", Tok::Struct),
    ("union", Tok::Union),
    ("enum", Tok::Enum),
    ("typedef", Tok::Typedef),
    ("program", Tok::Program),
    ("namespace", Tok::Namespace),
    ("bool", Tok::Bool),
    ("unsigned", Tok::Unsigned),
    ("int", Tok::Int),
    ("hyper", Tok::Hyper),
    ("float", Tok::Float),
    ("double", Tok::Double),
    ("quadruple", Tok::Quadruple),
    ("void", Tok::Void),
    ("version", Tok::Version),
    ("switch", Tok::Switch),
    ("case", Tok::Case),
    ("default", Tok::Default),
    ("opaque", Tok::Opaque),
    ("string", Tok::String),
];

/// C keywords that RFC 4506 reserves but does not use.
const RESERVED: &[&str] = &["char", "short", "inline", "sizeof"];

/// A token, with the comments attached to it.
#[derive(Clone, Debug)]
pub(crate) struct Token {
    pub(crate) kind: Tok,
    pub(crate) span: Span,
    /// The leading comments before the token. A blank line between them
    /// and the token is kept as an extra trailing newline.
    pub(crate) leading: String,
    /// The trailing comment after the token, indented to the column where
    /// it starts, with a trailing newline.
    pub(crate) trailing: String,
}

/// The tokens of a file, ending with [`Tok::Eof`], and its header comment.
#[derive(Debug)]
pub(crate) struct Lexed {
    pub(crate) tokens: Vec<Token>,
    pub(crate) header: String,
}

pub(crate) fn lex(src: &str, diags: &mut Vec<Diagnostic>) -> Lexed {
    Lexer {
        src: src.as_bytes(),
        lines: LineIndex::new(src),
        pos: 0,
        tokens: Vec::new(),
        leading: String::new(),
        leading_end_line: 0,
        header: String::new(),
        diags,
    }
    .run(src)
}

/// What matched at a position, in order of preference on a tie.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Match {
    Newline,
    Space,
    LeadingComment,
    Literal,
    TrailingComment,
    Word,
    Number,
    Punct,
}

struct Lexer<'s, 'd> {
    src: &'s [u8],
    lines: LineIndex<'s>,
    pos: usize,
    tokens: Vec<Token>,
    /// Leading comments not yet attached to a token.
    leading: String,
    /// The line just past the last leading comment.
    leading_end_line: usize,
    header: String,
    diags: &'d mut Vec<Diagnostic>,
}

impl Lexer<'_, '_> {
    fn run(mut self, src: &str) -> Lexed {
        while self.pos < self.src.len() {
            let start = self.pos;
            let Some((m, len)) = self.longest_match() else {
                // Skip to the next space, newline or `;`.
                let end = start
                    + self.src[start..]
                        .iter()
                        .position(|b| b" \t\n;".contains(b))
                        .unwrap_or(self.src.len() - start)
                        .max(1);
                let end = (start + 1..=end)
                    .rev()
                    .find(|&e| src.is_char_boundary(e))
                    .unwrap_or(end);
                self.diags.push(Diagnostic::error(
                    Span::new(start, end),
                    format!("unexpected `{}`", &src[start..end]),
                ));
                self.pos = end;
                continue;
            };
            let end = start + len;
            self.pos = end;
            let span = Span::new(start, end);
            let text = &src[start..end];
            match m {
                Match::Newline | Match::Space => {}
                Match::LeadingComment => {
                    self.take_header(start);
                    if !self.leading.is_empty() {
                        self.leading.push('\n');
                    }
                    self.leading.push_str(text);
                    self.leading_end_line = self.lines.line(end);
                }
                Match::TrailingComment => {
                    if let Some(prev) = self.tokens.last_mut() {
                        let col = self.lines.line_col(start).col;
                        prev.trailing = format!("{}{text}\n", " ".repeat(col - 1));
                    } else {
                        // With no token before it, the comment leads.
                        self.take_header(start);
                        if !self.leading.is_empty() {
                            self.leading.push('\n');
                        }
                        self.leading.push_str(text);
                        self.leading.push('\n');
                        self.leading_end_line = self.lines.line(end);
                    }
                }
                Match::Literal => self.push(Tok::Literal, span),
                Match::Number => self.push(Tok::Number, span),
                Match::Punct => self.push(Tok::Punct(self.src[start]), span),
                Match::Word => {
                    if let Some(&(_, kind)) = KEYWORDS.iter().find(|(k, _)| *k == text) {
                        self.push(kind, span);
                    } else if RESERVED.contains(&text) {
                        self.diags.push(Diagnostic::error(
                            span,
                            format!("`{text}` is reserved and cannot be used"),
                        ));
                    } else {
                        self.push(Tok::Ident, span);
                    }
                }
            }
        }
        let end = self.src.len();
        self.push(Tok::Eof, Span::new(end, end));
        Lexed {
            tokens: self.tokens,
            header: self.header,
        }
    }

    fn push(&mut self, kind: Tok, span: Span) {
        self.take_header(span.start);
        if !self.leading.is_empty() && self.lines.line(span.start) > self.leading_end_line {
            // A blank line separates the comment from the token.
            self.leading.push('\n');
        }
        self.tokens.push(Token {
            kind,
            span,
            leading: core::mem::take(&mut self.leading),
            trailing: String::new(),
        });
    }

    /// Before the first token, comments that a blank line sets off from what
    /// follows them, starting at `next`, belong to the file's header.
    fn take_header(&mut self, next: usize) {
        if self.tokens.is_empty()
            && !self.leading.is_empty()
            && self.lines.line(next) > self.leading_end_line
        {
            if !self.header.is_empty() {
                self.header.push('\n');
            }
            self.header.push_str(&core::mem::take(&mut self.leading));
        }
    }

    fn longest_match(&self) -> Option<(Match, usize)> {
        let p = self.pos;
        let bol = p == 0 || self.src[p - 1] == b'\n';
        let candidates = [
            (Match::Newline, (self.src[p] == b'\n').then_some(1)),
            (Match::Space, nonzero(self.span_of(p, b" \t\r\x0c"))),
            (
                Match::LeadingComment,
                bol.then(|| self.block_comment_lines(p).max(self.line_comment_lines(p)))
                    .and_then(nonzero),
            ),
            (
                Match::Literal,
                (bol && self.src[p] == b'%').then(|| self.to_eol(p) - p),
            ),
            (Match::TrailingComment, self.trailing_comment(p)),
            (Match::Word, self.word(p)),
            (Match::Number, self.number(p)),
            (
                Match::Punct,
                b"=;{}<>[]*,:()".contains(&self.src[p]).then_some(1),
            ),
        ];
        // The longest match; on a tie, the first.
        candidates
            .into_iter()
            .filter_map(|(m, len)| Some((m, len?)))
            .fold(None, |best: Option<(Match, usize)>, (m, len)| match best {
                Some((_, l)) if l >= len => best,
                _ => Some((m, len)),
            })
    }

    /// The number of bytes from `p` in `set`.
    fn span_of(&self, p: usize, set: &[u8]) -> usize {
        self.src[p..].iter().take_while(|b| set.contains(b)).count()
    }

    /// The offset of the newline ending the line at `p`, or the end of the
    /// file.
    fn to_eol(&self, p: usize) -> usize {
        self.src[p..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(self.src.len(), |i| p + i)
    }

    /// The end of a `/* */` comment starting at `p`.
    fn block_comment_end(&self, p: usize) -> Option<usize> {
        if !self.src[p..].starts_with(b"/*") {
            return None;
        }
        self.src[p + 2..]
            .windows(2)
            .position(|w| w == b"*/")
            .map(|i| p + 2 + i + 2)
    }

    /// The length of the lines from `p` that each hold only a `/* */`
    /// comment, through the final newline.
    fn block_comment_lines(&self, p: usize) -> usize {
        let mut end = p;
        loop {
            let q = end + self.span_of(end, b" \t");
            let Some(close) = self.block_comment_end(q) else {
                break;
            };
            let after = close + self.span_of(close, b" \t");
            if self.src.get(after) != Some(&b'\n') {
                break;
            }
            end = after + 1;
        }
        end - p
    }

    /// The length of the lines from `p` that each hold only a `//` comment,
    /// through the final newline.
    fn line_comment_lines(&self, p: usize) -> usize {
        let mut end = p;
        loop {
            let q = end + self.span_of(end, b" \t");
            if !self.src[q..].starts_with(b"//") {
                break;
            }
            let eol = self.to_eol(q);
            if eol == self.src.len() {
                break;
            }
            end = eol + 1;
        }
        end - p
    }

    /// The length of a comment starting at `p`. A `//` comment continues
    /// onto following lines that start with `//` but not `///`.
    fn trailing_comment(&self, p: usize) -> Option<usize> {
        if let Some(end) = self.block_comment_end(p) {
            return Some(end - p);
        }
        if !self.src[p..].starts_with(b"//") {
            return None;
        }
        let mut end = self.to_eol(p);
        while end < self.src.len() {
            let q = end + 1 + self.span_of(end + 1, b" \t");
            let continues = self.src[q..].starts_with(b"//")
                && self
                    .src
                    .get(q + 2)
                    .is_some_and(|&b| b != b'/' && b != b'\n');
            if !continues {
                break;
            }
            end = self.to_eol(q);
        }
        Some(end - p)
    }

    fn word(&self, p: usize) -> Option<usize> {
        let first = self.src[p];
        (first.is_ascii_alphabetic() || first == b'_').then(|| {
            1 + self.src[p + 1..]
                .iter()
                .take_while(|b| b.is_ascii_alphanumeric() || **b == b'_')
                .count()
        })
    }

    /// `[+-]?([0-9'_]+|0x[0-9a-fA-F'_]+)`
    fn number(&self, p: usize) -> Option<usize> {
        let sign = usize::from(matches!(self.src[p], b'+' | b'-'));
        let q = p + sign;
        let dec = self.src[q..]
            .iter()
            .take_while(|b| b.is_ascii_digit() || **b == b'\'' || **b == b'_')
            .count();
        let hex = if self.src[q..].starts_with(b"0x") {
            self.src[q + 2..]
                .iter()
                .take_while(|b| b.is_ascii_hexdigit() || **b == b'\'' || **b == b'_')
                .count()
        } else {
            0
        };
        let len = if hex > 0 { (2 + hex).max(dec) } else { dec };
        (len > 0).then_some(sign + len)
    }
}

fn nonzero(n: usize) -> Option<usize> {
    (n > 0).then_some(n)
}
