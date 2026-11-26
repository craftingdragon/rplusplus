// SPDX-License-Identifier: GPL-3.0-or-later
mod artifacts;

use crate::{lex, preprocess as pp};
use std::{
    env, fs,
    path::{Component, Path, PathBuf},
};

const HELP: &str = "Usage: rpp preprocess INPUT -o OUTPUT [options]
  -I, --include-dir DIR        Add a preprocessing include directory
  -D, --define NAME[=VALUE]     Define a macro (default value: 1)
  -U, --undef NAME              Remove a macro definition
      --input FILE             Declare a preprocessing fragment (repeatable)
      --source-root DIR        Bound inputs (default: input's parent)
      --edition 2021|2024      Select Rust edition (default: 2024)
      --depfile FILE           Write Make-style dependencies
      --source-map FILE        Write a versioned JSON source map
      --diagnostic-format human|json
      --                       End option parsing";

#[derive(Default)]
struct Cli {
    entry: Option<PathBuf>,
    output: Option<PathBuf>,
    root: Option<PathBuf>,
    inputs: Vec<PathBuf>,
    depfile: Option<PathBuf>,
    map: Option<PathBuf>,
    options: pp::Options,
    json: bool,
}

fn set<T>(slot: &mut Option<T>, value: T, name: &str) -> Result<(), String> {
    if slot.is_some() {
        return Err(format!("duplicate {name}"));
    }
    *slot = Some(value);
    Ok(())
}

fn parse(args: &[String]) -> Result<Option<Cli>, String> {
    let mut cli = Cli::default();
    let mut edition = None;
    let mut format = None;
    let mut options = true;
    let mut args = args.iter().map(String::as_str);
    while let Some(arg) = args.next() {
        if !options || !arg.starts_with('-') {
            set(&mut cli.entry, arg.into(), "input file")?;
            continue;
        }
        if arg == "--" {
            options = false;
            continue;
        }
        if matches!(arg, "-h" | "--help") {
            return Ok(None);
        }
        let (flag, attached) =
            if arg.len() > 2 && ["-I", "-D", "-U"].iter().any(|p| arg.starts_with(p)) {
                (&arg[..2], Some(&arg[2..]))
            } else {
                (arg, None)
            };
        if matches!(flag, "--source" | "--output-dir") {
            return Err(format!("{flag} requires source-tree mode (M4)"));
        }
        if !matches!(
            flag,
            "-o" | "-I"
                | "--include-dir"
                | "-D"
                | "--define"
                | "-U"
                | "--undef"
                | "--input"
                | "--source-root"
                | "--edition"
                | "--depfile"
                | "--source-map"
                | "--diagnostic-format"
        ) {
            return Err(format!("unknown option {flag}"));
        }
        let value = attached
            .or_else(|| args.next().filter(|v| !v.starts_with('-')))
            .filter(|v| !v.is_empty())
            .ok_or_else(|| format!("missing value for {flag}"))?;
        match flag {
            "-o" => set(&mut cli.output, value.into(), flag)?,
            "--source-root" => set(&mut cli.root, value.into(), flag)?,
            "--depfile" => set(&mut cli.depfile, value.into(), flag)?,
            "--source-map" => set(&mut cli.map, value.into(), flag)?,
            "-I" | "--include-dir" => cli.options.include_dirs.push(value.into()),
            "--input" => cli.inputs.push(value.into()),
            "-D" | "--define" | "-U" | "--undef" => {
                let define = matches!(flag, "-D" | "--define");
                let (name, replacement) = if define {
                    value.split_once('=').unwrap_or((value, "1"))
                } else {
                    (value, "")
                };
                if !pp::name(name) {
                    return Err(format!("invalid macro name {name:?}"));
                }
                cli.options.definitions.push(if define {
                    pp::InitialDefinition::Define(name.into(), replacement.into())
                } else {
                    pp::InitialDefinition::Undef(name.into())
                });
            }
            "--edition" => {
                let value = match value {
                    "2021" => lex::Edition::Rust2021,
                    "2024" => lex::Edition::Rust2024,
                    _ => return Err("expected --edition 2021 or 2024".into()),
                };
                set(&mut edition, value, flag)?;
                cli.options.edition = value;
            }
            "--diagnostic-format" => {
                if !matches!(value, "human" | "json") {
                    return Err("expected --diagnostic-format human or json".into());
                }
                set(&mut format, value, flag)?;
                cli.json = value == "json";
            }
            _ => unreachable!(),
        }
    }
    if cli.entry.is_none() || cli.output.is_none() {
        return Err("expected an input file and -o OUTPUT".into());
    }
    Ok(Some(cli))
}

pub(super) fn run(args: &[String]) -> i32 {
    let cli = match parse(args) {
        Ok(Some(cli)) => cli,
        Ok(None) => {
            println!("{HELP}");
            return 0;
        }
        Err(error) => {
            eprintln!("rpp preprocess: {error}");
            return 2;
        }
    };
    let json = cli.json;
    match execute(cli) {
        Ok(()) => 0,
        Err(error) => {
            if json {
                eprintln!("{}", artifacts::diagnostic(error.as_ref()));
            } else {
                eprintln!("rpp preprocess: {error}");
            }
            1
        }
    }
}

fn absolute(cwd: &Path, path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in cwd.join(path).components() {
        match component {
            Component::CurDir => (),
            Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component),
        }
    }
    normalized
}

fn canonical(path: &Path) -> crate::Result<PathBuf> {
    fs::canonicalize(path).map_err(|e| format!("{}: {e}", path.display()).into())
}

fn beneath(path: &Path, root: &Path) -> crate::Result<PathBuf> {
    path.strip_prefix(root)
        .map(Path::to_path_buf)
        .map_err(|_| format!("{} escapes source root {}", path.display(), root.display()).into())
}

fn execute(cli: Cli) -> crate::Result<()> {
    let cwd = env::current_dir()?;
    let entry = absolute(&cwd, &cli.entry.unwrap());
    let output = absolute(&cwd, &cli.output.unwrap());
    let root = cli.root.map_or_else(
        || entry.parent().unwrap_or(&entry).into(),
        |p| absolute(&cwd, &p),
    );
    let physical_root = canonical(&root)?;
    if !physical_root.is_dir() {
        return Err(format!("{} is not a source directory", root.display()).into());
    }

    let mut physical_inputs: Vec<PathBuf> = Vec::new();
    let mut sources = pp::Sources::default();
    for path in std::iter::once(entry.clone()).chain(cli.inputs.iter().map(|p| absolute(&cwd, p))) {
        let logical = beneath(&path, &root)?;
        let physical = canonical(&path)?;
        beneath(&physical, &physical_root)?;
        if !physical.is_file() {
            return Err(format!("{} is not a regular input file", path.display()).into());
        }
        if physical_inputs
            .iter()
            .any(|old| artifacts::aliases(old, &physical))
        {
            return Err(format!("duplicate input alias: {}", path.display()).into());
        }
        let text = fs::read_to_string(&physical).map_err(|e| format!("{}: {e}", path.display()))?;
        sources.insert(logical, text)?;
        physical_inputs.push(physical);
    }
    let mut options = cli.options;
    for dir in &mut options.include_dirs {
        let path = absolute(&cwd, dir);
        let physical = canonical(&path)?;
        beneath(&physical, &physical_root)?;
        if !physical.is_dir() {
            return Err(format!("{} is not an include directory", path.display()).into());
        }
        *dir = beneath(&path, &root)?;
    }

    let logical_entry = beneath(&entry, &root)?;
    let result = pp::preprocess(&sources, &logical_entry, &options)?;
    check_single_file(&result, &sources, &logical_entry, options.edition)?;

    let mut files = Vec::new();
    if let Some(depfile) = cli.depfile {
        let dep = artifacts::depfile(&cwd, &root, &output, &result)?;
        files.push((absolute(&cwd, &depfile), dep.into_bytes()));
    }
    if let Some(map) = cli.map {
        let generated = output.strip_prefix(&cwd).unwrap_or(&output);
        files.push((
            absolute(&cwd, &map),
            artifacts::source_map(generated, &result)?.into_bytes(),
        ));
    }
    files.push((output, result.text.into_bytes()));
    artifacts::publish(files, &physical_inputs)
}

fn check_single_file(
    output: &pp::Output,
    inputs: &pp::Sources,
    entry: &Path,
    edition: lex::Edition,
) -> crate::Result<()> {
    let error_at = |span: lex::Span, kind, message: String| {
        let mapping = output
            .mappings
            .iter()
            .find(|m| m.generated.end > span.start)
            .or_else(|| output.mappings.last());
        let mut location = mapping
            .map(|m| m.original.clone())
            .unwrap_or_else(|| pp::Location {
                source: pp::SourceName::File(entry.to_owned()),
                span: lex::Span { start: 0, end: 0 },
                line: 1,
                column: 1,
            });
        if let Some((m, path)) = mapping
            .filter(|m| m.exact)
            .and_then(|m| match &location.source {
                pp::SourceName::File(path) => Some((m, path)),
                pp::SourceName::Predefined(_) => None,
            })
        {
            let start = m.original.span.start + span.start.saturating_sub(m.generated.start);
            if let Some(pos) = inputs
                .text(path)
                .and_then(|text| lex::source_position(text, start))
            {
                location.span = lex::Span {
                    start,
                    end: start + span.end.min(m.generated.end).saturating_sub(span.start),
                };
                location.line = pos.line;
                location.column = pos.column;
            }
        }
        pp::Error {
            kind,
            message,
            location,
            notes: mapping.map_or_else(Vec::new, |m| m.definitions.clone()),
            include_stack: mapping.map_or_else(Vec::new, |m| m.include_stack.clone()),
        }
    };
    let lexed = lex::scan_generated(&output.text, edition).map_err(|e| {
        error_at(
            e.span,
            pp::ErrorKind::Lexical,
            format!("generated Rust: {:?}", e.kind),
        )
    })?;
    let tokens: Vec<_> = lexed.tokens.iter().filter(|t| !t.kind.trivia()).collect();
    for t in tokens.windows(3) {
        if !matches!(t[0].kind, lex::Kind::Identifier { .. }) {
            continue;
        }
        let name = lexed.text(t[0].span);
        let load = matches!(
            name.trim_start_matches("r#"),
            "include" | "include_str" | "include_bytes"
        ) && t[1].kind == lex::Kind::Punct('!')
            && matches!(t[2].kind, lex::Kind::Open(_))
            && !lexed.protected.iter().any(|r| {
                r.kind == lex::Protection::MacroDefinition
                    && r.span.start <= t[0].span.start
                    && t[2].span.end <= r.span.end
            });
        let module = name == "mod"
            && matches!(t[1].kind, lex::Kind::Identifier { .. })
            && t[2].kind == lex::Kind::Punct(';')
            && !lexed.protected(t[0].span);
        if load || module {
            return Err(error_at(
                t[0].span,
                pp::ErrorKind::InvalidInput,
                format!(
                    "{name} loads a Rust source or data file and requires source-tree preprocessing"
                ),
            )
            .into());
        }
    }
    Ok(())
}
