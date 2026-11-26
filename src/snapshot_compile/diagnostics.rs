// SPDX-License-Identifier: GPL-3.0-or-later
use super::json::{self, Value};
use crate::{Result, lex, preprocess_cli::snapshot::Verified};
use std::{collections::BTreeSet, fs, io::Write, path::Path};

pub(super) enum Presentation {
    Mapped,
    Json,
    ExplicitHuman,
}

pub(super) fn presentation(args: &[String]) -> Presentation {
    let explicit = args
        .iter()
        .enumerate()
        .filter_map(|(i, a)| {
            if a == "--error-format" {
                args.get(i + 1).map(String::as_str)
            } else {
                a.strip_prefix("--error-format=")
            }
        })
        .last();
    match explicit {
        Some("json") => Presentation::Json,
        Some(_) => Presentation::ExplicitHuman,
        None => Presentation::Mapped,
    }
}

pub(super) struct SourceMap(Value);

impl SourceMap {
    pub fn read(mirror: &Path, tree: &Verified) -> Result<Self> {
        let text = fs::read_to_string(mirror.join(&tree.map))?;
        let map = json::parse(&text)
            .filter(|m| m.number("version") == Some(1))
            .ok_or("invalid snapshot source map")?;
        let files = map
            .array("sources")
            .ok_or("snapshot source map lacks sources")?;
        let mut seen = BTreeSet::new();
        for file in files {
            let path = Path::new(
                file.text("generated")
                    .ok_or("source map lacks a generated path")?,
            );
            if !tree.payload.contains(path)
                || path == tree.map
                || !seen.insert(path)
                || file.number("version") != Some(1)
                || file.array("mappings").is_none()
            {
                return Err("invalid source map file entry".into());
            }
        }
        if !seen.contains(tree.entry.as_path()) {
            return Err("source map lacks the crate entry".into());
        }
        Ok(Self(map))
    }

    fn notes(&self, diagnostic: &Value, cwd: &Path, mirror: &Path, original: &Path) -> Vec<String> {
        let mut lines = Vec::new();
        for span in diagnostic.array("spans").unwrap_or_default() {
            if !span.yes("is_primary") || !matches!(span.get("expansion"), Some(Value::Null) | None)
            {
                continue;
            }
            let Some((location, mapping, line, column)) = self.map_span(span, cwd, mirror) else {
                continue;
            };
            let Some(path) = location.get("source").and_then(|s| s.text("path")) else {
                continue;
            };
            if !safe_path(path) {
                continue;
            }
            let primary = format!(
                "rpp: original source {}:{line}:{column}",
                original.join(path).display()
            );
            if lines.contains(&primary) {
                continue;
            }
            lines.push(primary);
            for (key, label) in [
                ("definitions", "macro defined at"),
                ("include_stack", "preprocessing include at"),
            ] {
                for note in mapping.array(key).unwrap_or_default() {
                    if let Some(note) = note_text(note, original) {
                        lines.push(format!("rpp: {label} {note}"));
                    }
                }
            }
        }
        lines
    }

    fn map_span(
        &self,
        span: &Value,
        cwd: &Path,
        mirror: &Path,
    ) -> Option<(&Value, &Value, usize, usize)> {
        let file = crate::preprocess_cli::absolute(cwd, Path::new(span.text("file_name")?));
        let relative = file.strip_prefix(mirror).ok()?.to_str()?;
        let entry = self
            .0
            .array("sources")?
            .iter()
            .find(|s| s.text("generated") == Some(relative))?;
        let (start, end) = (span.number("byte_start")?, span.number("byte_end")?);
        if end < start {
            return None;
        }
        let text = fs::read_to_string(file).ok()?;
        text.get(start..end)?;
        let (mapping, begin) = entry.array("mappings")?.iter().find_map(|m| {
            let range = m.get("generated")?;
            let from = range.number("start")?;
            (from <= start && start < range.number("end")? && end <= range.number("end")?)
                .then_some((m, from))
        })?;
        let location = mapping.get("original")?;
        let (mut line, mut column) = (location.number("line")?, location.number("column")?);
        if mapping.yes("exact") {
            let raw = location.get("bytes")?;
            let prefix = text.get(begin..start)?;
            // Verify rustc's offset/column convention for this span before
            // transporting the position into an original copied range.
            let generated = lex::source_position(&text, start)?;
            if generated.line != span.number("line_start")?
                || generated.column != span.number("column_start")?
            {
                return None;
            }
            let newlines = prefix.bytes().filter(|&b| b == b'\n').count();
            if let Some((_, tail)) = prefix.rsplit_once('\n') {
                line = line.checked_add(newlines)?;
                column = 1 + tail.chars().count();
            } else {
                column = column.checked_add(prefix.chars().count())?;
                if raw.number("start")? == 0 && prefix.starts_with('\u{feff}') {
                    column = column.checked_sub(1)?;
                }
            }
            if prefix.ends_with('\r') && text.get(start..)?.starts_with('\n') {
                column = column.checked_sub(1)?;
            }
        } else if mapping.array("definitions")?.is_empty() {
            return None;
        }
        Some((location, mapping, line, column))
    }
}

fn safe_path(path: &str) -> bool {
    crate::preprocess_cli::snapshot::validate_relative(Path::new(path)).is_ok()
}

fn note_text(note: &Value, root: &Path) -> Option<String> {
    let source = note.get("source")?;
    let name = if let Some(path) = source.text("path").filter(|p| safe_path(p)) {
        root.join(path).display().to_string()
    } else {
        format!("<predefined {}>", source.text("predefined")?)
    };
    Some(format!(
        "{name}:{}:{}",
        note.number("line")?,
        note.number("column")?
    ))
}

pub(super) fn report(
    bytes: &[u8],
    mode: Presentation,
    native: bool,
    sources: Option<(&SourceMap, &Path, &Path, &Path)>,
) -> Result<Option<Vec<String>>> {
    let mut runtime = None;
    let mut stderr = std::io::stderr().lock();
    for line in bytes.split_inclusive(|b| *b == b'\n') {
        let text = std::str::from_utf8(line).ok();
        let parsed = text.and_then(json::parse);
        let libs = if native {
            if matches!(mode, Presentation::ExplicitHuman) {
                text.and_then(|s| s.trim_start().strip_prefix("note: native-static-libs:"))
            } else {
                parsed
                    .as_ref()
                    .filter(|v| {
                        v.text("$message_type") == Some("diagnostic")
                            && v.text("level") == Some("note")
                            && v.array("spans").is_some_and(|a| a.is_empty())
                    })
                    .and_then(|v| v.text("message")?.strip_prefix("native-static-libs:"))
            }
        } else {
            None
        };
        if let Some(libs) = libs {
            runtime = Some(libs.split_whitespace().map(String::from).collect());
        }
        if matches!(mode, Presentation::Json) {
            stderr.write_all(line)?;
        } else if libs.is_none() {
            if matches!(mode, Presentation::Mapped) {
                if let Some(v) = parsed
                    .as_ref()
                    .filter(|v| v.text("$message_type") == Some("diagnostic"))
                {
                    if let Some(rendered) = v.text("rendered") {
                        stderr.write_all(rendered.as_bytes())?;
                        if !rendered.ends_with('\n') {
                            writeln!(stderr)?;
                        }
                    } else {
                        stderr.write_all(line)?;
                    }
                    if let Some((map, cwd, mirror, original)) = sources {
                        for note in map.notes(v, cwd, mirror, original) {
                            writeln!(stderr, "{note}")?;
                        }
                    }
                    continue;
                }
            }
            stderr.write_all(line)?;
        }
    }
    Ok(runtime)
}
