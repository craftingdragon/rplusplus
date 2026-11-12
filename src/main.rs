// SPDX-License-Identifier: GPL-3.0-or-later
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;
fn run(tool: &str, args: &[String]) -> Result<()> {
    let status = Command::new(tool).args(args).status()?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
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
fn embed(path: &Path, args: &[String]) -> Result<()> {
    use std::io::Write;
    let bytes = args.join("\0").into_bytes();
    let mut file = fs::OpenOptions::new().append(true).open(path)?;
    write!(
        file,
        "{:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n",
        "rpp-args/",
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
fn embedded(path: &Path) -> Result<Vec<String>> {
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
        let member = bytes.get(offset..offset + size).ok_or("truncated rlib")?;
        if header.starts_with(b"rpp-args/") {
            return Ok(member
                .split(|b| *b == 0)
                .map(|s| String::from_utf8(s.to_vec()))
                .collect::<std::result::Result<_, _>>()?);
        }
        offset += size + size % 2;
    }
    Err("missing rpp compile record".into())
}
fn main() {
    if let Err(e) = execute() {
        eprintln!("rpp: {e}");
        std::process::exit(1);
    }
}
fn execute() -> Result<()> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--version") {
        println!("rpp 0.1.0");
        return Ok(());
    }
    if args.is_empty() || args[0] == "--help" {
        println!("rpp compile|link --rustc PATH [options]\nCargo-free Rust compiler driver");
        return Ok(());
    }
    let mut expanded = Vec::new();
    for arg in args {
        if let Some(path) = arg.strip_prefix('@') {
            expanded.extend(read_args(Path::new(path))?);
        } else {
            expanded.push(arg);
        }
    }
    let mut args = expanded;
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
            if sidecar.exists() {
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
                        runtime.extend(libs.split_whitespace().map(String::from));
                    } else {
                        eprintln!("{line}");
                    }
                }
                if !result.status.success() {
                    std::process::exit(result.status.code().unwrap_or(1));
                }
                write_args(Path::new(&format!("{destination}.runtime")), &runtime)?;
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
    fn missing_option_value_is_an_error() {
        assert!(take(&mut vec!["-o".into()], "-o").is_err());
    }
}
