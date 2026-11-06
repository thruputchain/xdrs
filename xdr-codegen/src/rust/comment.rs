//! Comments and the code-generation markers they carry.
//!
//! A comment in a `.x` file becomes a comment in the generated Rust:
//! `///` lines become doc comments, and `//` and `/* */` comments become
//! plain `//` comments. Generated code is built with `quote`, which keeps
//! only doc attributes, so a plain comment line travels as a doc attribute
//! whose text starts with [`PLAIN`], and [`finish`] turns those lines back
//! into `//` comments after printing.

use proc_macro2::TokenStream;
use quote::quote;

/// Marks a doc attribute that stands for a plain `//` comment line.
const PLAIN: char = '\u{1}';

/// Code-generation markers found in a comment, such as `#[rust(newtype)]`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Markers {
    /// `#[rust(newtype)]` on a typedef: a struct wrapping the type, rather
    /// than an alias.
    pub(crate) newtype: bool,
    /// `#[rust(box_ptr)]` on a field or arm: box the type, for recursion.
    pub(crate) box_ptr: bool,
    /// `#[rust(const=u32)]` on a constant (the default).
    pub(crate) const_u32: bool,
    /// `#[rust(const=u64)]` on a constant.
    pub(crate) const_u64: bool,
    /// `#[rust(ConstWriteXDR)]`: also derive `ConstWriteXDR`.
    pub(crate) const_write_xdr: bool,
    /// `#[rust(no_debug)]`: leave `Debug` to hand-written code.
    pub(crate) no_debug: bool,
    /// `#[rust(no_eq)]`: derive only the partial comparison traits.
    pub(crate) no_eq: bool,
    /// `#[doc(hidden)]`: hide the item from rustdoc.
    pub(crate) doc_hidden: bool,
}

/// Sets the flag for a marker.
type SetMarker = fn(&mut Markers);

const MARKERS: &[(&str, SetMarker)] = &[
    ("#[rust(newtype)]", |m| m.newtype = true),
    ("#[rust(box_ptr)]", |m| m.box_ptr = true),
    ("#[rust(const=u32)]", |m| m.const_u32 = true),
    ("#[rust(const=u64)]", |m| m.const_u64 = true),
    ("#[rust(ConstWriteXDR)]", |m| m.const_write_xdr = true),
    ("#[rust(no_debug)]", |m| m.no_debug = true),
    ("#[rust(no_eq)]", |m| m.no_eq = true),
    ("#[doc(hidden)]", |m| m.doc_hidden = true),
];

/// A comment, as the generated code shows it.
#[derive(Clone, Debug, Default)]
pub(crate) struct Comment {
    pub(crate) markers: Markers,
    lines: Vec<Line>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Line {
    /// A `///` line, without the `///`.
    Doc(String),
    /// A `//` line or a line of a `/* */` comment, without the delimiters.
    Plain(String),
}

impl Comment {
    /// An empty comment with the given markers.
    pub(crate) fn with_markers(markers: Markers) -> Self {
        Self {
            markers,
            lines: Vec::new(),
        }
    }

    /// Parses a comment as the AST keeps it, removing its markers.
    pub(crate) fn parse(text: &str) -> Self {
        let mut markers = Markers::default();
        let mut text = text.to_owned();
        for (marker, set) in MARKERS {
            if text.contains(marker) {
                set(&mut markers);
                text = text.replace(marker, "");
            }
        }

        let mut lines = Vec::new();
        let mut block: Option<bool> = None; // Inside `/* */`: whether `/**`.
        for line in text.lines() {
            let t = line.trim_start();
            if let Some(doc) = block {
                let (body, closed) = match t.find("*/") {
                    Some(i) => (&t[..i], true),
                    None => (t, false),
                };
                push(&mut lines, doc, body.trim_start_matches('*'));
                if closed {
                    block = None;
                }
            } else if let Some(rest) = t.strip_prefix("/*") {
                let doc = rest.starts_with('*') && !rest.starts_with("*/");
                let rest = rest.trim_start_matches('*');
                match rest.find("*/") {
                    Some(i) => push(&mut lines, doc, &rest[..i]),
                    None => {
                        push(&mut lines, doc, rest);
                        block = Some(doc);
                    }
                }
            } else if let Some(rest) = t.strip_prefix("///").filter(|r| !r.starts_with('/')) {
                lines.push(Line::Doc(rest.trim_end().to_owned()));
            } else if let Some(rest) = t.strip_prefix("//") {
                // A line left empty by a removed marker is dropped.
                if !rest.trim().is_empty() {
                    lines.push(Line::Plain(rest.trim_end().to_owned()));
                }
            } else if !t.is_empty() {
                lines.push(Line::Plain(format!(" {}", t.trim_end())));
            }
        }
        Self { markers, lines }
    }

    /// The comment as attributes on the item it documents, followed by
    /// `#[doc(hidden)]` if it was marked so.
    pub(crate) fn attrs(&self) -> TokenStream {
        let lines = self.lines.iter().map(|line| match line {
            Line::Doc(text) => quote! { #[doc = #text] },
            Line::Plain(text) => {
                let text = format!("{PLAIN}{text}");
                quote! { #[doc = #text] }
            }
        });
        let hidden = self.markers.doc_hidden.then(|| quote! { #[doc(hidden)] });
        quote! { #(#lines)* #hidden }
    }
}

/// Plain `//` comment lines for the generated code itself.
pub(crate) fn plain(lines: &[&str]) -> TokenStream {
    let lines = lines.iter().map(|line| format!("{PLAIN} {line}"));
    quote! { #(#[doc = #lines])* }
}

fn push(lines: &mut Vec<Line>, doc: bool, body: &str) {
    let body = body.trim_end();
    if body.trim().is_empty() {
        return;
    }
    let body = format!(" {}", body.trim_start());
    lines.push(if doc {
        Line::Doc(body)
    } else {
        Line::Plain(body)
    });
}

/// Turns the doc comments that stand for plain comments into `//` comments.
pub(crate) fn finish(printed: &str) -> String {
    let marker = format!("///{PLAIN}");
    let mut out = String::with_capacity(printed.len());
    for line in printed.lines() {
        match line.find(&marker) {
            Some(i) if line[..i].trim().is_empty() => {
                out.push_str(&line[..i]);
                out.push_str("//");
                out.push_str(&line[i + marker.len()..]);
            }
            _ => out.push_str(line),
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(text: &str) -> Vec<Line> {
        Comment::parse(text).lines
    }

    #[test]
    fn styles() {
        assert_eq!(
            lines("/// Doc.\n// Plain.\n////// Banner\n"),
            [
                Line::Doc(" Doc.".into()),
                Line::Plain(" Plain.".into()),
                Line::Plain("//// Banner".into()),
            ]
        );
        assert_eq!(
            lines("/* one\n * two */\n/** doc */\n"),
            [
                Line::Plain(" one".into()),
                Line::Plain(" two".into()),
                Line::Doc(" doc".into()),
            ]
        );
    }

    #[test]
    fn markers_are_removed() {
        let c =
            Comment::parse("/// A thing.\n// #[rust(newtype)]\n          // #[doc(hidden)] more\n");
        assert!(c.markers.newtype && c.markers.doc_hidden);
        assert!(!c.markers.box_ptr);
        assert_eq!(
            c.lines,
            [Line::Doc(" A thing.".into()), Line::Plain("  more".into())]
        );
    }

    #[test]
    fn plain_lines_print_as_comments() {
        let c = Comment::parse("/// Doc.\n// Plain.\n");
        let attrs = c.attrs();
        let file: syn::File = syn::parse2(quote! { #attrs struct S; }).unwrap();
        assert_eq!(
            finish(&prettyplease::unparse(&file)),
            "/// Doc.\n// Plain.\nstruct S;\n"
        );
    }
}
