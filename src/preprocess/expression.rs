// SPDX-License-Identifier: GPL-3.0-or-later
use crate::lex::{self, Edition, Kind, Literal, Span};

#[derive(Debug)]
pub(super) struct Error {
    pub span: Span,
    pub message: String,
}
#[derive(Clone, Copy)]
enum Value {
    Signed(i64),
    MinMagnitude(Span),
}
#[derive(Clone)]
struct Item {
    text: String,
    kind: Kind,
    span: Span,
}
struct Parser<'a> {
    items: Vec<Item>,
    at: usize,
    end: usize,
    defined: &'a dyn Fn(&str) -> bool,
}

impl Parser<'_> {
    fn err(&self, message: &str) -> Error {
        Error {
            span: self.items.get(self.at).map_or(
                Span {
                    start: self.end,
                    end: self.end,
                },
                |t| t.span,
            ),
            message: message.into(),
        }
    }
    fn take(&mut self, s: &str) -> bool {
        if self.items.get(self.at).is_some_and(|t| t.text == s) {
            self.at += 1;
            true
        } else {
            false
        }
    }
    fn signed(&self, v: Value, enabled: bool) -> Result<i64, Error> {
        if !enabled {
            return Ok(0);
        }
        match v {
            Value::Signed(n) => Ok(n),
            Value::MinMagnitude(span) => Err(Error {
                span,
                message: "integer is outside signed 64-bit range".into(),
            }),
        }
    }
    fn prefix(&mut self, enabled: bool, depth: usize) -> Result<Value, Error> {
        if depth > 256 {
            return Err(self.err("expression nesting exceeds 256"));
        }
        for op in ["!", "+", "-"] {
            let op_span = self.items.get(self.at).map(|t| t.span);
            if self.take(op) {
                let v = self.prefix(enabled, depth + 1)?;
                if !enabled {
                    return Ok(Value::Signed(0));
                }
                if op == "-" && matches!(v, Value::MinMagnitude(_)) {
                    return Ok(Value::Signed(i64::MIN));
                }
                let n = self.signed(v, true)?;
                return Ok(Value::Signed(match op {
                    "!" => i64::from(n == 0),
                    "+" => n,
                    _ => n.checked_neg().ok_or_else(|| Error {
                        span: op_span.unwrap(),
                        message: "signed negation overflow".into(),
                    })?,
                }));
            }
        }
        if self.take("(") {
            let n = self.expr(0, enabled, depth + 1)?;
            if !self.take(")") {
                return Err(self.err("expected ')'"));
            }
            return Ok(n);
        }
        if self.take("defined") {
            let paren = self.take("(");
            let t = self
                .items
                .get(self.at)
                .ok_or_else(|| self.err("defined requires an ASCII macro name"))?;
            if t.kind != (Kind::Identifier { raw: false }) || !super::name(&t.text) {
                return Err(self.err("defined requires an ASCII macro name"));
            }
            let value = enabled && (self.defined)(&t.text);
            self.at += 1;
            if paren && !self.take(")") {
                return Err(self.err("expected ')' after defined operand"));
            }
            return Ok(Value::Signed(i64::from(value)));
        }
        let t = self
            .items
            .get(self.at)
            .ok_or_else(|| self.err("expected an integer expression"))?;
        if t.kind == (Kind::Identifier { raw: false }) && super::name(&t.text) {
            self.at += 1;
            return Ok(Value::Signed(0));
        }
        if t.kind != Kind::Literal(Literal::Number) {
            return Err(
                self.err("expected an unsuffixed integer, name or parenthesized expression")
            );
        }
        let text = t.text.replace('_', "");
        let (radix, digits) = if let Some(s) = text.strip_prefix("0x") {
            (16, s)
        } else if let Some(s) = text.strip_prefix("0o") {
            (8, s)
        } else if let Some(s) = text.strip_prefix("0b") {
            (2, s)
        } else {
            (10, text.as_str())
        };
        let n = u64::from_str_radix(digits, radix)
            .ok()
            .filter(|&n| n <= (1u64 << 63))
            .ok_or_else(|| {
                self.err("expected an unsuffixed integer within signed 64-bit magnitude")
            })?;
        let span = t.span;
        self.at += 1;
        Ok(if n == 1u64 << 63 {
            Value::MinMagnitude(span)
        } else {
            Value::Signed(n as i64)
        })
    }
    fn expr(&mut self, min: u8, enabled: bool, depth: usize) -> Result<Value, Error> {
        let mut left = self.prefix(enabled, depth)?;
        loop {
            let Some(op) = self.items.get(self.at).map(|t| t.text.clone()) else {
                break;
            };
            let prec = match op.as_str() {
                "||" => 1,
                "&&" => 2,
                "==" | "!=" => 3,
                "<" | "<=" | ">" | ">=" => 4,
                _ => break,
            };
            if prec < min {
                break;
            }
            let a = self.signed(left, enabled)?;
            self.at += 1;
            let rhs_enabled = enabled && !(op == "&&" && a == 0 || op == "||" && a != 0);
            let right = self.expr(prec + 1, rhs_enabled, depth + 1)?;
            let b = self.signed(right, rhs_enabled)?;
            left = Value::Signed(if !enabled {
                0
            } else {
                i64::from(match op.as_str() {
                    "||" => a != 0 || b != 0,
                    "&&" => a != 0 && b != 0,
                    "==" => a == b,
                    "!=" => a != b,
                    "<" => a < b,
                    "<=" => a <= b,
                    ">" => a > b,
                    _ => a >= b,
                })
            });
        }
        Ok(left)
    }
}

pub(super) fn evaluate(
    text: &str,
    edition: Edition,
    defined: &dyn Fn(&str) -> bool,
) -> Result<i64, Error> {
    let lexed = lex::scan_fragment(text, edition).map_err(|e| Error {
        span: e.span,
        message: format!("invalid expression token: {:?}", e.kind),
    })?;
    let mut items: Vec<Item> = Vec::new();
    for token in lexed.tokens.iter().filter(|t| !t.kind.trivia()) {
        let value = lexed.text(token.span);
        if let Some(last) = items.last_mut() {
            let joint = format!("{}{value}", last.text);
            if last.span.end == token.span.start
                && matches!(joint.as_str(), "&&" | "||" | "==" | "!=" | "<=" | ">=")
            {
                last.text = joint;
                last.span.end = token.span.end;
                continue;
            }
        }
        items.push(Item {
            text: value.into(),
            kind: token.kind,
            span: token.span,
        });
    }
    let mut p = Parser {
        items,
        at: 0,
        end: text.len(),
        defined,
    };
    let n = p.expr(0, true, 0)?;
    if p.at != p.items.len() {
        return Err(p.err("unexpected token or unsupported operator"));
    }
    p.signed(n, true)
}
