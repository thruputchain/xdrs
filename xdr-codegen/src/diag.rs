//! Source positions and diagnostics.

use core::fmt;

/// A byte range in the source text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Span {
    /// The offset of the first byte.
    pub start: usize,
    /// The offset just past the last byte.
    pub end: usize,
}

impl Span {
    /// The span from `start` to `end`.
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    /// The smallest span covering both `self` and `other`.
    pub fn to(self, other: Self) -> Self {
        Self::new(self.start.min(other.start), self.end.max(other.end))
    }
}

/// A line and column in the source text, both counted from 1.
///
/// Columns count characters, with a tab advancing to the next multiple of 8
/// plus one, as compilers conventionally report them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LineCol {
    /// The line, from 1.
    pub line: usize,
    /// The column, from 1.
    pub col: usize,
}

/// Maps byte offsets in a source text to lines and columns.
#[derive(Clone, Debug)]
pub struct LineIndex<'s> {
    src: &'s str,
    /// The offset at which each line starts.
    starts: Vec<usize>,
}

impl<'s> LineIndex<'s> {
    /// Indexes the lines of `src`.
    pub fn new(src: &'s str) -> Self {
        let starts = core::iter::once(0)
            .chain(src.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        Self { src, starts }
    }

    /// The line and column of byte offset `offset`.
    pub fn line_col(&self, offset: usize) -> LineCol {
        let line = self.line(offset);
        let start = self.starts[line - 1];
        let end = offset.min(self.src.len());
        let col = self.src[start..end].chars().fold(1, |col, c| match c {
            '\t' => col + 8 - (col - 1) % 8,
            _ => col + 1,
        });
        LineCol { line, col }
    }

    /// The line of byte offset `offset`, from 1.
    pub fn line(&self, offset: usize) -> usize {
        self.starts.partition_point(|&s| s <= offset)
    }
}

/// How serious a diagnostic is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    /// The source is accepted, but departs from RFC 4506 or from convention.
    /// `--strict` makes these fatal.
    Warning,
    /// The source is rejected.
    Error,
}

/// A message about a place in the source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// How serious it is.
    pub severity: Severity,
    /// Where it applies.
    pub span: Span,
    /// What it says.
    pub message: String,
}

impl Diagnostic {
    pub(crate) fn error(span: Span, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Error,
            span,
            message: message.into(),
        }
    }

    pub(crate) fn warning(span: Span, message: impl Into<String>) -> Self {
        Self {
            severity: Severity::Warning,
            span,
            message: message.into(),
        }
    }

    /// Displays the diagnostic as `file:line:col: warning: message` or `... error: message`.
    pub fn display<'a>(
        &'a self,
        file: &'a str,
        lines: &'a LineIndex<'_>,
    ) -> impl fmt::Display + 'a {
        struct Show<'a, 'l> {
            diag: &'a Diagnostic,
            file: &'a str,
            lines: &'a LineIndex<'l>,
        }
        impl fmt::Display for Show<'_, '_> {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                let LineCol { line, col } = self.lines.line_col(self.diag.span.start);
                let level = match self.diag.severity {
                    Severity::Warning => "warning: ",
                    Severity::Error => "error: ",
                };
                write!(
                    f,
                    "{}:{line}:{col}: {level}{}",
                    self.file, self.diag.message
                )
            }
        }
        Show {
            diag: self,
            file,
            lines,
        }
    }
}
