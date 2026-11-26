// SPDX-License-Identifier: GPL-3.0-or-later
use super::{Cli, absolute, artifacts, beneath, canonical, pp, references, snapshot};
use crate::Result;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Role {
    Rust,
    Fragment,
    Data,
}

pub(super) fn execute(cli: Cli, cwd: &Path) -> Result<()> {
    let root = absolute(cwd, &cli.root.unwrap());
    let physical_root = canonical(&root)?;
    let out = absolute(cwd, &cli.output_dir.unwrap());
    let physical_out = snapshot::prospective_path(&out)?;
    if !physical_root.is_dir() || root.starts_with(&out) || physical_root.starts_with(&physical_out)
    {
        return Err(
            "output directory overlaps the source root or source root is not a directory".into(),
        );
    }

    let resolve = |path: &Path| -> Result<(PathBuf, PathBuf)> {
        let path = absolute(cwd, path);
        let rel = beneath(&path, &root)?;
        let physical = canonical(&path)?;
        beneath(&physical, &physical_root)?;
        if path.starts_with(&out) || physical.starts_with(&physical_out) {
            return Err(format!("{} overlaps the generated subtree", path.display()).into());
        }
        Ok((rel, physical))
    };

    let entry = cli.entry.unwrap();
    let (logical_entry, _) = resolve(&entry)?;
    if cli
        .output
        .is_some_and(|p| absolute(cwd, &p) != out.join(&logical_entry))
    {
        return Err("-o must name the mirrored entry under --output-dir".into());
    }
    let mut roles = BTreeMap::new();
    let mut sources = pp::Sources::default();
    let mut assets = BTreeMap::new();
    let mut physical_inputs = Vec::<PathBuf>::new();
    for (path, role) in std::iter::once((&entry, Role::Rust))
        .chain(cli.sources.iter().map(|p| (p, Role::Rust)))
        .chain(cli.inputs.iter().map(|p| (p, Role::Fragment)))
        .chain(cli.data.iter().map(|p| (p, Role::Data)))
    {
        let (rel, physical) = resolve(path)?;
        snapshot::validate_relative(&rel)?;
        if rel.starts_with(".rpp") || !physical.is_file() {
            return Err(format!(
                "expected a regular input outside reserved .rpp: {}",
                path.display()
            )
            .into());
        }
        if roles.insert(rel.clone(), role).is_some()
            || physical_inputs
                .iter()
                .any(|p| artifacts::aliases(p, &physical))
        {
            return Err(format!(
                "duplicate input alias or conflicting roles: {}",
                path.display()
            )
            .into());
        }
        let bytes = fs::read(&physical).map_err(|e| format!("{}: {e}", path.display()))?;
        if role == Role::Data {
            assets.insert(rel, bytes);
        } else {
            let text = String::from_utf8(bytes).map_err(|e| format!("{}: {e}", path.display()))?;
            if role == Role::Rust {
                sources.insert_unit(&rel, text)?;
            } else {
                sources.insert(rel, text)?;
            }
        }
        physical_inputs.push(physical);
    }

    let mut options = cli.options;
    let managed = cli.cmake_source_dir.is_some();
    if managed {
        let mut definitions = BTreeMap::new();
        for definition in &options.definitions {
            if let pp::InitialDefinition::Define(name, value) = definition {
                if definitions
                    .insert(name, value)
                    .is_some_and(|old| old != value)
                {
                    return Err(format!(
                        "{}: conflicting initial definition of {name}",
                        out.display()
                    )
                    .into());
                }
            }
        }
    }
    for dir in &mut options.include_dirs {
        let path = absolute(cli.cmake_source_dir.as_deref().unwrap_or(cwd), dir);
        let (rel, physical) = resolve(&path)?;
        if !physical.is_dir() {
            return Err(format!("{} is not an include directory", dir.display()).into());
        }
        *dir = rel;
    }

    let sidecar = |p: PathBuf| -> Result<PathBuf> {
        let rel = beneath(&absolute(cwd, &p), &out)?;
        snapshot::validate_relative(&rel)?;
        if snapshot::control(&rel) || rel == Path::new(".rpp") {
            return Err(format!("reserved snapshot destination: {}", rel.display()).into());
        }
        Ok(rel)
    };
    let map = cli
        .map
        .map(sidecar)
        .transpose()?
        .unwrap_or_else(|| ".rpp/source-map.json".into());
    let dep = cli.depfile.map(sidecar).transpose()?;

    let mut deps: BTreeSet<_> = roles
        .iter()
        .filter(|(_, r)| **r != Role::Fragment)
        .map(|(p, _)| p.clone())
        .collect();
    let mut results = BTreeMap::new();
    for (path, role) in &roles {
        if *role == Role::Rust {
            let result = pp::preprocess(&sources, path, &options)?;
            deps.extend(result.dependencies.iter().cloned());
            results.insert(path.clone(), result);
        }
    }
    references::validate(&results, &roles, &sources, options.edition)?;

    let mut payload = assets;
    let maps = results
        .iter()
        .map(|(path, result)| artifacts::source_map(path, result))
        .collect::<Result<Vec<_>>>()?;
    let map_bytes = format!(
        "{{\"version\":1,\"sources\":[{}]}}\n",
        maps.iter()
            .map(|m| m.trim_end())
            .collect::<Vec<_>>()
            .join(",")
    );
    for (path, result) in results {
        payload.insert(path, result.text.into_bytes());
    }
    if payload
        .insert(map.clone(), map_bytes.into_bytes())
        .is_some()
    {
        return Err("source map collides with mirrored input".into());
    }
    let dependency = dep
        .map(|path| -> Result<_> {
            if payload.contains_key(&path) {
                return Err("depfile collides with mirrored input or source map".into());
            }
            let contents =
                artifacts::dependency_rule(cwd, &root, &out.join(snapshot::COMPLETE), &deps)?;
            Ok((path, contents.into_bytes()))
        })
        .transpose()?;
    let spec = snapshot::TreeSpec {
        entry: &logical_entry,
        map: &map,
        edition: match options.edition {
            crate::lex::Edition::Rust2021 => "2021",
            crate::lex::Edition::Rust2024 => "2024",
        },
    };
    snapshot::publish(&out, spec, payload, dependency, &physical_inputs, managed)
}
