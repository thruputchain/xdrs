//! Splits a `.x` file into tokens, attaching comments to them.
//!
//! The lexer works like x23c's flex scanner (`lex.ll`): [`RULES`] is a list
//! of regular expressions, and at each position the lexer takes the longest
//! match among them, preferring the earlier rule on a tie. So `const` is a
//! keyword but `constant` an identifier. `(?m:^)` anchors a rule to the
//! start of a line, like flex's `^`.
//!
//! A leading comment is attached to the next token, and a trailing comment
//! to the previous one. See [`crate::ast`] for how the parser assembles
//! them into a node's comment.

use std::sync::LazyLock;

use regex_automata::{
    Anchored, MatchKind,
    dfa::{Automaton, StartKind, dense, dense::DFA},
    util::start,
};

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

/// What a rule does with its match.
#[derive(Clone, Copy, Debug)]
enum Rule {
    /// Discard it.
    Skip,
    /// Attach it, as a comment, to the next token.
    Leading,
    /// Attach it, as a comment, to the previous token.
    Trailing,
    /// Report a reserved word.
    Reserved,
    /// Emit a [`Tok::Punct`].
    Punct,
    /// Emit a token.
    Tok(Tok),
}

/// One or more lines that each hold only a `/* */` comment.
const BLOCK_COMMENTS: &str = r"(?m:^)([ \t]*/\*([^*]|\*+[^*/])*\*+/[ \t]*\n)+";
/// One or more lines that each hold only a `//` comment.
const LINE_COMMENTS: &str = r"(?m:^)([ \t]*//.*\n)+";
/// A `/* */` comment, or a `//` comment continued by following lines that
/// start with `//` but not `///`. Unlike x23c's rule, which ends in `\*/`,
/// this one accepts a comment ending in `**/`.
const COMMENT: &str = r"/\*([^*]|\*+[^*/])*\*+/|//.*(\n[ \t]*//[^/\n].*)*";
const ID: &str = r"[a-zA-Z_][a-zA-Z_0-9]*";
const NUM: &str = r"[+-]?([0-9'_]+|0x[0-9a-fA-F'_]+)";

/// The rules, in order of preference on a tie.
const RULES: &[(Rule, &str)] = &[
    (Rule::Skip, r"\n"),
    (Rule::Skip, r"[ \t\r\x0c]+"),
    (Rule::Leading, BLOCK_COMMENTS),
    (Rule::Leading, LINE_COMMENTS),
    (Rule::Tok(Tok::Literal), r"(?m:^)%.*"),
    (Rule::Trailing, COMMENT),
    (Rule::Tok(Tok::Const), "const"),
    (Rule::Tok(Tok::Struct), "struct"),
    (Rule::Tok(Tok::Union), "union"),
    (Rule::Tok(Tok::Enum), "enum"),
    (Rule::Tok(Tok::Typedef), "typedef"),
    (Rule::Tok(Tok::Program), "program"),
    (Rule::Tok(Tok::Namespace), "namespace"),
    (Rule::Tok(Tok::Bool), "bool"),
    (Rule::Tok(Tok::Unsigned), "unsigned"),
    (Rule::Tok(Tok::Int), "int"),
    (Rule::Tok(Tok::Hyper), "hyper"),
    (Rule::Tok(Tok::Float), "float"),
    (Rule::Tok(Tok::Double), "double"),
    (Rule::Tok(Tok::Quadruple), "quadruple"),
    (Rule::Tok(Tok::Void), "void"),
    (Rule::Tok(Tok::Version), "version"),
    (Rule::Tok(Tok::Switch), "switch"),
    (Rule::Tok(Tok::Case), "case"),
    (Rule::Tok(Tok::Default), "default"),
    (Rule::Tok(Tok::Opaque), "opaque"),
    (Rule::Tok(Tok::String), "string"),
    // C keywords that RFC 4506 reserves but does not use.
    (Rule::Reserved, "char|short|inline|sizeof"),
    (Rule::Tok(Tok::Ident), ID),
    (Rule::Tok(Tok::Number), NUM),
    (Rule::Punct, r"[=;{}<>\[\]*,:()]"),
];

/// Where no rule matches: the unexpected character and what follows it up
/// to the next space, newline or `;`, which the lexer skips.
const UNEXPECTED: &str = r"(?s:.)[^\n \t;]*";

/// A DFA that recognizes `patterns`, keeping every pattern that matches in
/// its match states, so that [`longest_match`] can choose among them.
fn dfa(patterns: &[&str]) -> DFA<Vec<u32>> {
    dense::Builder::new()
        .configure(
            dense::Config::new()
                .match_kind(MatchKind::All)
                .start_kind(StartKind::Anchored),
        )
        .build_many(patterns)
        .expect("lexer rules are valid")
}

/// All of [`RULES`] as one automaton, as flex builds it.
static RULES_DFA: LazyLock<DFA<Vec<u32>>> = LazyLock::new(|| {
    let patterns: Vec<&str> = RULES.iter().map(|&(_, pattern)| pattern).collect();
    dfa(&patterns)
});

static UNEXPECTED_DFA: LazyLock<DFA<Vec<u32>>> = LazyLock::new(|| dfa(&[UNEXPECTED]));

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
        src,
        lines: LineIndex::new(src),
        tokens: Vec::new(),
        leading: String::new(),
        leading_end_line: 0,
        header: String::new(),
        diags,
    }
    .run()
}

struct Lexer<'s, 'd> {
    src: &'s str,
    lines: LineIndex<'s>,
    tokens: Vec<Token>,
    /// Leading comments not yet attached to a token.
    leading: String,
    /// The line just past the last leading comment.
    leading_end_line: usize,
    header: String,
    diags: &'d mut Vec<Diagnostic>,
}

impl Lexer<'_, '_> {
    fn run(mut self) -> Lexed {
        let mut pos = 0;
        while pos < self.src.len() {
            let Some((rule, len)) = longest_match(&RULES_DFA, self.src, pos) else {
                let (_, len) =
                    longest_match(&UNEXPECTED_DFA, self.src, pos).expect("matches any character");
                let span = Span::new(pos, pos + len);
                self.diags.push(Diagnostic::error(
                    span,
                    format!("unexpected `{}`", &self.src[pos..pos + len]),
                ));
                pos += len;
                continue;
            };
            let span = Span::new(pos, pos + len);
            self.apply(RULES[rule].0, span);
            pos += len;
        }
        let end = self.src.len();
        self.push(Tok::Eof, Span::new(end, end));
        Lexed {
            tokens: self.tokens,
            header: self.header,
        }
    }

    /// Carries out the action of `rule`, which matched `span`.
    fn apply(&mut self, rule: Rule, span: Span) {
        let text = &self.src[span.start..span.end];
        match rule {
            Rule::Skip => {}
            Rule::Leading => self.lead(text, span),
            Rule::Trailing => {
                if let Some(prev) = self.tokens.last_mut() {
                    let col = self.lines.line_col(span.start).col;
                    prev.trailing = format!("{}{text}\n", " ".repeat(col - 1));
                } else {
                    // With no token before it, the comment leads.
                    self.lead(&format!("{text}\n"), span);
                }
            }
            Rule::Reserved => self.diags.push(Diagnostic::error(
                span,
                format!("`{text}` is reserved and cannot be used"),
            )),
            Rule::Punct => self.push(Tok::Punct(text.as_bytes()[0]), span),
            Rule::Tok(kind) => self.push(kind, span),
        }
    }

    /// Adds `text`, a comment ending with a newline, to the leading
    /// comments.
    fn lead(&mut self, text: &str, span: Span) {
        self.take_header(span.start);
        if !self.leading.is_empty() {
            self.leading.push('\n');
        }
        self.leading.push_str(text);
        self.leading_end_line = self.lines.line(span.end);
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
}

/// The longest non-empty match of `dfa` starting at `pos` in `src`, as the
/// index of the pattern and the length. On a tie, the pattern with the
/// lowest index wins.
fn longest_match(dfa: &DFA<Vec<u32>>, src: &str, pos: usize) -> Option<(usize, usize)> {
    let src = src.as_bytes();
    // The byte before `pos` decides whether `(?m:^)` matches.
    let start = start::Config::new()
        .anchored(Anchored::Yes)
        .look_behind(pos.checked_sub(1).map(|i| src[i]));
    let mut state = dfa
        .start_state(&start)
        .expect("the DFA has anchored starts");
    let mut best = None;
    for end in pos..=src.len() {
        // The DFA reports a match one byte late, on the transition past its
        // end, or on the end-of-input transition.
        state = match src.get(end) {
            Some(&b) => dfa.next_state(state, b),
            None => dfa.next_eoi_state(state),
        };
        if dfa.is_dead_state(state) {
            break;
        }
        assert!(
            !dfa.is_quit_state(state),
            "the lexer rules use no quit bytes"
        );
        if dfa.is_match_state(state) && end > pos {
            let pattern = (0..dfa.match_len(state))
                .map(|i| dfa.match_pattern(state, i).as_usize())
                .min()
                .expect("a match state has a pattern");
            best = Some((pattern, end - pos));
        }
    }
    best
}
