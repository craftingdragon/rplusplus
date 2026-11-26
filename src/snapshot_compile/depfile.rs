// SPDX-License-Identifier: GPL-3.0-or-later
use crate::{
    Result,
    preprocess_cli::{absolute, artifacts},
};
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

fn words(text: &str) -> Result<BTreeSet<String>> {
    let mut result = BTreeSet::new();
    let mut word = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&' ') => {
                chars.next();
                word.push(' ');
            }
            ' ' if !word.is_empty() => {
                result.insert(std::mem::take(&mut word));
            }
            ' ' => (),
            '\\' | '\t' | '\r' | '\0' => {
                return Err("unsupported character in rustc dependency path".into());
            }
            _ => word.push(c),
        }
    }
    if !word.is_empty() {
        result.insert(word);
    }
    Ok(result)
}

pub(super) fn externs(args: &[String], cwd: &Path) -> BTreeSet<PathBuf> {
    args.iter()
        .enumerate()
        .filter_map(|(i, flag)| {
            let argument = if flag == "--extern" {
                args.get(i + 1)?.as_str()
            } else {
                flag.strip_prefix("--extern=")?
            };
            let (_, path) = argument.split_once('=')?;
            Some(absolute(cwd, Path::new(path)))
        })
        .collect()
}

pub(crate) struct Report {
    pub files: BTreeSet<PathBuf>,
    pub runtime: Vec<String>,
    comments: Vec<String>,
}

pub(super) fn audit(
    depfile: &Path,
    emitted: &str,
    cwd: &Path,
    entry: &Path,
    allowed: &BTreeSet<PathBuf>,
) -> Result<Report> {
    let dep = depfile.to_str().ok_or("non-UTF-8 staged depfile path")?;
    let targets = [emitted, dep];
    let mut seen = BTreeSet::new();
    let mut canonical_set = None;
    let mut phony = BTreeSet::new();
    let mut comments = Vec::new();
    for line in fs::read_to_string(depfile)?.lines() {
        if line.is_empty() {
            continue;
        }
        if let Some(comment) = line.strip_prefix("# env-dep:") {
            if comment.is_empty() || comment.contains(['\r', '\0']) {
                return Err("malformed rustc environment dependency".into());
            }
            comments.push(line.to_owned());
            continue;
        }
        let mut matched = false;
        for target in targets {
            let escaped = target.replace(' ', "\\ ");
            if let Some(rest) = line
                .strip_prefix(target)
                .or_else(|| line.strip_prefix(&escaped))
                .and_then(|s| s.strip_prefix(": "))
            {
                if !seen.insert(target) {
                    return Err("duplicate rustc dependency rule".into());
                }
                let files = words(rest)?;
                if files.is_empty()
                    || canonical_set
                        .as_ref()
                        .is_some_and(|previous| previous != &files)
                {
                    return Err("inconsistent rustc dependency rules".into());
                }
                canonical_set = Some(files);
                matched = true;
                break;
            }
        }
        if !matched {
            let words = words(
                line.strip_suffix(':')
                    .ok_or("unrecognized rustc dependency line")?,
            )?;
            if words.len() != 1 {
                return Err("invalid rustc dependency sentinel".into());
            }
            phony.extend(words);
        }
    }
    let Some(paths) = canonical_set.filter(|p| seen.len() == 2 && &phony == p) else {
        return Err("missing or inconsistent rustc dependency information".into());
    };
    let mut files = BTreeSet::new();
    let canonical_allowed: BTreeSet<_> = allowed
        .iter()
        .map(fs::canonicalize)
        .collect::<std::io::Result<_>>()?;
    for p in paths {
        let path = absolute(cwd, Path::new(&p));
        // Canonicalize the actual compiler spelling as well: symlinks before
        // a parent component must resolve as they did for rustc.
        let canonical = fs::canonicalize(cwd.join(&p))
            .map_err(|_| format!("unexpected rustc dependency: {p}"))?;
        if !allowed.contains(&path)
            || !canonical_allowed.contains(&canonical)
            || fs::canonicalize(&path).ok().as_ref() != Some(&canonical)
        {
            return Err(format!(
                "unexpected rustc dependency outside the snapshot or declared externs: {p}"
            )
            .into());
        }
        files.insert(path);
    }
    if !files.contains(entry) {
        return Err("rustc dependency information omitted the generated entry".into());
    }
    Ok(Report {
        files,
        comments,
        runtime: Vec::new(),
    })
}

impl Report {
    pub(crate) fn publish_rule(self, path: &Path, output: &str, cwd: &Path) -> Result<()> {
        let mut text = String::new();
        // Preserve the spelling used by the producing build rule.
        artifacts::make_path(&mut text, Path::new(""), Path::new(output), true)?;
        text.push(':');
        for file in self.files {
            text.push(' ');
            artifacts::make_path(&mut text, cwd, &file, false)?;
        }
        text.push('\n');
        for line in self.comments {
            text.push_str(&line);
            text.push('\n');
        }
        fs::write(path, text)?;
        Ok(())
    }
}
