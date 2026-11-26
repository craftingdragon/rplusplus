// SPDX-License-Identifier: GPL-3.0-or-later
//! Lossless Rust lexical boundaries for optional preprocessing. Not a Rust parser.
mod scanner;
mod unicode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edition {
    Rust2021,
    Rust2024,
}

/// Half-open offsets into the ORIGINAL UTF-8 bytes. No CRLF/BOM normalization.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Position {
    pub byte: usize,
    pub line: usize,
    /// One-based Unicode scalar column, not terminal display width.
    pub column: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Literal {
    Number,
    Character,
    Byte,
    String,
    ByteString,
    CString,
    RawString,
    RawByteString,
    RawCString,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Bom,
    Shebang,
    Whitespace,
    LineComment,
    BlockComment,
    Identifier {
        raw: bool,
    },
    /// Labels and lifetime parameters have the same lexical representation.
    Lifetime {
        raw: bool,
    },
    Literal(Literal),
    /// Unglued punctuation: adjacency is preserved by byte spans.
    Punct(char),
    Open(char),
    Close(char),
    /// Whole physical line excluding its CRLF/LF. No directive interpretation.
    DirectiveLine {
        name: Span,
    },
}

impl Kind {
    pub fn trivia(self) -> bool {
        matches!(
            self,
            Self::Bom | Self::Shebang | Self::Whitespace | Self::LineComment | Self::BlockComment
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    pub kind: Kind,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protection {
    Attribute,
    MacroInvocation,
    MacroDefinition,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub kind: Protection,
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    InvalidUtf8,
    UnknownCharacter,
    UnterminatedComment,
    UnterminatedLiteral,
    InvalidLiteral,
    InvalidEscape,
    InvalidRawDelimiter,
    ReservedSyntax,
    MismatchedDelimiter,
    UnclosedDelimiter,
    NestingLimit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    pub span: Span,
    pub opening: Option<Span>,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:?} at bytes {}..{}",
            self.kind, self.span.start, self.span.end
        )
    }
}
impl std::error::Error for Error {}

#[derive(Debug)]
pub struct Lexed<'a> {
    pub source: &'a str,
    pub tokens: Vec<Token>,
    pub protected: Vec<Region>,
    line_starts: Vec<usize>,
}

impl Lexed<'_> {
    pub fn text(&self, span: Span) -> &str {
        &self.source[span.start..span.end]
    }

    /// None for out-of-range offsets or offsets inside a UTF-8 scalar.
    pub fn position(&self, byte: usize) -> Option<Position> {
        if byte > self.source.len() || !self.source.is_char_boundary(byte) {
            return None;
        }
        let line = self.line_starts.partition_point(|&p| p <= byte) - 1;
        let start = self.line_starts[line];
        let prefix = &self.source[start..byte];
        let mut column = prefix.chars().count() + 1;
        if start == 0 && prefix.starts_with('\u{feff}') {
            column -= 1;
        }
        // Rust normalizes CRLF. Keep raw offsets but do not count its CR.
        if prefix.ends_with('\r') && self.source[byte..].starts_with('\n') {
            column -= 1;
        }
        Some(Position {
            byte,
            line: line + 1,
            column,
        })
    }

    pub fn protected(&self, span: Span) -> bool {
        self.protected
            .iter()
            .any(|r| r.span.start <= span.start && span.end <= r.span.end)
    }

    /// Eligibility only; M1 never expands or replaces a token.
    pub fn substitutable(&self, token: &Token) -> bool {
        matches!(token.kind, Kind::Identifier { raw: false }) && !self.protected(token.span)
    }
}

pub fn scan(source: &str, edition: Edition) -> Result<Lexed<'_>, Error> {
    scanner::scan(source, edition, true, true)
}

/// Rust-token fragments have neither preprocessing directives nor file prologues.
pub fn scan_fragment(source: &str, edition: Edition) -> Result<Lexed<'_>, Error> {
    scanner::scan(source, edition, false, false)
}

/// Generated Rust keeps file prologues, but cannot introduce directives.
pub fn scan_generated(source: &str, edition: Edition) -> Result<Lexed<'_>, Error> {
    scanner::scan(source, edition, false, true)
}

/// Original-source locations are also available when lexical scanning failed.
pub fn source_position(source: &str, byte: usize) -> Option<Position> {
    if byte > source.len() || !source.is_char_boundary(byte) {
        return None;
    }
    let start = source[..byte].rfind('\n').map_or(0, |p| p + 1);
    let line = source[..byte].bytes().filter(|&b| b == b'\n').count() + 1;
    let prefix = &source[start..byte];
    let mut column = prefix.chars().count() + 1;
    if start == 0 && prefix.starts_with('\u{feff}') {
        column -= 1;
    }
    if prefix.ends_with('\r') && source[byte..].starts_with('\n') {
        column -= 1;
    }
    Some(Position { byte, line, column })
}

pub fn scan_bytes(source: &[u8], edition: Edition) -> Result<Lexed<'_>, Error> {
    let text = std::str::from_utf8(source).map_err(|e| Error {
        kind: ErrorKind::InvalidUtf8,
        span: Span {
            start: e.valid_up_to(),
            end: e.valid_up_to() + e.error_len().unwrap_or(source.len() - e.valid_up_to()),
        },
        opening: None,
    })?;
    scan(text, edition)
}

#[cfg(test)]
mod tests;
