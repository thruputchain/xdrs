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
    ///
    /// A line that holds only markers is dropped. An empty comment line is
    /// kept, as it separates paragraphs, except at the start or end of a
    /// `/* */` comment.
    pub(crate) fn parse(text: &str) -> Self {
        let mut markers = Markers::default();
        let mut lines = Vec::new();
        // Inside `/* */`: whether `/**`, and where its lines start.
        let mut block: Option<(bool, usize)> = None;
        for line in text.lines() {
            let mut line = line.to_owned();
            let mut had_marker = false;
            for (marker, set) in MARKERS {
                if line.contains(marker) {
                    set(&mut markers);
                    line = line.replace(marker, "");
                    had_marker = true;
                }
            }
            let t = line.trim_start();
            if let Some((doc, start)) = block {
                let (body, closed) = match t.find("*/") {
                    Some(i) => (&t[..i], true),
                    None => (t, false),
                };
                if !(had_marker && body.trim_matches(['*', ' ', '\t']).is_empty()) {
                    push(&mut lines, doc, body.trim_start_matches('*'), true);
                }
                if closed {
                    trim_blank(&mut lines, start);
                    block = None;
                }
            } else if let Some(rest) = t.strip_prefix("/*") {
                let doc = rest.starts_with('*') && !rest.starts_with("*/");
                let rest = rest.trim_start_matches('*');
                match rest.find("*/") {
                    Some(i) => push(&mut lines, doc, &rest[..i], false),
                    None => {
                        let start = lines.len();
                        push(&mut lines, doc, rest, true);
                        block = Some((doc, start));
                    }
                }
            } else if let Some(rest) = t.strip_prefix("///").filter(|r| !r.starts_with('/')) {
                if !(had_marker && rest.trim().is_empty()) {
                    lines.push(Line::Doc(rest.trim_end().to_owned()));
                }
            } else if let Some(rest) = t.strip_prefix("//") {
                if rest.trim().is_empty() {
                    if !had_marker {
                        lines.push(Line::Plain(String::new()));
                    }
                } else {
                    lines.push(Line::Plain(rest.trim_end().to_owned()));
                }
            } else if !t.is_empty() {
                lines.push(Line::Plain(format!(" {}", t.trim_end())));
            }
        }
        if let Some((_, start)) = block {
            trim_blank(&mut lines, start);
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

/// Adds a line of a `/* */` comment. An empty one is kept only if
/// `keep_empty`, for a comment of several lines.
fn push(lines: &mut Vec<Line>, doc: bool, body: &str, keep_empty: bool) {
    let body = body.trim();
    if body.is_empty() && !keep_empty {
        return;
    }
    let body = if body.is_empty() {
        String::new()
    } else {
        format!(" {body}")
    };
    lines.push(if doc {
        Line::Doc(body)
    } else {
        Line::Plain(body)
    });
}

/// Removes the empty lines at the start and end of `lines[start..]`, the
/// lines of one `/* */` comment.
fn trim_blank(lines: &mut Vec<Line>, start: usize) {
    let is_blank = |l: &Line| match l {
        Line::Doc(t) | Line::Plain(t) => t.is_empty(),
    };
    while lines.len() > start && lines.last().is_some_and(is_blank) {
        lines.pop();
    }
    let leading = lines[start..].iter().take_while(|l| is_blank(l)).count();
    lines.drain(start..start + leading);
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
    fn empty_lines_separate_paragraphs() {
        assert_eq!(
            lines("// One.\n//\n// Two.\n///\n/// Doc.\n"),
            [
                Line::Plain(" One.".into()),
                Line::Plain("".into()),
                Line::Plain(" Two.".into()),
                Line::Doc("".into()),
                Line::Doc(" Doc.".into()),
            ]
        );
        // Not at the edges of a `/* */` comment.
        assert_eq!(
            lines("/*\n * One.\n *\n * Two.\n */\n"),
            [
                Line::Plain(" One.".into()),
                Line::Plain("".into()),
                Line::Plain(" Two.".into()),
            ]
        );
    }

    #[test]
    fn marker_lines_are_dropped() {
        let c = Comment::parse("// One.\n// #[rust(newtype)]\n//\n/// #[doc(hidden)]\n");
        assert!(c.markers.newtype && c.markers.doc_hidden);
        assert_eq!(
            c.lines,
            [Line::Plain(" One.".into()), Line::Plain("".into())]
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
