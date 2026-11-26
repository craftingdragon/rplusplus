// SPDX-License-Identifier: GPL-3.0-or-later
use crate::{Result, lex::Span, preprocess as pp};
use std::{
    fmt::Write as _,
    fs::{self, OpenOptions},
    io::{self, Write as _},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

fn string(out: &mut String, value: &str) {
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c <= '\u{1f}' => write!(out, "\\u{:04x}", c as u32).unwrap(),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn span(out: &mut String, span: Span) {
    write!(out, "{{\"start\":{},\"end\":{}}}", span.start, span.end).unwrap();
}

fn location(out: &mut String, location: &pp::Location) {
    out.push_str("{\"source\":{");
    match &location.source {
        pp::SourceName::File(path) => {
            out.push_str("\"path\":");
            string(out, &path.to_string_lossy());
        }
        pp::SourceName::Predefined(name) => {
            out.push_str("\"predefined\":");
            string(out, name);
        }
    }
    out.push_str("},\"bytes\":");
    span(out, location.span);
    write!(
        out,
        ",\"line\":{},\"column\":{}}}",
        location.line, location.column
    )
    .unwrap();
}

fn locations(out: &mut String, values: &[pp::Location]) {
    out.push('[');
    for (i, loc) in values.iter().enumerate() {
        if i != 0 {
            out.push(',');
        }
        location(out, loc);
    }
    out.push(']');
}

pub(super) fn diagnostic(error: &(dyn std::error::Error + 'static)) -> String {
    let mut out = String::from("{\"kind\":");
    if let Some(error) = error.downcast_ref::<pp::Error>() {
        string(&mut out, &format!("{:?}", error.kind));
        out.push_str(",\"message\":");
        string(&mut out, &error.message);
        out.push_str(",\"location\":");
        location(&mut out, &error.location);
        out.push_str(",\"notes\":");
        locations(&mut out, &error.notes);
        out.push_str(",\"include_stack\":");
        locations(&mut out, &error.include_stack);
    } else {
        out.push_str("\"InputOutput\",\"message\":");
        string(&mut out, &error.to_string());
    }
    out.push('}');
    out
}

pub(super) fn source_map(generated: &Path, output: &pp::Output) -> Result<String> {
    let mut out = String::from("{\"version\":1,\"generated\":");
    string(&mut out, utf8_path(generated)?);
    out.push_str(",\"mappings\":[");
    for (i, mapping) in output.mappings.iter().enumerate() {
        if i != 0 {
            out.push(',');
        }
        out.push_str("{\"generated\":");
        span(&mut out, mapping.generated);
        out.push_str(",\"original\":");
        location(&mut out, &mapping.original);
        write!(out, ",\"exact\":{},\"definitions\":", mapping.exact).unwrap();
        locations(&mut out, &mapping.definitions);
        out.push_str(",\"include_stack\":");
        locations(&mut out, &mapping.include_stack);
        out.push('}');
    }
    out.push_str("]}\n");
    Ok(out)
}

fn utf8_path(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| format!("path is not UTF-8: {}", path.display()).into())
}

fn make_path(out: &mut String, cwd: &Path, path: &Path, target: bool) -> Result<()> {
    let value = utf8_path(path.strip_prefix(cwd).unwrap_or(path))?;
    // Some Make grammar characters cannot be passed literally to both the
    // target and prerequisite parsers. Fail before touching any destination.
    if value.is_empty()
        || value.contains([
            '\n', '\r', '\t', '\\', ':', ';', '|', '%', '*', '?', '[', ']', '=',
        ])
        || (target && value.ends_with('&'))
    {
        return Err(format!(
            "path cannot be represented in a Make depfile: {}",
            path.display()
        )
        .into());
    }
    if value.starts_with('~') {
        out.push_str("./");
    }
    for c in value.chars() {
        match c {
            ' ' | '#' => {
                out.push('\\');
                out.push(c);
            }
            '$' => out.push_str("$$"),
            _ => out.push(c),
        }
    }
    Ok(())
}

pub(super) fn depfile(
    cwd: &Path,
    root: &Path,
    output: &Path,
    result: &pp::Output,
) -> Result<String> {
    let mut out = String::new();
    make_path(&mut out, cwd, output, true)?;
    out.push(':');
    for path in &result.dependencies {
        out.push(' ');
        make_path(&mut out, cwd, &root.join(path), false)?;
    }
    out.push('\n');
    Ok(out)
}

pub(super) fn aliases(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(left), Ok(right)) = (fs::metadata(left), fs::metadata(right)) {
            return (left.dev(), left.ino()) == (right.dev(), right.ino());
        }
    }
    false
}

struct Stage {
    path: PathBuf,
    destination: PathBuf,
}

impl Drop for Stage {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn stage(destination: PathBuf, bytes: &[u8], reserved: &[PathBuf]) -> Result<Stage> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    loop {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let path =
            destination.with_file_name(format!(".rpp-preprocess-{}-{id}.tmp", std::process::id()));
        if reserved.contains(&path) {
            continue;
        }
        let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("stage {}: {e}", destination.display()).into()),
        };
        let stage = Stage { path, destination };
        file.write_all(bytes)
            .map_err(|e| format!("stage {}: {e}", stage.destination.display()))?;
        return Ok(stage);
    }
}

fn commit(staged: Vec<Stage>) -> Result<()> {
    for stage in staged {
        fs::rename(&stage.path, &stage.destination)
            .map_err(|e| format!("publish {}: {e}", stage.destination.display()))?;
    }
    Ok(())
}

pub(super) fn publish(files: Vec<(PathBuf, Vec<u8>)>, inputs: &[PathBuf]) -> Result<()> {
    let mut destinations: Vec<PathBuf> = Vec::new();
    let mut changed = Vec::new();
    for (path, bytes) in files {
        let name = path.file_name().ok_or("expected an output file path")?;
        let parent = super::canonical(path.parent().ok_or("expected an output parent")?)?;
        if !parent.is_dir() {
            return Err(format!("{} is not an output directory", parent.display()).into());
        }
        let destination = parent.join(name);
        let existing = match fs::symlink_metadata(&destination) {
            Ok(meta) if meta.is_file() => true,
            Err(e) if e.kind() == io::ErrorKind::NotFound => false,
            Ok(_) => return Err(format!("{} is not a regular output file", path.display()).into()),
            Err(e) => return Err(format!("{}: {e}", path.display()).into()),
        };
        if inputs
            .iter()
            .chain(&destinations)
            .any(|other| aliases(other, &destination))
        {
            return Err(format!(
                "output aliases an input or another output: {}",
                path.display()
            )
            .into());
        }
        if !existing || fs::read(&destination)? != bytes {
            changed.push((destination.clone(), bytes));
        }
        destinations.push(destination);
    }
    let staged = changed
        .into_iter()
        .map(|(path, bytes)| stage(path, &bytes, &destinations))
        .collect::<Result<Vec<_>>>()?;
    commit(staged)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publication_failure_drops_pending_stages_and_preserves_primary() {
        let root = env_temp_dir();
        let sidecar = root.join("sidecar");
        let primary = root.join("primary");
        fs::write(&primary, "previous").unwrap();
        let staging_failure = [sidecar.clone(), root.join("missing/output")]
            .into_iter()
            .map(|path| stage(path, b"staged", &[]))
            .collect::<Result<Vec<_>>>();
        assert!(staging_failure.is_err());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        assert_eq!(fs::read(&primary).unwrap(), b"previous");

        let side = stage(sidecar.clone(), b"new sidecar", &[]).unwrap();
        let output = stage(primary.clone(), b"new primary", &[]).unwrap();
        let pending = output.path.clone();
        fs::create_dir(&sidecar).unwrap();
        assert!(commit(vec![side, output]).is_err());
        assert_eq!(fs::read(&primary).unwrap(), b"previous");
        assert!(!pending.exists());
        fs::remove_dir_all(root).unwrap();
    }

    fn env_temp_dir() -> PathBuf {
        let root = std::env::temp_dir().join(format!("rpp-m3-publish-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        root
    }
}
