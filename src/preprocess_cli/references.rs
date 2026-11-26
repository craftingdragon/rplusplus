// SPDX-License-Identifier: GPL-3.0-or-later
use super::{lex, mapped_error, pp, tree::Role};
use crate::Result;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path, PathBuf},
};

fn literal(token: &lex::Token, text: &str) -> Option<String> {
    use lex::{
        Kind::Literal,
        Literal::{RawString, String as Cooked},
    };
    let text = text.replace("\r\n", "\n");
    if token.kind == Literal(RawString) {
        let quote = text.find('"')?;
        let hashes = quote - 1;
        if !text.ends_with(&format!("\"{}", "#".repeat(hashes))) || text.len() < quote + 2 + hashes
        {
            return None;
        }
        return Some(text[quote + 1..text.len() - hashes - 1].into());
    }
    if token.kind != Literal(Cooked) {
        return None;
    }
    let mut chars = text.strip_prefix('"')?.chars().peekable();
    let mut output = String::new();
    while let Some(c) = chars.next() {
        let decoded = match c {
            '"' => return chars.next().is_none().then_some(output),
            '\\' => match chars.next()? {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                '0' => '\0',
                '\\' => '\\',
                '"' => '"',
                '\'' => '\'',
                'x' => {
                    char::from_u32(chars.next()?.to_digit(16)? * 16 + chars.next()?.to_digit(16)?)?
                }
                'u' => {
                    if chars.next()? != '{' {
                        return None;
                    }
                    let mut digits = String::new();
                    loop {
                        match chars.next()? {
                            '}' => break,
                            '_' => (),
                            c => digits.push(c),
                        }
                    }
                    char::from_u32(u32::from_str_radix(&digits, 16).ok()?)?
                }
                '\n' => {
                    while chars
                        .next_if(|c| matches!(c, ' ' | '\t' | '\n' | '\r'))
                        .is_some()
                    {}
                    continue;
                }
                _ => return None,
            },
            _ => c,
        };
        output.push(decoded);
    }
    None
}

fn resolve(base: &Path, value: &str) -> Option<PathBuf> {
    if value.is_empty() || value.contains('\0') {
        return None;
    }
    let mut path = base.to_owned();
    for part in Path::new(value).components() {
        match part {
            Component::Normal(p) => path.push(p),
            Component::CurDir => (),
            Component::ParentDir if path.pop() => (),
            _ => return None,
        }
    }
    (!path.starts_with(".rpp")).then_some(path)
}

fn macro_definition(lexed: &lex::Lexed<'_>, token: &lex::Token) -> bool {
    lexed.protected.iter().any(|r| {
        r.kind == lex::Protection::MacroDefinition
            && r.span.start <= token.span.start
            && token.span.end <= r.span.end
    })
}

fn implicit_match(path: &Path, name: &str) -> bool {
    let Some(file) = path.file_name().and_then(|p| p.to_str()) else {
        return false;
    };
    let candidate = if file == "mod.rs" {
        // mod.rs can also be the name.rs candidate of r#mod.
        if name == "mod" {
            return true;
        }
        path.parent()
            .and_then(Path::file_name)
            .and_then(|p| p.to_str())
    } else {
        file.strip_suffix(".rs")
    };
    candidate.is_some_and(|c| c == name || !name.is_ascii() || !c.is_ascii())
}

struct Scan<'a> {
    source: &'a Path,
    output: &'a pp::Output,
    originals: &'a pp::Sources,
    lexed: lex::Lexed<'a>,
}

impl Scan<'_> {
    fn error(&self, token: &lex::Token, message: String) -> Box<dyn std::error::Error> {
        mapped_error(
            self.output,
            self.originals,
            self.source,
            token.span,
            pp::ErrorKind::InvalidInput,
            message,
        )
        .into()
    }

    fn imports(
        &self,
        roles: &BTreeMap<PathBuf, Role>,
        included: &mut BTreeSet<PathBuf>,
    ) -> Result<()> {
        use lex::Kind::{Close, Identifier, Open, Punct};
        let tokens: Vec<_> = self
            .lexed
            .tokens
            .iter()
            .filter(|t| !t.kind.trivia())
            .collect();
        for (i, t) in tokens.windows(3).enumerate() {
            if !matches!(t[0].kind, Identifier { .. })
                || t[1].kind != Punct('!')
                || macro_definition(&self.lexed, t[0])
            {
                continue;
            }
            let Open(delimiter) = t[2].kind else { continue };
            let name = self.lexed.text(t[0].span).trim_start_matches("r#");
            let (role, flag) = match name {
                "include" => (Role::Rust, "--source"),
                "include_str" | "include_bytes" => (Role::Data, "--data"),
                _ => continue,
            };
            let close = Close(match delimiter {
                '(' => ')',
                '{' => '}',
                _ => ']',
            });
            let tail = &tokens[i + 3..];
            let value = tail
                .first()
                .and_then(|t| literal(t, self.lexed.text(t.span)));
            let Some(value) = value.filter(|_| {
                tail.get(1).is_some_and(|t| t.kind == close)
                    || tail.get(1).is_some_and(|t| t.kind == Punct(','))
                        && tail.get(2).is_some_and(|t| t.kind == close)
            }) else {
                return Err(self.error(
                    t[0],
                    format!("{name}! requires one literal path to a declared {flag} input"),
                ));
            };
            let path = resolve(self.source.parent().unwrap(), &value).ok_or_else(|| {
                self.error(
                    t[0],
                    format!("{name}! path is absolute or escapes the source root: {value:?}"),
                )
            })?;
            if roles.get(&path) != Some(&role) {
                return Err(self.error(
                    t[0],
                    format!("{name}! requires {flag} for {}", path.display()),
                ));
            }
            if role == Role::Rust {
                included.insert(path);
            }
        }
        Ok(())
    }

    fn modules(&self, roles: &BTreeMap<PathBuf, Role>, included: &BTreeSet<PathBuf>) -> Result<()> {
        use lex::Kind::{Close, Identifier, Open, Punct};
        let tokens: Vec<_> = self
            .lexed
            .tokens
            .iter()
            .filter(|t| !t.kind.trivia())
            .collect();
        let data = || {
            roles
                .iter()
                .filter(|(_, r)| **r == Role::Data)
                .map(|(p, _)| p)
        };
        // Keep depth for source-level attributes. Inline module and include!
        // contexts may change the base rustc selects for a path attribute.
        let mut braces = 0usize;
        for (i, t) in tokens.iter().enumerate() {
            let text = self.lexed.text(t.span);
            let module = tokens.get(i + 1..i + 3).filter(|rest| {
                text == "mod"
                    && !self.lexed.protected(t.span)
                    && matches!(rest[0].kind, Identifier { .. })
                    && rest[1].kind == Punct(';')
            });
            if let Some(rest) = module {
                let name = self.lexed.text(rest[0].span).trim_start_matches("r#");
                if let Some(path) = data().find(|p| implicit_match(p, name)) {
                    return Err(self.error(t, format!("module {name} could load --data {}; declare Rust modules with --source or move the asset", path.display())));
                }
            }
            if text.trim_start_matches("r#") == "path"
                && tokens.get(i + 1).is_some_and(|t| t.kind == Punct('='))
                && !macro_definition(&self.lexed, t)
            {
                let attribute = self.lexed.protected.iter().find(|r| {
                    if r.kind != lex::Protection::Attribute
                        || r.span.start > t.span.start
                        || t.span.end > r.span.end
                    {
                        return false;
                    }
                    let begin = tokens.partition_point(|a| a.span.start < r.span.start);
                    tokens[begin..i]
                        .iter()
                        .find(|a| matches!(a.kind, Identifier { .. }))
                        .map_or(true, |a| {
                            self.lexed.text(a.span).trim_start_matches("r#") == "cfg_attr"
                        })
                });
                if attribute.is_some() {
                    let value = tokens
                        .get(i + 2)
                        .and_then(|v| literal(v, self.lexed.text(v.span)))
                        .filter(|_| {
                            tokens
                                .get(i + 3)
                                .is_some_and(|v| matches!(v.kind, Close(']' | ')') | Punct(',')))
                        })
                        .ok_or_else(|| {
                            self.error(t, "#[path] requires a literal relative path".into())
                        })?;
                    let exact = braces == 0 && !included.contains(self.source);
                    let base = if exact {
                        self.source.parent().unwrap()
                    } else {
                        Path::new("")
                    };
                    let path = resolve(base, &value).ok_or_else(|| self.error(t,
                        format!("#[path] must stay inside the mirror with a statically bounded base: {value:?}")))?;
                    let conflict = data().find(|p| {
                        if path.as_os_str().is_empty() {
                            false
                        } else if exact {
                            **p == path
                        } else {
                            p.ends_with(&path)
                        }
                    });
                    if let Some(conflict) = conflict {
                        return Err(self.error(t, format!("#[path] could load --data {}; declare it with --source or move the asset", conflict.display())));
                    }
                    // rustc resolves the actual #[path] target, including paths
                    // attached to inline module directories rather than files.
                }
            }
            match t.kind {
                Open('{') => braces += 1,
                Close('}') => braces = braces.saturating_sub(1),
                _ => (),
            }
        }
        Ok(())
    }
}

pub(super) fn validate(
    outputs: &BTreeMap<PathBuf, pp::Output>,
    roles: &BTreeMap<PathBuf, Role>,
    originals: &pp::Sources,
    edition: lex::Edition,
) -> Result<()> {
    let mut included = BTreeSet::new();
    let mut scans = Vec::new();
    for (source, output) in outputs {
        let lexed = lex::scan_generated(&output.text, edition).map_err(|e| {
            mapped_error(
                output,
                originals,
                source,
                e.span,
                pp::ErrorKind::Lexical,
                format!("generated Rust: {:?}", e.kind),
            )
        })?;
        let scan = Scan {
            source,
            output,
            originals,
            lexed,
        };
        scan.imports(roles, &mut included)?;
        scans.push(scan);
    }
    for scan in scans {
        scan.modules(roles, &included)?;
    }
    Ok(())
}
