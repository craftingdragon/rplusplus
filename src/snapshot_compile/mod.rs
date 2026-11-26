// SPDX-License-Identifier: GPL-3.0-or-later
mod depfile;
mod diagnostics;
mod json;

use crate::{
    Result, ToolFailure,
    preprocess_cli::{absolute, snapshot},
};
use std::{
    fs,
    io::IsTerminal,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

pub(crate) const RECORD: &str = "rpp-record-v2";

pub(crate) struct Snapshot {
    directory: PathBuf,
    original: PathBuf,
    verified: snapshot::Verified,
}

impl Snapshot {
    fn open(directory: &Path) -> Result<(PathBuf, snapshot::Verified)> {
        let verified = snapshot::verify(directory).map_err(|e| {
            format!(
                "invalid preprocessing snapshot {}: {e}; regenerate the tree and rebuild",
                directory.display()
            )
        })?;
        Ok((fs::canonicalize(directory)?, verified))
    }

    pub(crate) fn compile(
        dir: &str,
        edition: &str,
        original_entry: Option<&str>,
        args: &mut [String],
    ) -> Result<Self> {
        let cwd = std::env::current_dir()?;
        let (directory, verified) = Self::open(Path::new(dir))?;
        if verified.edition != edition {
            return Err(format!("snapshot edition {} does not match compile edition {edition}; regenerate the tree or select the matching edition", verified.edition).into());
        }
        // Only driver-owned output/entry arguments may alter the audited
        // rustc invocation. The v0.1 flag behavior outside snapshot mode stays intact.
        if args.iter().any(|a| {
            a.starts_with('@')
                || matches!(a.as_str(), "-o" | "--out-dir" | "--print" | "--edition")
                || a.starts_with("--out-dir=")
                || a.starts_with("--print=")
                || a.starts_with("--edition=")
        }) {
            return Err("snapshot compilation manages rustc output and edition flags and requires expanded arguments".into());
        }
        let source = args
            .last_mut()
            .ok_or("snapshot compile requires the original entry as the final argument")?;
        if source.starts_with('-') {
            return Err(
                "snapshot compile requires the original entry as the final argument".into(),
            );
        }
        let generated = directory.join(&verified.entry);
        if original_entry.is_some()
            && fs::canonicalize(absolute(&cwd, Path::new(source)))? != generated
        {
            return Err("native compile source does not match the verified snapshot entry".into());
        }
        let original_entry = original_entry.unwrap_or(source);
        let mut original = absolute(&cwd, Path::new(original_entry));
        if !original.ends_with(&verified.entry) {
            return Err(format!(
                "original entry {original_entry} must end with {}",
                verified.entry.display()
            )
            .into());
        }
        for _ in verified.entry.components() {
            original.pop();
        }
        *source = utf8(&generated)?.to_owned();
        Ok(Self {
            directory,
            original,
            verified,
        })
    }

    pub(crate) fn from_record(args: &[String]) -> Result<Self> {
        let [directory, identity, original, ..] = args else {
            return Err("truncated snapshot compile record".into());
        };
        let (directory, verified) = Self::open(Path::new(directory))?;
        if verified.identity != *identity {
            return Err("preprocessing snapshot changed since compilation; rebuild the crate before linking".into());
        }
        Ok(Self {
            directory,
            original: original.into(),
            verified,
        })
    }

    pub(crate) fn record(&self, kind: String, entry: String) -> Result<Vec<String>> {
        Ok(vec![
            RECORD.into(),
            kind,
            entry,
            utf8(&self.directory)?.into(),
            self.verified.identity.clone(),
            utf8(&self.original)?.into(),
        ])
    }

    pub(crate) fn outside(&self, path: &str) -> Result<()> {
        let path = Path::new(path);
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let canonical_parent = fs::canonicalize(parent)?;
        if canonical_parent.starts_with(&self.directory) {
            return Err(format!(
                "compiler output must be outside the verified snapshot: {}",
                path.display()
            )
            .into());
        }
        let original_entry = self.original.join(&self.verified.entry);
        if crate::preprocess_cli::artifacts::aliases(path, &original_entry) {
            return Err("compiler output must not overwrite the original entry".into());
        }
        Ok(())
    }

    pub(crate) fn run(
        &self,
        rustc: &str,
        mut args: Vec<String>,
        destination: &str,
        dep: &Path,
        native: bool,
    ) -> Result<depfile::Report> {
        self.outside(destination)?;
        self.outside(utf8(dep)?)?;
        if utf8(dep)?.contains(',') {
            return Err("snapshot compiler depfile path cannot contain a comma".into());
        }
        args.push(format!("--emit=link,dep-info={}", utf8(dep)?));
        let mode = diagnostics::presentation(&args);
        let cwd = std::env::current_dir()?;
        let can_map = matches!(mode, diagnostics::Presentation::Mapped)
            && !args
                .iter()
                .any(|a| a == "--remap-path-prefix" || a.starts_with("--remap-path-prefix="));
        let sources = can_map
            .then(|| diagnostics::SourceMap::read(&self.directory, &self.verified))
            .transpose()?;
        if matches!(mode, diagnostics::Presentation::Mapped) {
            // rustc does not take a human color switch for JSON rendering.
            let mut color = "auto".to_owned();
            let mut filtered = Vec::new();
            let mut iter = args.into_iter();
            while let Some(arg) = iter.next() {
                if arg == "--color" {
                    color = iter.next().ok_or("missing --color value")?;
                } else if let Some(value) = arg.strip_prefix("--color=") {
                    color = value.to_owned();
                } else {
                    filtered.push(arg);
                }
            }
            args = filtered;
            args.push("--error-format=json".into());
            match color.as_str() {
                "always" => args.push("--json=diagnostic-rendered-ansi".into()),
                "auto" if std::io::stderr().is_terminal() => {
                    args.push("--json=diagnostic-rendered-ansi".into())
                }
                "auto" | "never" => (),
                _ => return Err("invalid --color option".into()),
            }
        }
        let runtime_file = if native && matches!(mode, diagnostics::Presentation::ExplicitHuman) {
            let path = dep.with_extension("rpp-native-libs");
            self.outside(utf8(&path)?)?;
            remove_if_present(&path)?;
            let flag = args
                .iter_mut()
                .find(|a| *a == "--print=native-static-libs")
                .ok_or("missing native static library query")?;
            *flag = format!("--print=native-static-libs={}", utf8(&path)?);
            Some(RuntimeFile(path))
        } else {
            None
        };
        // A successful compiler must produce new dep-info for this invocation.
        remove_if_present(dep)?;
        remove_if_present(Path::new(destination))?;
        let result = Command::new(rustc)
            .args(&args)
            .stdout(Stdio::inherit())
            .output()?;
        let raw = matches!(mode, diagnostics::Presentation::Json);
        let mut libs = diagnostics::report(
            &result.stderr,
            mode,
            native && runtime_file.is_none(),
            sources.as_ref().map(|m| {
                (
                    m,
                    cwd.as_path(),
                    self.directory.as_path(),
                    self.original.as_path(),
                )
            }),
        )?;
        if !result.status.success() {
            return Err(Box::new(ToolFailure(
                result.status.code().unwrap_or(1),
                raw,
            )));
        }
        if let Some(file) = &runtime_file {
            libs = Some(
                fs::read_to_string(&file.0)?
                    .split_whitespace()
                    .map(String::from)
                    .collect(),
            );
        }
        let (_, next) = Self::open(&self.directory)?;
        if next.identity != self.verified.identity {
            return Err(
                "preprocessing snapshot changed during compilation; rebuild the crate".into(),
            );
        }

        let externs = depfile::externs(&args, &cwd);
        let mut allowed = externs.clone();
        allowed.extend(
            self.verified
                .payload
                .iter()
                .filter(|p| *p != &self.verified.map)
                .map(|p| self.directory.join(p)),
        );
        let entry = self.directory.join(&self.verified.entry);
        let mut report = depfile::audit(dep, destination, &cwd, &entry, &allowed)?;
        report.files.extend(externs);
        report.files.insert(self.directory.join(snapshot::MANIFEST));
        report.files.insert(self.directory.join(&self.verified.map));
        report.runtime = if native {
            libs.ok_or("rustc did not report native static libraries")?
        } else {
            Vec::new()
        };
        Ok(report)
    }
}

pub(crate) fn utf8(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| format!("expected a UTF-8 path: {}", path.display()).into())
}

struct RuntimeFile(PathBuf);

impl Drop for RuntimeFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub(crate) fn remove_if_present(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
