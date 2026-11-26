// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;

fn property(c: char, ranges: &[(u32, u32)]) -> bool {
    let n = c as u32;
    let i = ranges.partition_point(|&(start, _)| start <= n);
    i > 0 && n <= ranges[i - 1].1
}
fn start(c: char) -> bool {
    c == '_' || property(c, unicode::XID_START)
}
fn continuation(c: char) -> bool {
    !matches!(c, '\u{200c}' | '\u{200d}') && property(c, unicode::XID_CONTINUE)
}
fn whitespace(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n'
            | '\x0b'
            | '\x0c'
            | '\r'
            | ' '
            | '\u{85}'
            | '\u{200e}'
            | '\u{200f}'
            | '\u{2028}'
            | '\u{2029}'
    )
}
fn error(kind: ErrorKind, start: usize, end: usize) -> Error {
    Error {
        kind,
        span: Span { start, end },
        opening: None,
    }
}

struct Frame {
    close: char,
    opening: Span,
    region: Option<(usize, Protection)>,
}
struct Scanner<'a> {
    source: &'a str,
    edition: Edition,
    directives: bool,
    at: usize,
    tokens: Vec<Token>,
    significant: Vec<usize>,
    frames: Vec<Frame>,
    regions: Vec<Region>,
}

impl Scanner<'_> {
    fn rest(&self) -> &str {
        &self.source[self.at..]
    }
    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }
    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.at += c.len_utf8();
        Some(c)
    }
    fn identifier(&mut self) {
        while self.peek().is_some_and(continuation) {
            self.bump();
        }
    }
    fn suffix(&mut self) {
        if self.peek().is_some_and(start) {
            self.bump();
            self.identifier();
        }
    }
    fn raw_name(&mut self, begin: usize) -> Result<(), Error> {
        if !self.peek().is_some_and(start) {
            return Err(error(ErrorKind::ReservedSyntax, begin, self.at));
        }
        let name = self.at;
        self.bump();
        self.identifier();
        if matches!(
            &self.source[name..self.at],
            "_" | "crate" | "self" | "Self" | "super"
        ) {
            return Err(error(ErrorKind::ReservedSyntax, begin, self.at));
        }
        Ok(())
    }

    fn directive(&mut self) -> Option<Kind> {
        if !self.directives || !self.frames.is_empty() || !self.rest().starts_with('#') {
            return None;
        }
        let line = self.source[..self.at].rfind('\n').map_or(0, |i| i + 1);
        let prefix = &self.source[line..self.at];
        let prefix = if line == 0 {
            prefix.strip_prefix('\u{feff}').unwrap_or(prefix)
        } else {
            prefix
        };
        if !prefix.bytes().all(|c| c == b' ' || c == b'\t') {
            return None;
        }
        let mut name = self.at + 1;
        while matches!(self.source.as_bytes().get(name), Some(b' ' | b'\t')) {
            name += 1;
        }
        let mut end = name;
        let first = self.source.as_bytes().get(end)?;
        if !first.is_ascii_alphabetic() && *first != b'_' {
            return None;
        }
        end += 1;
        while self
            .source
            .as_bytes()
            .get(end)
            .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_')
        {
            end += 1;
        }
        if self.source[end..].chars().next().is_some_and(continuation) {
            return None;
        }
        self.at += self.rest().find('\n').unwrap_or(self.rest().len());
        if self.source[..self.at].ends_with('\r') {
            self.at -= 1;
        }
        Some(Kind::DirectiveLine {
            name: Span { start: name, end },
        })
    }

    fn block_comment(&mut self) -> Result<Kind, Error> {
        let begin = self.at;
        self.at += 2;
        let mut depth = 1usize;
        while self.at < self.source.len() {
            if self.rest().starts_with("/*") {
                depth += 1;
                self.at += 2;
            } else if self.rest().starts_with("*/") {
                depth -= 1;
                self.at += 2;
                if depth == 0 {
                    return Ok(Kind::BlockComment);
                }
            } else {
                self.bump();
            }
        }
        Err(error(ErrorKind::UnterminatedComment, begin, self.at))
    }

    /// Consume an escape, validating its lexical form, not evaluating literals.
    fn escape(&mut self, byte: bool, c_string: bool, character: bool) -> Result<(), Error> {
        let begin = self.at;
        self.at += 1; // backslash
        let Some(c) = self.bump() else {
            return Err(error(ErrorKind::UnterminatedLiteral, begin, self.at));
        };
        match c {
            'n' | 'r' | 't' | '\\' | '\'' | '"' => (),
            '0' if !c_string => (),
            'x' => {
                let digits = self.at;
                for _ in 0..2 {
                    if !self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                        return Err(error(ErrorKind::InvalidEscape, begin, self.at));
                    }
                    self.bump();
                }
                let n = u8::from_str_radix(&self.source[digits..self.at], 16).unwrap();
                if (!byte && !c_string && n > 127) || (c_string && n == 0) {
                    return Err(error(ErrorKind::InvalidEscape, begin, self.at));
                }
            }
            'u' if !byte => {
                if self.bump() != Some('{') || !self.peek().is_some_and(|c| c.is_ascii_hexdigit()) {
                    return Err(error(ErrorKind::InvalidEscape, begin, self.at));
                }
                let mut value = 0u32;
                let mut count = 0;
                while let Some(c) = self.peek() {
                    if c == '_' {
                        self.bump();
                        continue;
                    }
                    let Some(d) = c.to_digit(16).filter(|_| c.is_ascii()) else {
                        break;
                    };
                    count += 1;
                    if count > 6 {
                        return Err(error(ErrorKind::InvalidEscape, begin, self.at));
                    }
                    value = value * 16 + d;
                    self.bump();
                }
                if self.bump() != Some('}')
                    || char::from_u32(value).is_none()
                    || (c_string && value == 0)
                {
                    return Err(error(ErrorKind::InvalidEscape, begin, self.at));
                }
            }
            '\n' if !character => (),
            '\r' if !character && self.peek() == Some('\n') => {
                self.bump();
            }
            _ => return Err(error(ErrorKind::InvalidEscape, begin, self.at)),
        }
        Ok(())
    }

    fn quoted(&mut self, begin: usize, kind: Literal) -> Result<Kind, Error> {
        let character = matches!(kind, Literal::Character | Literal::Byte);
        let byte = matches!(kind, Literal::Byte | Literal::ByteString);
        let c_string = kind == Literal::CString;
        let quote = if character { '\'' } else { '"' };
        self.at += 1; // opening quote
        let mut units = 0;
        loop {
            let Some(c) = self.peek() else {
                return Err(error(ErrorKind::UnterminatedLiteral, begin, self.at));
            };
            if c == quote {
                self.bump();
                if character && units != 1 {
                    return Err(error(ErrorKind::InvalidLiteral, begin, self.at));
                }
                self.suffix();
                return Ok(Kind::Literal(kind));
            }
            if c == '\\' {
                self.escape(byte, c_string, character)?;
            } else {
                if (byte && !c.is_ascii())
                    || (c_string && c == '\0')
                    || (character && matches!(c, '\t' | '\n' | '\r'))
                    || (c == '\r' && !self.rest().starts_with("\r\n"))
                {
                    return Err(error(
                        ErrorKind::InvalidLiteral,
                        self.at,
                        self.at + c.len_utf8(),
                    ));
                }
                self.bump();
            }
            units += 1;
        }
    }

    fn raw(&mut self, begin: usize, prefix: usize, kind: Literal) -> Result<Option<Kind>, Error> {
        let mut quote = begin + prefix;
        while self.source.as_bytes().get(quote) == Some(&b'#') {
            quote += 1;
        }
        let hashes = quote - begin - prefix;
        if self.source.as_bytes().get(quote) != Some(&b'"') {
            return Ok(None);
        }
        if hashes > 255 {
            return Err(error(ErrorKind::InvalidRawDelimiter, begin, quote));
        }
        self.at = quote + 1;
        loop {
            let Some(c) = self.peek() else {
                return Err(error(ErrorKind::UnterminatedLiteral, begin, self.at));
            };
            if c == '"'
                && self
                    .source
                    .as_bytes()
                    .get(self.at + 1..self.at + 1 + hashes)
                    .is_some_and(|s| s.iter().all(|&c| c == b'#'))
            {
                self.at += 1 + hashes;
                self.suffix();
                return Ok(Some(Kind::Literal(kind)));
            }
            if (kind == Literal::RawByteString && !c.is_ascii())
                || (kind == Literal::RawCString && c == '\0')
                || (c == '\r' && !self.rest().starts_with("\r\n"))
            {
                return Err(error(
                    ErrorKind::InvalidLiteral,
                    self.at,
                    self.at + c.len_utf8(),
                ));
            }
            self.bump();
        }
    }

    fn number(&mut self) -> Result<Kind, Error> {
        let begin = self.at;
        let base = if self.rest().starts_with("0x") {
            16
        } else if self.rest().starts_with("0o") {
            8
        } else if self.rest().starts_with("0b") {
            2
        } else {
            10
        };
        if base != 10 {
            self.at += 2;
            let mut digits = 0;
            while let Some(c) = self.peek() {
                if c == '_' {
                    self.bump();
                    continue;
                }
                if !c.is_ascii() || !(c.is_ascii_digit() || (base == 16 && c.is_ascii_hexdigit())) {
                    break;
                }
                if c.to_digit(base).is_none() {
                    return Err(error(ErrorKind::InvalidLiteral, begin, self.at + 1));
                }
                digits += 1;
                self.bump();
            }
            if digits == 0 {
                return Err(error(ErrorKind::InvalidLiteral, begin, self.at));
            }
            if (base != 16 && matches!(self.peek(), Some('e' | 'E')))
                || (self.peek() == Some('.')
                    && !self.rest().starts_with("..")
                    && !self.rest()[1..].chars().next().is_some_and(start))
            {
                return Err(error(ErrorKind::InvalidLiteral, begin, self.at + 1));
            }
        } else {
            while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '_') {
                self.bump();
            }
            if self.peek() == Some('.')
                && !self.rest().starts_with("..")
                && !self.rest()[1..].chars().next().is_some_and(start)
            {
                self.at += 1;
                while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '_') {
                    self.bump();
                }
            }
            if matches!(self.peek(), Some('e' | 'E')) {
                self.bump();
                if matches!(self.peek(), Some('+' | '-')) {
                    self.bump();
                }
                let mut digits = 0;
                while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '_') {
                    if self.bump() != Some('_') {
                        digits += 1;
                    }
                }
                if digits == 0 {
                    return Err(error(ErrorKind::InvalidLiteral, begin, self.at));
                }
            }
        }
        self.suffix();
        Ok(Kind::Literal(Literal::Number))
    }

    fn token(&mut self) -> Result<Kind, Error> {
        let begin = self.at;
        if let Some(kind) = self.directive() {
            return Ok(kind);
        }
        let c = self.peek().unwrap();
        if whitespace(c) {
            while self.peek().is_some_and(whitespace) {
                self.bump();
            }
            return Ok(Kind::Whitespace);
        }
        if self.rest().starts_with("//") {
            self.at += self.rest().find('\n').unwrap_or(self.rest().len());
            return Ok(Kind::LineComment);
        }
        if self.rest().starts_with("/*") {
            return self.block_comment();
        }
        for (prefix, n, kind) in [
            ("br", 2, Literal::RawByteString),
            ("cr", 2, Literal::RawCString),
            ("r", 1, Literal::RawString),
        ] {
            if self.rest().starts_with(prefix) {
                if let Some(kind) = self.raw(begin, n, kind)? {
                    return Ok(kind);
                }
            }
        }
        for (prefix, kind) in [
            ("b\"", Literal::ByteString),
            ("c\"", Literal::CString),
            ("b'", Literal::Byte),
        ] {
            if self.rest().starts_with(prefix) {
                self.at += 1;
                return self.quoted(begin, kind);
            }
        }
        if c == '"' {
            return self.quoted(begin, Literal::String);
        }
        if self.rest().starts_with("r#") {
            self.at += 2;
            self.raw_name(begin)?;
            return Ok(Kind::Identifier { raw: true });
        }
        if c == '\'' {
            self.at += 1;
            if self.rest().starts_with("r#") {
                self.at += 2;
                self.raw_name(begin)?;
                if self.peek() == Some('\'') {
                    return Err(error(ErrorKind::InvalidLiteral, begin, self.at + 1));
                }
                return Ok(Kind::Lifetime { raw: true });
            }
            if self.peek().is_some_and(start) {
                self.bump();
                self.identifier();
                if self.peek() != Some('\'') {
                    return Ok(Kind::Lifetime { raw: false });
                }
            }
            self.at = begin;
            return self.quoted(begin, Literal::Character);
        }
        if c.is_ascii_digit() {
            return self.number();
        }
        if start(c) {
            self.bump();
            self.identifier();
            if matches!(self.peek(), Some('#' | '\'' | '"')) {
                return Err(error(ErrorKind::ReservedSyntax, begin, self.at + 1));
            }
            return Ok(Kind::Identifier { raw: false });
        }
        if self.edition == Edition::Rust2024
            && c == '#'
            && matches!(self.rest().as_bytes().get(1), Some(b'#' | b'"'))
        {
            return Err(error(ErrorKind::ReservedSyntax, begin, begin + 2));
        }
        self.bump();
        Ok(match c {
            '(' | '[' | '{' => Kind::Open(c),
            ')' | ']' | '}' => Kind::Close(c),
            '!' | '#' | '$' | '%' | '&' | '*' | '+' | ',' | '-' | '.' | '/' | ':' | ';' | '<'
            | '=' | '>' | '?' | '@' | '^' | '|' | '~' => Kind::Punct(c),
            _ => return Err(error(ErrorKind::UnknownCharacter, begin, self.at)),
        })
    }

    fn previous(&self, n: usize) -> Option<&Token> {
        self.significant
            .len()
            .checked_sub(n + 1)
            .map(|i| &self.tokens[self.significant[i]])
    }
    fn punctuation(&self, n: usize, c: char) -> bool {
        self.previous(n).is_some_and(|t| t.kind == Kind::Punct(c))
    }
    fn name(&self, n: usize) -> bool {
        self.previous(n)
            .is_some_and(|t| matches!(t.kind, Kind::Identifier { .. }))
    }
    fn opening_region(&self, open: char) -> Option<(usize, Protection)> {
        if open == '[' {
            if self.punctuation(0, '#') {
                return Some((self.previous(0)?.span.start, Protection::Attribute));
            }
            if self.punctuation(0, '!') && self.punctuation(1, '#') {
                return Some((self.previous(1)?.span.start, Protection::Attribute));
            }
        }
        if self.name(0)
            && self.punctuation(1, '!')
            && self
                .previous(2)
                .is_some_and(|t| self.source[t.span.start..t.span.end] == *"macro_rules")
        {
            return Some((self.previous(2)?.span.start, Protection::MacroDefinition));
        }
        if self.punctuation(0, '!') && self.name(1) {
            let mut n = 1;
            while self.punctuation(n + 1, ':') && self.punctuation(n + 2, ':') && self.name(n + 3) {
                n += 3;
            }
            if self.punctuation(n + 1, ':') && self.punctuation(n + 2, ':') {
                n += 2;
            }
            return Some((self.previous(n)?.span.start, Protection::MacroInvocation));
        }
        None
    }

    fn push(&mut self, token: Token) -> Result<(), Error> {
        match token.kind {
            Kind::Open(c) => {
                let region = self.opening_region(c);
                if region.is_some() || !self.frames.is_empty() {
                    if self.frames.len() >= 4096 {
                        return Err(error(
                            ErrorKind::NestingLimit,
                            token.span.start,
                            token.span.end,
                        ));
                    }
                    self.frames.push(Frame {
                        close: match c {
                            '(' => ')',
                            '[' => ']',
                            _ => '}',
                        },
                        opening: token.span,
                        region,
                    });
                }
            }
            Kind::Close(c) if !self.frames.is_empty() => {
                let frame = self.frames.last().unwrap();
                if frame.close != c {
                    return Err(Error {
                        kind: ErrorKind::MismatchedDelimiter,
                        span: token.span,
                        opening: Some(frame.opening),
                    });
                }
                if let Some((start, kind)) = self.frames.pop().unwrap().region {
                    self.regions.push(Region {
                        kind,
                        span: Span {
                            start,
                            end: token.span.end,
                        },
                    });
                }
            }
            _ => (),
        }
        if matches!(token.kind, Kind::DirectiveLine { .. }) {
            // Directives may surround complete constructs, not divide a macro
            // path/bang from its token tree and retroactively hide a directive.
            self.significant.clear();
        } else if !token.kind.trivia() {
            self.significant.push(self.tokens.len());
        }
        self.tokens.push(token);
        Ok(())
    }
}

/// Whether an initial #! is followed by an attribute, allowing trivia as rustc does.
fn shebang_attribute(source: &str, at: usize) -> bool {
    let mut s = Scanner {
        source,
        at,
        edition: Edition::Rust2024,
        directives: false,
        tokens: vec![],
        significant: vec![],
        frames: vec![],
        regions: vec![],
    };
    loop {
        if s.peek().is_some_and(whitespace) {
            s.bump();
        } else if s.rest().starts_with("//") {
            s.at += s.rest().find('\n').unwrap_or(s.rest().len());
        } else if s.rest().starts_with("/*") {
            if s.block_comment().is_err() {
                return false;
            }
        } else {
            return s.peek() == Some('[');
        }
    }
}

pub(super) fn scan(
    source: &str,
    edition: Edition,
    directives: bool,
    prologue: bool,
) -> Result<Lexed<'_>, Error> {
    let mut s = Scanner {
        source,
        edition,
        directives,
        at: 0,
        tokens: vec![],
        significant: vec![],
        frames: vec![],
        regions: vec![],
    };
    if prologue && source.starts_with('\u{feff}') {
        s.at = 3;
        s.push(Token {
            kind: Kind::Bom,
            span: Span { start: 0, end: 3 },
        })?;
    }
    if prologue && s.rest().starts_with("#!") && !shebang_attribute(source, s.at + 2) {
        let start = s.at;
        s.at += s.rest().find('\n').unwrap_or(s.rest().len());
        s.push(Token {
            kind: Kind::Shebang,
            span: Span { start, end: s.at },
        })?;
    }
    while s.at < source.len() {
        let start = s.at;
        let kind = s.token()?;
        s.push(Token {
            kind,
            span: Span { start, end: s.at },
        })?;
    }
    if let Some(frame) = s.frames.last() {
        return Err(Error {
            kind: ErrorKind::UnclosedDelimiter,
            span: frame.opening,
            opening: Some(frame.opening),
        });
    }
    s.regions
        .sort_by_key(|r| (r.span.start, std::cmp::Reverse(r.span.end)));
    let mut line_starts = vec![0];
    line_starts.extend(
        source
            .bytes()
            .enumerate()
            .filter_map(|(i, b)| (b == b'\n').then_some(i + 1)),
    );
    Ok(Lexed {
        source,
        tokens: s.tokens,
        protected: s.regions,
        line_starts,
    })
}
