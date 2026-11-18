// SPDX-License-Identifier: GPL-3.0-or-later
//! Pure optional preprocessing. No compiler invocation, filesystem I/O or linking.
mod engine;
mod expression;

use crate::lex::{self, Edition, Span};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path, PathBuf},
};

pub const MAX_INCLUDE_DEPTH: usize = 128;
pub const MAX_CONDITIONAL_DEPTH: usize = 256;
pub const MAX_EXPANSION_DEPTH: usize = 128;
pub const MAX_OUTPUT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceName {
    File(PathBuf),
    Predefined(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
    pub source: SourceName,
    pub span: Span,
    pub line: usize,
    pub column: usize,
}

impl std::fmt::Display for Location {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.source {
            SourceName::File(p) => write!(f, "{}", p.display())?,
            SourceName::Predefined(n) => write!(f, "<predefined {n}>")?,
        }
        write!(f, ":{}:{}", self.line, self.column)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    InvalidInput,
    Lexical,
    Directive,
    Definition,
    Expression,
    Conditional,
    Include,
    Recursion,
    Limit,
    ExplicitError,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    pub message: String,
    pub location: Location,
    pub notes: Vec<Location>,
    pub include_stack: Vec<Location>,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {:?}: {}", self.location, self.kind, self.message)?;
        for note in &self.notes {
            write!(f, "\n  note: {note}")?;
        }
        for site in self.include_stack.iter().rev() {
            write!(f, "\n  included from {site}")?;
        }
        Ok(())
    }
}
impl std::error::Error for Error {}

#[derive(Clone, Debug)]
pub enum InitialDefinition {
    Define(String, String),
    Undef(String),
}

#[derive(Clone, Debug)]
pub struct Options {
    pub edition: Edition,
    pub include_dirs: Vec<PathBuf>,
    pub definitions: Vec<InitialDefinition>,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            edition: Edition::Rust2024,
            include_dirs: vec![],
            definitions: vec![],
        }
    }
}

/// Explicit logical inputs beneath one root. Physical path/symlink validation is
/// the future filesystem adapter's responsibility; this object never opens files.
#[derive(Default, Debug)]
pub struct Sources {
    files: BTreeMap<PathBuf, String>,
}
impl Sources {
    pub fn insert(&mut self, path: impl AsRef<Path>, text: String) -> Result<(), String> {
        let path = normalize(path.as_ref())?;
        if self.files.contains_key(&path) {
            return Err(format!("duplicate source {}", path.display()));
        }
        self.files.insert(path, text);
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mapping {
    pub generated: Span,
    pub original: Location,
    /// True only for copied bytes, permitting exact offset mapping within span.
    pub exact: bool,
    pub definitions: Vec<Location>,
    pub include_stack: Vec<Location>,
}

#[derive(Debug)]
pub struct Output {
    pub text: String,
    pub mappings: Vec<Mapping>,
    pub dependencies: BTreeSet<PathBuf>,
}

pub fn preprocess(
    sources: &Sources,
    entry: impl AsRef<Path>,
    options: &Options,
) -> Result<Output, Error> {
    engine::preprocess(sources, entry.as_ref(), options)
}

fn normalize(path: &Path) -> Result<PathBuf, String> {
    let out = normalize_directory(path)?;
    if out.as_os_str().is_empty() {
        return Err("expected a file path".into());
    }
    Ok(out)
}

fn normalize_directory(path: &Path) -> Result<PathBuf, String> {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::Normal(p) => out.push(p),
            Component::CurDir => (),
            Component::ParentDir if out.pop() => (),
            _ => {
                return Err(format!(
                    "path escapes the declared logical source root: {}",
                    path.display()
                ));
            }
        }
    }
    Ok(out)
}

fn name(s: &str) -> bool {
    let mut bytes = s.bytes();
    bytes
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

#[cfg(test)]
mod tests;
