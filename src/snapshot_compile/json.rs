// SPDX-License-Identifier: GPL-3.0-or-later
//! JSON data from rustc's line protocol and the versioned source map.
use std::collections::BTreeMap;

pub(super) enum Value {
    Null,
    Bool(bool),
    Number(Option<usize>),
    String(String),
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>),
}

impl Value {
    pub fn get(&self, name: &str) -> Option<&Self> {
        match self {
            Self::Object(fields) => fields.get(name),
            _ => None,
        }
    }
    pub fn text(&self, name: &str) -> Option<&str> {
        match self.get(name)? {
            Self::String(s) => Some(s),
            _ => None,
        }
    }
    pub fn number(&self, name: &str) -> Option<usize> {
        match self.get(name)? {
            Self::Number(n) => *n,
            _ => None,
        }
    }
    pub fn array(&self, name: &str) -> Option<&[Self]> {
        match self.get(name)? {
            Self::Array(items) => Some(items),
            _ => None,
        }
    }
    pub fn yes(&self, name: &str) -> bool {
        matches!(self.get(name), Some(Self::Bool(true)))
    }
}

struct Parser<'a>(&'a str);
impl Parser<'_> {
    fn ws(&mut self) {
        self.0 = self.0.trim_start_matches([' ', '\n', '\r', '\t']);
    }

    fn take(&mut self, s: &str) -> bool {
        self.ws();
        if let Some(tail) = self.0.strip_prefix(s) {
            self.0 = tail;
            true
        } else {
            false
        }
    }

    fn char(&mut self) -> Option<char> {
        let ch = self.0.chars().next()?;
        self.0 = &self.0[ch.len_utf8()..];
        Some(ch)
    }

    fn hex(&mut self) -> Option<u16> {
        let s = self.0.get(..4)?;
        if !s.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        self.0 = &self.0[4..];
        u16::from_str_radix(s, 16).ok()
    }

    fn string(&mut self) -> Option<String> {
        if !self.take("\"") {
            return None;
        }
        let mut out = String::new();
        loop {
            out.push(match self.char()? {
                '"' => return Some(out),
                '\\' => match self.char()? {
                    '"' => '"',
                    '\\' => '\\',
                    '/' => '/',
                    'b' => '\x08',
                    'f' => '\x0c',
                    'n' => '\n',
                    'r' => '\r',
                    't' => '\t',
                    'u' => {
                        let n = self.hex()?;
                        let scalar = if (0xd800..=0xdbff).contains(&n) {
                            self.0 = self.0.strip_prefix("\\u")?;
                            let low = self.hex()?;
                            if !(0xdc00..=0xdfff).contains(&low) {
                                return None;
                            }
                            0x10000 + ((u32::from(n) - 0xd800) << 10) + u32::from(low) - 0xdc00
                        } else {
                            u32::from(n)
                        };
                        char::from_u32(scalar)?
                    }
                    _ => return None,
                },
                c if c >= '\u{20}' => c,
                _ => return None,
            });
        }
    }

    fn digits(&mut self) -> bool {
        let n = self.0.bytes().take_while(u8::is_ascii_digit).count();
        self.0 = &self.0[n..];
        n != 0
    }

    fn number(&mut self) -> Option<Value> {
        let start = self.0;
        if let Some(rest) = self.0.strip_prefix('-') {
            self.0 = rest;
        }
        if let Some(rest) = self.0.strip_prefix('0') {
            self.0 = rest;
        } else if !self.digits() {
            return None;
        }
        if let Some(rest) = self.0.strip_prefix('.') {
            self.0 = rest;
            if !self.digits() {
                return None;
            }
        }
        if let Some(rest) = self.0.strip_prefix(['e', 'E']) {
            self.0 = rest.strip_prefix(['+', '-']).unwrap_or(rest);
            if !self.digits() {
                return None;
            }
        }
        Some(Value::Number(
            start[..start.len() - self.0.len()].parse().ok(),
        ))
    }

    fn value(&mut self, depth: usize) -> Option<Value> {
        if depth == 128 {
            return None;
        }
        self.ws();
        match self.0.as_bytes().first()? {
            b'n' if self.take("null") => Some(Value::Null),
            b't' if self.take("true") => Some(Value::Bool(true)),
            b'f' if self.take("false") => Some(Value::Bool(false)),
            b'"' => Some(Value::String(self.string()?)),
            b'[' => {
                self.take("[");
                let mut items = Vec::new();
                if !self.take("]") {
                    loop {
                        items.push(self.value(depth + 1)?);
                        if self.take("]") {
                            break;
                        }
                        if !self.take(",") {
                            return None;
                        }
                    }
                }
                Some(Value::Array(items))
            }
            b'{' => {
                self.take("{");
                let mut fields = BTreeMap::new();
                if !self.take("}") {
                    loop {
                        let key = self.string()?;
                        if !self.take(":") || fields.insert(key, self.value(depth + 1)?).is_some() {
                            return None;
                        }
                        if self.take("}") {
                            break;
                        }
                        if !self.take(",") {
                            return None;
                        }
                    }
                }
                Some(Value::Object(fields))
            }
            _ => self.number(),
        }
    }
}

pub(super) fn parse(line: &str) -> Option<Value> {
    let mut parser = Parser(line);
    let value = parser.value(0)?;
    parser.ws();
    parser.0.is_empty().then_some(value)
}
