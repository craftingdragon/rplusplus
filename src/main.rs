// SPDX-License-Identifier: GPL-3.0-or-later
#[allow(dead_code)]
mod lex;
mod preprocess;
mod preprocess_cli;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
#[derive(Debug)]
struct ToolFailure(i32);
impl std::fmt::Display for ToolFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "compiler or linker exited with status {}", self.0)
    }
}
impl std::error::Error for ToolFailure {}
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn run(tool: &str, args: &[String]) -> Result<()> {
    let status = Command::new(tool).args(args).status()?;
    if !status.success() {
        return Err(Box::new(ToolFailure(status.code().unwrap_or(1))));
    }
    Ok(())
}
fn take(args: &mut Vec<String>, flag: &str) -> Result<Option<String>> {
    if let Some(i) = args.iter().position(|a| a == flag) {
        args.remove(i);
        if i == args.len() {
            return Err(format!("missing value for {flag}").into());
        }
        Ok(Some(args.remove(i)))
    } else {
        Ok(None)
    }
}
#[cfg(test)]
fn write_args(path: &Path, args: &[String]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut bytes = Vec::new();
    for arg in args {
        bytes.extend_from_slice(arg.as_bytes());
        bytes.push(0);
    }
    fs::write(path, bytes)?;
    Ok(())
}
fn read_args(path: &Path) -> Result<Vec<String>> {
    Ok(fs::read(path)?
        .split(|b| *b == 0)
        .filter(|a| !a.is_empty())
        .map(|a| String::from_utf8(a.to_vec()))
        .collect::<std::result::Result<_, _>>()?)
}
// Store the private compile record as a non-object ar member, so metadata and
// compiler output have one build-system owner and cannot become separated.
fn embed_named(path: &Path, name: &str, args: &[String]) -> Result<()> {
    use std::io::Write;
    let bytes = args.join("\0").into_bytes();
    let mut file = fs::OpenOptions::new().append(true).open(path)?;
    write!(
        file,
        "{:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n",
        name,
        0,
        0,
        0,
        "644",
        bytes.len()
    )?;
    file.write_all(&bytes)?;
    if bytes.len() % 2 != 0 {
        file.write_all(b"\n")?;
    }
    Ok(())
}
fn embed(path: &Path, args: &[String]) -> Result<()> {
    embed_named(path, "rpp-args/", args)
}
fn embedded_named(path: &Path, name: &[u8]) -> Result<Option<Vec<String>>> {
    let bytes = fs::read(path)?;
    if !bytes.starts_with(b"!<arch>\n") {
        return Err("invalid rlib archive".into());
    }
    let mut offset = 8;
    while offset + 60 <= bytes.len() {
        let header = &bytes[offset..offset + 60];
        let size = std::str::from_utf8(&header[48..58])?
            .trim()
            .parse::<usize>()?;
        offset += 60;
        let end = offset
            .checked_add(size)
            .ok_or("invalid archive member size")?;
        if &header[58..60] != b"`\n" {
            return Err("invalid archive member header".into());
        }
        let member = bytes.get(offset..end).ok_or("truncated rlib")?;
        if header.starts_with(name) {
            return Ok(Some(
                member
                    .split(|b| *b == 0)
                    .map(|s| String::from_utf8(s.to_vec()))
                    .collect::<std::result::Result<_, _>>()?,
            ));
        }
        offset = end.checked_add(size % 2).ok_or("invalid archive padding")?;
    }
    if offset != bytes.len() {
        return Err("truncated archive header".into());
    }
    Ok(None)
}
fn embedded(path: &Path) -> Result<Vec<String>> {
    embedded_named(path, b"rpp-args/")?.ok_or_else(|| "missing rpp compile record".into())
}
fn main() {
    if let Err(e) = execute() {
        eprintln!("rpp: {e}");
        std::process::exit(e.downcast_ref::<ToolFailure>().map_or(1, |e| e.0));
    }
}
fn execute() -> Result<()> {
    let mut args = Vec::new();
    for arg in env::args().skip(1) {
        if let Some(path) = arg.strip_prefix('@') {
            args.extend(read_args(Path::new(path))?);
        } else {
            args.push(arg);
        }
    }
    if args.first().is_some_and(|op| op == "preprocess") {
        let status = preprocess_cli::run(&args[1..]);
        if status != 0 {
            std::process::exit(status);
        }
        return Ok(());
    }
    let Some(index) = args.iter().position(|s| s == "-o") else {
        return execute_inner(args);
    };
    let Some(output) = args.get(index + 1).cloned() else {
        return execute_inner(args);
    };
    let staged = format!("{output}.rpp-stage.rlib");
    args[index + 1] = staged.clone();
    let dep = args
        .iter()
        .position(|s| s == "--depfile")
        .and_then(|i| args.get(i + 1))
        .cloned();
    if let Some(dep) = &dep {
        let i = args.iter().position(|s| s == "--depfile").unwrap();
        args[i + 1] = format!("{dep}.rpp-stage");
    }
    let result = (|| {
        execute_inner(args)?;
        if let Some(dep) = &dep {
            let temp = format!("{dep}.rpp-stage");
            let escape = |s: &str| {
                s.replace('\\', "\\\\")
                    .replace(' ', "\\ ")
                    .replace('#', "\\#")
            };
            let text = fs::read_to_string(&temp)?.replace(&escape(&staged), &escape(&output));
            fs::write(&temp, text)?;
            fs::rename(temp, dep)?;
        }
        fs::rename(&staged, &output)?;
        Ok(())
    })();
    let _ = fs::remove_file(&staged);
    let _ = fs::remove_file(format!("{staged}.rust.a"));
    if let Some(dep) = dep {
        let _ = fs::remove_file(format!("{dep}.rpp-stage"));
    }
    result
}
fn execute_inner(args: Vec<String>) -> Result<()> {
    if args.first().map(String::as_str) == Some("--version") {
        println!("rpp 0.1.0");
        return Ok(());
    }
    if args.is_empty() || args[0] == "--help" {
        println!("rpp compile|link --rustc PATH [options]\nrpp preprocess INPUT -o OUTPUT [options]\nCargo-free Rust compiler driver");
        return Ok(());
    }
    let mut args = args;
    let op = args.remove(0);
    let rustc = take(&mut args, "--rustc")?.unwrap_or_else(|| "rustc".into());
    let cxx = take(&mut args, "--cxx")?
        .filter(|a| !a.is_empty())
        .unwrap_or_else(|| "c++".into());
    if op == "rustc-version" {
        return run(&rustc, &["--version".into()]);
    }
    let output = take(&mut args, "-o")?.ok_or("missing -o")?;
    if op == "compile" {
        let kind = take(&mut args, "--kind")?.unwrap_or_else(|| "staticlib".into());
        let entry = take(&mut args, "--entry")?.unwrap_or_else(|| "Rpp".into());
        let dep = take(&mut args, "--depfile")?.ok_or("missing depfile")?;
        let edition = take(&mut args, "--edition")?.unwrap_or_else(|| "2024".into());
        let name = take(&mut args, "--name")?.unwrap_or_else(|| "rpp_probe".into());
        if !["bin", "rlib", "staticlib"].contains(&kind.as_str()) {
            return Err("unsupported crate type".into());
        }
        if args.iter().any(|a| {
            a.starts_with("--crate-type")
                || a.starts_with("--emit")
                || a.starts_with("--crate-name")
        }) {
            return Err("driver manages crate type, name and emitted outputs".into());
        }
        if args
            .iter()
            .any(|a| a.starts_with("--target") || a.starts_with("--sysroot"))
        {
            return Err("Rpp v0.1 supports native builds only; remove --target/--sysroot and select a matching installed rustc".into());
        }
        use std::hash::{Hash, Hasher};
        let mut identity = std::collections::hash_map::DefaultHasher::new();
        output.hash(&mut identity);
        let mut saved = vec![
            format!("--edition={edition}"),
            format!("--crate-name={name}"),
        ];
        saved.push(format!("-Cmetadata={:x}", identity.finish()));
        saved.extend(args);
        let mut compile = saved.clone();
        if kind == "bin" && entry == "Rpp" {
            compile.push("--cap-lints=allow".into());
        }
        compile.extend([
            "--crate-type=rlib".into(),
            format!("--emit=link,dep-info={dep}"),
            "-o".into(),
            output.clone(),
        ]);
        run(&rustc, &compile)?;
        let mut deptext = fs::read_to_string(&dep)?;
        let escape = |s: &str| {
            s.replace('\\', "\\\\")
                .replace(' ', "\\ ")
                .replace('#', "\\#")
        };
        let mut dependencies = Vec::new();
        let mut externs: Vec<String> = saved
            .iter()
            .filter_map(|a| a.strip_prefix("--extern=").map(String::from))
            .collect();
        for pair in saved.windows(2) {
            if pair[0] == "--extern" {
                externs.push(pair[1].clone());
            }
        }
        for item in externs {
            {
                if let Some((_, path)) = item.split_once('=') {
                    dependencies.push(escape(
                        &Path::new(path)
                            .strip_prefix(env::current_dir()?)
                            .unwrap_or(Path::new(path))
                            .to_string_lossy(),
                    ));
                }
            }
        }
        deptext = deptext.replacen('\n', &format!(" {}\n", dependencies.join(" ")), 1);
        fs::write(&dep, deptext)?;
        let mut data = vec!["rpp-record-v1".into(), kind, entry];
        data.extend(saved);
        embed(Path::new(&output), &data)?;
    } else if op == "link" {
        let archive = if let Some(i) = args.iter().position(|a| a == "--rpp-archive") {
            args.remove(i);
            true
        } else {
            false
        };
        let ar = take(&mut args, "--ar")?.unwrap_or_else(|| "ar".into());
        let split = args
            .iter()
            .position(|a| a == "--libraries")
            .unwrap_or(args.len());
        let mut native = if split < args.len() {
            args.split_off(split + 1)
        } else {
            vec![]
        };
        if args.last().map(String::as_str) == Some("--libraries") {
            args.pop();
        }
        let records: Vec<_> = args
            .iter()
            .filter(|a| a.ends_with(".rlib"))
            .cloned()
            .collect();
        let objects: Vec<_> = args.into_iter().filter(|a| !a.ends_with(".rlib")).collect();
        let mut runtime = Vec::new();
        let mut boundaries = 0;
        for lib in &native {
            let sidecar = PathBuf::from(format!("{lib}.runtime"));
            let record = if lib.ends_with(".a") && Path::new(lib).is_file() {
                embedded_named(Path::new(lib), b"rpp-runtime/")?
            } else {
                None
            };
            if let Some(record) = record {
                if record.first().map(String::as_str) != Some("rpp-runtime-v1") {
                    return Err(format!(
                        "invalid Rpp runtime metadata in {lib}; rebuild the library"
                    )
                    .into());
                }
                boundaries += 1;
                runtime.extend(record.into_iter().skip(1));
            } else if sidecar.exists() {
                boundaries += 1;
                runtime.extend(read_args(&sidecar)?);
            }
        }
        native.retain(|a| !a.ends_with(".rlib"));
        if records.len() > 1 {
            return Err("each target must have one Rust crate root".into());
        }
        if let Some(object) = records.first() {
            let data = embedded(Path::new(object))?;
            if data.len() < 4 || data.first().map(String::as_str) != Some("rpp-record-v1") {
                return Err("invalid compile record".into());
            }
            let kind = &data[1];
            let entry = &data[2];
            if kind == "rlib" {
                if !objects.is_empty() {
                    return Err("rlib cannot contain C++ sources".into());
                }
                fs::copy(object, &output)?;
                return Ok(());
            }
            let static_boundary = kind == "staticlib" || entry == "CXX";
            let destination = if kind == "bin" && static_boundary {
                format!("{output}.rust.a")
            } else {
                output.clone()
            };
            let mut compile = data[3..].to_vec();
            compile.extend([
                format!(
                    "--crate-type={}",
                    if static_boundary { "staticlib" } else { "bin" }
                ),
                "-o".into(),
                destination.clone(),
            ]);
            if static_boundary {
                if boundaries > 0 {
                    return Err("multiple Rust static runtime boundaries".into());
                }
                compile.push("--print=native-static-libs".into());
                let result = Command::new(&rustc)
                    .args(&compile)
                    .stdout(Stdio::inherit())
                    .output()?;
                let stderr = String::from_utf8_lossy(&result.stderr);
                for line in stderr.lines() {
                    if let Some((_, libs)) = line.split_once("native-static-libs:") {
                        runtime.extend(
                            libs.split('"')
                                .next()
                                .unwrap_or(libs)
                                .split_whitespace()
                                .map(String::from),
                        );
                    } else {
                        eprintln!("{line}");
                    }
                }
                if !result.status.success() {
                    return Err(Box::new(ToolFailure(result.status.code().unwrap_or(1))));
                }
                let mut record = vec!["rpp-runtime-v1".into()];
                record.extend(runtime.clone());
                embed_named(Path::new(&destination), "rpp-runtime/", &record)?;
                if kind == "staticlib" {
                    if !objects.is_empty() {
                        let mut a = vec!["q".into(), destination];
                        a.extend(objects);
                        run(&ar, &a)?;
                    }
                    return Ok(());
                }
                native.insert(0, destination);
            } else {
                compile.push(format!("-Clinker={cxx}"));
                for arg in objects.iter().chain(native.iter()) {
                    compile.push(format!("-Clink-arg={arg}"));
                }
                return run(&rustc, &compile);
            }
        }
        if archive && records.is_empty() {
            if Path::new(&output).exists() {
                fs::remove_file(&output)?;
            }
            let mut archive_args = vec!["qc".into(), output];
            archive_args.extend(objects);
            return run(&ar, &archive_args);
        }
        if boundaries > 1 {
            return Err("multiple Rust static runtime boundaries".into());
        }
        let mut link = objects;
        link.extend(native);
        link.extend(runtime);
        link.extend(["-o".into(), output]);
        run(&cxx, &link)?;
    } else {
        return Err(format!("unknown operation: {op}").into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn records_preserve_spaces_and_unicode() {
        let path = env::temp_dir().join(format!("rpp-record-test-{}", std::process::id()));
        let values = vec!["path with spaces/🦀".into(), "--cfg=feature=\"x\"".into()];
        write_args(&path, &values).unwrap();
        assert_eq!(read_args(&path).unwrap(), values);
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn runtime_archive_record_is_self_contained() {
        let path = env::temp_dir().join(format!("rpp-runtime-test-{}", std::process::id()));
        fs::write(&path, b"!<arch>\n").unwrap();
        let record = vec!["rpp-runtime-v1".into(), "-lm".into(), "-lm".into()];
        embed_named(&path, "rpp-runtime/", &record).unwrap();
        assert_eq!(
            embedded_named(&path, b"rpp-runtime/").unwrap(),
            Some(record)
        );
        let mut bytes = fs::read(&path).unwrap();
        bytes.truncate(bytes.len() - 3);
        fs::write(&path, bytes).unwrap();
        assert!(embedded_named(&path, b"rpp-runtime/").is_err());
        fs::remove_file(path).unwrap();
    }
    #[test]
    fn missing_option_value_is_an_error() {
        assert!(take(&mut vec!["-o".into()], "-o").is_err());
    }
}
