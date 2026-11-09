//! Parser and code generator for [XDR][RFC-4506] specifications, in the
//! dialect of `.x` files that x23c accepts.
//!
//! [`parse`] turns a `.x` file into an [`ast::Spec`]. See [`ast`] for the
//! dialect and for how comments are attached to definitions.
//!
//! ```
//! let src = "\
//! // A point.
//! struct Point {
//!     int x;  // across
//!     int y;  // down
//! };
//! ";
//! let parsed = xdr_codegen::parse(src).unwrap();
//! let item = &parsed.spec.items[0];
//! assert_eq!(item.comment, "// A point.\n");
//! let xdr_codegen::ast::ItemKind::Struct(s) = &item.kind else { panic!() };
//! assert_eq!(s.name.name, "Point");
//! // Indented to where the comment starts.
//! assert_eq!(s.body.fields[1].comment, "            // down\n");
//! ```
//!
//! [RFC-4506]: https://datatracker.ietf.org/doc/html/rfc4506

pub mod ast;
mod check;
pub mod diag;
mod lexer;
mod parser;
pub mod rust;

use diag::{Diagnostic, Severity};

/// A successfully parsed file.
#[derive(Clone, Debug)]
pub struct Parsed {
    /// The file's syntax tree.
    pub spec: ast::Spec,
    /// Warnings, in source order.
    pub warnings: Vec<Diagnostic>,
}

/// Parses the text of a `.x` file.
///
/// On failure, returns every diagnostic, errors and warnings, in source
/// order. Lexical errors are all reported; parsing stops at the first
/// syntax error. Duplicate type names, and the warnings about names and
/// values, are reported only if the file parses.
pub fn parse(src: &str) -> Result<Parsed, Vec<Diagnostic>> {
    let mut diags = Vec::new();
    let lexed = lexer::lex(src, &mut diags);
    let items = parser::Parser::new(src, lexed.tokens, &mut diags).parse();
    if let Some(items) = &items {
        check::check(items, &mut diags);
    }
    diags.sort_by_key(|d| d.span.start);
    match items {
        Some(items) if diags.iter().all(|d| d.severity == Severity::Warning) => Ok(Parsed {
            spec: ast::Spec {
                header: lexed.header,
                items,
            },
            warnings: diags,
        }),
        _ => Err(diags),
    }
}
