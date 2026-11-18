// SPDX-License-Identifier: GPL-3.0-or-later
use super::*;
use crate::lex::{Kind, Token};

#[derive(Clone)]
struct Definition {
    text: String,
    site: Location,
}
#[derive(Clone)]
struct Piece {
    text: String,
    site: Location,
    exact: bool,
    definitions: Vec<Location>,
    chain: Vec<String>,
    includes: Vec<Location>,
}
#[derive(Clone, Default)]
struct Material(Vec<Piece>);
impl Material {
    fn text(&self) -> String {
        self.0.iter().map(|p| p.text.as_str()).collect()
    }
}
struct Conditional {
    parent: bool,
    selected: bool,
    active: bool,
    seen_else: bool,
    opening: Location,
}
struct FileFrame<'a> {
    path: PathBuf,
    text: &'a str,
    tokens: Vec<Token>,
    next: usize,
    begin: usize,
    conditions: Vec<Conditional>,
}
struct Engine<'a> {
    sources: &'a Sources,
    options: &'a Options,
    macros: BTreeMap<String, Definition>,
    predefined: BTreeMap<String, String>,
    includes: Vec<Location>,
    files: Vec<PathBuf>,
    dirs: Vec<PathBuf>,
    output: Output,
    output_limit: usize,
}

impl<'a> Engine<'a> {
    fn text(&self, source: &SourceName) -> &str {
        match source {
            SourceName::File(p) => &self.sources.files[p],
            SourceName::Predefined(n) => &self.predefined[n],
        }
    }
    fn loc(&self, source: SourceName, span: Span) -> Location {
        let p = lex::source_position(self.text(&source), span.start).expect("internal source span");
        Location {
            source,
            span,
            line: p.line,
            column: p.column,
        }
    }
    fn err(&self, kind: ErrorKind, message: impl Into<String>, location: Location) -> Error {
        Error {
            kind,
            message: message.into(),
            location,
            notes: vec![],
            include_stack: self.includes.clone(),
        }
    }
    fn lexical(&self, source: &SourceName, offset: usize, e: lex::Error) -> Error {
        let at = self.loc(
            source.clone(),
            Span {
                start: offset + e.span.start,
                end: offset + e.span.end,
            },
        );
        let mut result = self.err(ErrorKind::Lexical, format!("{:?}", e.kind), at);
        if let Some(s) = e.opening {
            result.notes.push(self.loc(
                source.clone(),
                Span {
                    start: offset + s.start,
                    end: offset + s.end,
                },
            ));
        }
        result
    }
    fn slice(&self, m: &Material, span: Span) -> Material {
        let mut at = 0;
        let mut out = vec![];
        for p in &m.0 {
            let end = at + p.text.len();
            if span.start < end && at < span.end {
                let a = span.start.saturating_sub(at);
                let b = (span.end - at).min(p.text.len());
                let mut part = p.clone();
                part.text = p.text[a..b].into();
                if p.exact {
                    part.site = self.loc(
                        p.site.source.clone(),
                        Span {
                            start: p.site.span.start + a,
                            end: p.site.span.start + b,
                        },
                    );
                }
                out.push(part);
            }
            at = end;
        }
        Material(out)
    }
    fn origin(&self, m: &Material, span: Span) -> Piece {
        let mut at = 0;
        for p in &m.0 {
            let end = at + p.text.len();
            if span.start < end {
                let mut q = p.clone();
                if p.exact {
                    q.site = self.loc(
                        p.site.source.clone(),
                        Span {
                            start: p.site.span.start + span.start - at,
                            end: p.site.span.start + (span.end - at).min(p.text.len()),
                        },
                    );
                }
                return q;
            }
            at = end;
        }
        m.0.last().expect("nonempty material").clone()
    }
    fn mapped_error(
        &self,
        m: &Material,
        kind: ErrorKind,
        message: impl Into<String>,
        span: Span,
    ) -> Error {
        let p = self.origin(m, span);
        Error {
            kind,
            message: message.into(),
            location: p.site,
            notes: p.definitions,
            include_stack: p.includes,
        }
    }
    fn append(&mut self, m: Material) -> Result<(), Error> {
        for p in m.0 {
            if p.text.len() > self.output_limit - self.output.text.len() {
                return Err(self.err(
                    ErrorKind::Limit,
                    "generated output exceeds configured byte limit (64 MiB by default)",
                    p.site,
                ));
            }
            if p.text.is_empty() {
                continue;
            }
            let start = self.output.text.len();
            self.output.text.push_str(&p.text);
            self.output.mappings.push(Mapping {
                generated: Span {
                    start,
                    end: self.output.text.len(),
                },
                original: p.site,
                exact: p.exact,
                definitions: p.definitions,
                include_stack: p.includes,
            });
        }
        Ok(())
    }
    fn copied(&self, source: &SourceName, span: Span) -> Material {
        Material(vec![Piece {
            text: self.text(source)[span.start..span.end].into(),
            site: self.loc(source.clone(), span),
            exact: true,
            definitions: vec![],
            chain: vec![],
            includes: self.includes.clone(),
        }])
    }
    fn blank(&self, mut m: Material) -> Material {
        for p in &mut m.0 {
            p.text = String::from_utf8(
                p.text
                    .bytes()
                    .map(|b| if b == b'\n' || b == b'\r' { b } else { b' ' })
                    .collect(),
            )
            .unwrap();
            p.exact = false;
        }
        m
    }
    fn expand(&self, mut m: Material, condition: bool) -> Result<Material, Error> {
        loop {
            let text = m.text();
            let l = lex::scan_fragment(&text, self.options.edition).map_err(|e| {
                self.mapped_error(
                    &m,
                    ErrorKind::Lexical,
                    format!("invalid expanded token: {:?}", e.kind),
                    e.span,
                )
            })?;
            let mut shields = Vec::new();
            if condition {
                let ts: Vec<_> = l.tokens.iter().filter(|t| !t.kind.trivia()).collect();
                for (i, t) in ts.iter().enumerate() {
                    if l.text(t.span) == "defined" {
                        shields.push(t.span);
                        let n = i
                            + 1
                            + usize::from(ts.get(i + 1).is_some_and(|t| l.text(t.span) == "("));
                        if let Some(t) = ts.get(n) {
                            shields.push(t.span);
                        }
                    }
                }
            }
            let found = l.tokens.iter().find(|t| {
                l.substitutable(t)
                    && !shields.contains(&t.span)
                    && self.macros.contains_key(l.text(t.span))
            });
            let Some(token) = found else {
                return Ok(m);
            };
            let name = l.text(token.span);
            let definition = &self.macros[name];
            let original = self.origin(&m, token.span);
            if original.chain.iter().any(|n| n == name) {
                let mut chain = original.chain.clone();
                chain.push(name.into());
                let mut e = self.mapped_error(
                    &m,
                    ErrorKind::Recursion,
                    format!("macro expansion cycle: {}", chain.join(" -> ")),
                    token.span,
                );
                e.notes.push(definition.site.clone());
                return Err(e);
            }
            if original.chain.len() >= MAX_EXPANSION_DEPTH {
                return Err(self.mapped_error(
                    &m,
                    ErrorKind::Limit,
                    "macro expansion exceeds 128 levels",
                    token.span,
                ));
            }
            let value = format!(" {} ", definition.text);
            if value.len()
                > self.output_limit.saturating_sub(
                    self.output.text.len() + text.len() - token.span.end + token.span.start,
                )
            {
                return Err(self.mapped_error(
                    &m,
                    ErrorKind::Limit,
                    "generated output exceeds 64 MiB",
                    token.span,
                ));
            }
            let mut replacement = original;
            replacement.text = value;
            replacement.exact = false;
            replacement.chain.push(name.into());
            replacement.definitions.push(definition.site.clone());
            let mut next = self.slice(
                &m,
                Span {
                    start: 0,
                    end: token.span.start,
                },
            );
            next.0.push(replacement);
            next.0.extend(
                self.slice(
                    &m,
                    Span {
                        start: token.span.end,
                        end: text.len(),
                    },
                )
                .0,
            );
            m = next;
            // Rescan the resulting token stream before expanding the next name:
            // a replacement can create a macro invocation with surrounding text.
            // Inserted separators prevent implicit token pasting/comments.
        }
    }
    fn define(
        &mut self,
        name: &str,
        value: &str,
        site: Location,
        value_site: Location,
    ) -> Result<(), Error> {
        if !super::name(name) {
            return Err(self.err(ErrorKind::Definition, "expected an ASCII macro name", site));
        }
        if value.contains(['\n', '\r']) {
            return Err(self.err(
                ErrorKind::Definition,
                "multiline definitions are unsupported",
                value_site,
            ));
        }
        let lexed = lex::scan_fragment(value, self.options.edition)
            .map_err(|e| self.lexical(&value_site.source, value_site.span.start, e))?;
        let mut normalized = value.as_bytes().to_vec();
        for (i, t) in lexed.tokens.iter().enumerate() {
            if matches!(t.kind, Kind::LineComment | Kind::BlockComment) {
                normalized[t.span.start..t.span.end].fill(b' ');
            }
            if t.kind == Kind::Punct('#') && !lexed.protected(t.span) {
                let next = lexed.tokens.get(i + 1);
                if next.is_some_and(|n| {
                    n.span.start == t.span.end
                        && (n.kind == Kind::Punct('#') || matches!(n.kind, Kind::Identifier { .. }))
                }) {
                    return Err(self.err(
                        ErrorKind::Definition,
                        "token pasting and stringification are unsupported",
                        site,
                    ));
                }
            }
        }
        let text = String::from_utf8(normalized).unwrap().trim().to_string();
        if let Some(old) = self.macros.get(name) {
            if old.text != text {
                let mut e = self.err(
                    ErrorKind::Definition,
                    format!("conflicting definition of {name}; use #undef first"),
                    site,
                );
                e.notes.push(old.site.clone());
                return Err(e);
            }
        } else {
            self.macros.insert(name.into(), Definition { text, site });
        }
        Ok(())
    }
    fn tokens<'b>(
        &self,
        source: &SourceName,
        body: Span,
        text: &'b str,
    ) -> Result<lex::Lexed<'b>, Error> {
        lex::scan_fragment(text, self.options.edition)
            .map_err(|e| self.lexical(source, body.start, e))
    }
    fn single_name(&self, source: &SourceName, body: Span) -> Result<String, Error> {
        let text = &self.text(source)[body.start..body.end];
        let l = self.tokens(source, body, text)?;
        let ts: Vec<_> = l.tokens.iter().filter(|t| !t.kind.trivia()).collect();
        if ts.len() != 1
            || ts[0].kind != (Kind::Identifier { raw: false })
            || !super::name(l.text(ts[0].span))
        {
            return Err(self.err(
                ErrorKind::Directive,
                "expected exactly one ASCII macro name",
                self.loc(source.clone(), body),
            ));
        }
        Ok(l.text(ts[0].span).into())
    }
    fn empty(&self, source: &SourceName, body: Span) -> Result<(), Error> {
        let text = &self.text(source)[body.start..body.end];
        let l = self.tokens(source, body, text)?;
        if l.tokens.iter().any(|t| !t.kind.trivia()) {
            return Err(self.err(
                ErrorKind::Conditional,
                "unexpected trailing tokens",
                self.loc(source.clone(), body),
            ));
        }
        Ok(())
    }
    fn condition(&self, source: &SourceName, body: Span) -> Result<bool, Error> {
        let m = self.expand(self.copied(source, body), true)?;
        let text = m.text();
        let n = expression::evaluate(&text, self.options.edition, &|n| {
            self.macros.contains_key(n)
        })
        .map_err(|e| self.mapped_error(&m, ErrorKind::Expression, e.message, e.span))?;
        Ok(n != 0)
    }
    fn include(&self, source: &SourceName, body: Span, site: Location) -> Result<PathBuf, Error> {
        let text = &self.text(source)[body.start..body.end];
        let l = self.tokens(source, body, text)?;
        let ts: Vec<_> = l.tokens.iter().filter(|t| !t.kind.trivia()).collect();
        if ts.len() != 1 || ts[0].kind != Kind::Literal(lex::Literal::String) {
            return Err(self.err(
                ErrorKind::Include,
                "expected a quoted relative include path",
                site,
            ));
        }
        let token = l.text(ts[0].span);
        let path = token
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
            .filter(|s| !s.is_empty() && !s.contains(['\\', '\0', '\n', '\r']))
            .ok_or_else(|| {
                self.err(
                    ErrorKind::Include,
                    "include paths must be unescaped quoted relative paths",
                    site.clone(),
                )
            })?;
        if Path::new(path).is_absolute() {
            return Err(self.err(
                ErrorKind::Include,
                "absolute include paths are unsupported",
                site,
            ));
        }
        let SourceName::File(current) = source else {
            unreachable!()
        };
        let mut search = vec![current.parent().unwrap_or(Path::new("")).to_path_buf()];
        search.extend(self.dirs.clone());
        let mut chosen = None;
        for dir in search {
            let candidate = normalize(&dir.join(path))
                .map_err(|m| self.err(ErrorKind::Include, m, site.clone()))?;
            if self.sources.files.contains_key(&candidate) {
                chosen = Some(candidate);
                break;
            }
        }
        let path = chosen.ok_or_else(|| {
            self.err(
                ErrorKind::Include,
                format!("include not declared: {path}"),
                site.clone(),
            )
        })?;
        if self.files.contains(&path) {
            return Err(self.err(
                ErrorKind::Include,
                format!("include cycle at {}", path.display()),
                site,
            ));
        }
        if self.files.len() >= MAX_INCLUDE_DEPTH {
            return Err(self.err(ErrorKind::Limit, "include depth exceeds 128", site));
        }
        Ok(path)
    }
    fn directive(
        &mut self,
        source: &SourceName,
        t: Token,
        stack: &mut Vec<Conditional>,
    ) -> Result<Option<(PathBuf, Location)>, Error> {
        let Kind::DirectiveLine { name } = t.kind else {
            unreachable!()
        };
        let command = self.text(source)[name.start..name.end].to_string();
        let body = Span {
            start: name.end,
            end: t.span.end,
        };
        let site = self.loc(source.clone(), name);
        let active = stack.last().is_none_or(|s| s.active);
        match command.as_str() {
            "if" | "ifdef" | "ifndef" => {
                if stack.len() >= MAX_CONDITIONAL_DEPTH {
                    return Err(self.err(ErrorKind::Limit, "conditional depth exceeds 256", site));
                }
                let yes = if command == "if" {
                    if self.text(source)[body.start..body.end].trim().is_empty() {
                        return Err(self.err(
                            ErrorKind::Conditional,
                            "#if requires an expression",
                            site,
                        ));
                    }
                    active && self.condition(source, body)?
                } else {
                    let n = self.single_name(source, body)?;
                    active && (self.macros.contains_key(&n) != (command == "ifndef"))
                };
                stack.push(Conditional {
                    parent: active,
                    selected: yes,
                    active: yes,
                    seen_else: false,
                    opening: site,
                });
            }
            "elif" => {
                let s = stack.last().ok_or_else(|| {
                    self.err(ErrorKind::Conditional, "#elif without #if", site.clone())
                })?;
                if s.seen_else {
                    return Err(self.err(ErrorKind::Conditional, "#elif after #else", site));
                }
                if self.text(source)[body.start..body.end].trim().is_empty() {
                    return Err(self.err(
                        ErrorKind::Conditional,
                        "#elif requires an expression",
                        site,
                    ));
                }
                let yes = s.parent && !s.selected && self.condition(source, body)?;
                let s = stack.last_mut().unwrap();
                s.active = yes;
                s.selected |= yes;
            }
            "else" => {
                self.empty(source, body)?;
                let s = stack.last_mut().ok_or_else(|| {
                    self.err(ErrorKind::Conditional, "#else without #if", site.clone())
                })?;
                if s.seen_else {
                    return Err(self.err(ErrorKind::Conditional, "duplicate #else", site));
                }
                s.active = s.parent && !s.selected;
                s.selected = true;
                s.seen_else = true;
            }
            "endif" => {
                self.empty(source, body)?;
                if stack.pop().is_none() {
                    return Err(self.err(ErrorKind::Conditional, "#endif without #if", site));
                }
            }
            _ if !active => (),
            "define" => {
                let text = &self.text(source)[body.start..body.end];
                let l = self.tokens(source, body, text)?;
                let first = l.tokens.iter().find(|t| !t.kind.trivia()).ok_or_else(|| {
                    self.err(ErrorKind::Definition, "missing macro name", site.clone())
                })?;
                if first.kind != (Kind::Identifier { raw: false })
                    || !super::name(l.text(first.span))
                {
                    return Err(self.err(
                        ErrorKind::Definition,
                        "expected an ASCII macro name",
                        site,
                    ));
                }
                let name = l.text(first.span).to_string();
                let end = body.start + first.span.end;
                if self.text(source)[end..].starts_with('(') {
                    return Err(self.err(
                        ErrorKind::Definition,
                        "function-like macros are unsupported",
                        site,
                    ));
                }
                let value = self.text(source)[end..body.end].to_string();
                self.define(
                    &name,
                    &value,
                    self.loc(
                        source.clone(),
                        Span {
                            start: body.start + first.span.start,
                            end,
                        },
                    ),
                    self.loc(
                        source.clone(),
                        Span {
                            start: end,
                            end: body.end,
                        },
                    ),
                )?;
            }
            "undef" => {
                let n = self.single_name(source, body)?;
                self.macros.remove(&n);
            }
            "include" => return Ok(Some((self.include(source, body, site.clone())?, site))),
            "error" => {
                return Err(self.err(
                    ErrorKind::ExplicitError,
                    self.text(source)[body.start..body.end].trim().to_string(),
                    site,
                ));
            }
            _ => {
                return Err(self.err(
                    ErrorKind::Directive,
                    format!("unknown directive #{command}"),
                    site,
                ));
            }
        }
        Ok(None)
    }
    fn frame(&self, path: PathBuf) -> Result<FileFrame<'a>, Error> {
        let source = SourceName::File(path.clone());
        let sources = self.sources;
        let text = &sources.files[&path];
        let tokens = lex::scan(text, self.options.edition)
            .map_err(|e| self.lexical(&source, 0, e))?
            .tokens;
        Ok(FileFrame {
            path,
            text,
            tokens,
            next: 0,
            begin: 0,
            conditions: vec![],
        })
    }
    fn file(&mut self, path: &Path) -> Result<(), Error> {
        self.files.push(path.into());
        self.output.dependencies.insert(path.into());
        let mut frames = vec![self.frame(path.into())?];
        'files: while let Some(mut f) = frames.pop() {
            let source = SourceName::File(f.path.clone());
            while f.next < f.tokens.len() {
                let t = f.tokens[f.next];
                f.next += 1;
                if !matches!(
                    t.kind,
                    Kind::DirectiveLine { .. } | Kind::Bom | Kind::Shebang
                ) {
                    continue;
                }
                if f.begin < t.span.start {
                    let m = self.copied(
                        &source,
                        Span {
                            start: f.begin,
                            end: t.span.start,
                        },
                    );
                    let m = if f.conditions.last().is_none_or(|s| s.active) {
                        self.expand(m, false)?
                    } else {
                        self.blank(m)
                    };
                    self.append(m)?;
                }
                f.begin = t.span.end;
                if matches!(t.kind, Kind::DirectiveLine { .. }) {
                    self.append(self.blank(self.copied(&source, t.span)))?;
                    if let Some((path, site)) = self.directive(&source, t, &mut f.conditions)? {
                        frames.push(f);
                        self.includes.push(site);
                        self.files.push(path.clone());
                        self.output.dependencies.insert(path.clone());
                        frames.push(self.frame(path)?);
                        continue 'files;
                    }
                } else {
                    let m = self.copied(&source, t.span);
                    let m = if self.files.len() == 1 {
                        m
                    } else {
                        self.blank(m)
                    };
                    self.append(m)?;
                }
            }
            if f.begin < f.text.len() {
                let m = self.copied(
                    &source,
                    Span {
                        start: f.begin,
                        end: f.text.len(),
                    },
                );
                let m = if f.conditions.last().is_none_or(|s| s.active) {
                    self.expand(m, false)?
                } else {
                    self.blank(m)
                };
                self.append(m)?;
            }
            if let Some(s) = f.conditions.last() {
                return Err(self.err(
                    ErrorKind::Conditional,
                    "unterminated conditional in this file",
                    s.opening.clone(),
                ));
            }
            self.files.pop();
            if !self.includes.is_empty() {
                self.includes.pop();
            }
        }
        Ok(())
    }
}

pub(super) fn preprocess(
    sources: &Sources,
    entry: &Path,
    options: &Options,
) -> Result<Output, Error> {
    run(sources, entry, options, MAX_OUTPUT_BYTES)
}

#[cfg(test)]
pub(super) fn with_limit(
    sources: &Sources,
    entry: &Path,
    options: &Options,
    limit: usize,
) -> Result<Output, Error> {
    run(sources, entry, options, limit)
}

fn run(
    sources: &Sources,
    entry: &Path,
    options: &Options,
    output_limit: usize,
) -> Result<Output, Error> {
    let fallback = Location {
        source: SourceName::File(entry.into()),
        span: Span { start: 0, end: 0 },
        line: 1,
        column: 1,
    };
    let invalid = |message| Error {
        kind: ErrorKind::InvalidInput,
        message,
        location: fallback.clone(),
        notes: vec![],
        include_stack: vec![],
    };
    let entry = normalize(entry).map_err(invalid)?;
    if !sources.files.contains_key(&entry) {
        return Err(invalid(format!(
            "entry source not declared: {}",
            entry.display()
        )));
    }
    let mut engine = Engine {
        sources,
        options,
        output_limit,
        macros: BTreeMap::new(),
        predefined: BTreeMap::new(),
        includes: vec![],
        files: vec![],
        dirs: vec![],
        output: Output {
            text: String::new(),
            mappings: vec![],
            dependencies: BTreeSet::new(),
        },
    };
    for dir in &options.include_dirs {
        let p = normalize_directory(dir).map_err(invalid)?;
        engine.dirs.push(p);
    }
    for d in &options.definitions {
        match d {
            InitialDefinition::Define(n, v) => {
                engine.predefined.insert(n.clone(), v.clone());
                let site = engine.loc(
                    SourceName::Predefined(n.clone()),
                    Span {
                        start: 0,
                        end: v.len(),
                    },
                );
                engine.define(n, v, site.clone(), site)?;
            }
            InitialDefinition::Undef(n) => {
                if !name(n) {
                    return Err(invalid("invalid predefined macro name".into()));
                }
                engine.macros.remove(n);
            }
        }
    }
    engine.file(&entry)?;
    Ok(engine.output)
}
