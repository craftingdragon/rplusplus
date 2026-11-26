// SPDX-License-Identifier: GPL-3.0-or-later
use super::{artifacts, canonical, sha256};
use crate::Result;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Component, Path, PathBuf},
};

pub(super) const COMPLETE: &str = ".rpp/complete";
pub(crate) const MANIFEST: &str = ".rpp/manifest";
const OWNED: &str = ".rpp/owned";
const MANIFEST_MAGIC: &str = "rpp-tree-v2";
const OWNED_MAGIC: &str = "rpp-owned-v1";

pub(super) fn control(path: &Path) -> bool {
    [COMPLETE, MANIFEST, OWNED]
        .iter()
        .any(|p| path.starts_with(p))
}

pub(crate) fn validate_relative(path: &Path) -> Result<()> {
    let value = path.to_str().ok_or("snapshot paths must be UTF-8")?;
    if value.is_empty() || value.contains('\0')
        || !path.components().all(|c| matches!(c, Component::Normal(p) if !p.to_string_lossy().starts_with(".rpp-stage-")))
        || path.components().collect::<PathBuf>().as_os_str() != path.as_os_str()
    {
        return Err(format!("invalid or reserved snapshot path: {}", path.display()).into());
    }
    Ok(())
}

pub(super) fn prospective_path(path: &Path) -> Result<PathBuf> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => canonical(path),
        Ok(_) => Err(format!("{} must be a real output directory", path.display()).into()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            // The final directory cannot be a symlink, but its existing parent
            // can be one. Resolve that parent before checking source overlap.
            let mut ancestor = path.parent().ok_or("missing output parent")?;
            let base = loop {
                match fs::symlink_metadata(ancestor) {
                    Ok(_) => {
                        let base = canonical(ancestor)?;
                        if !base.is_dir() {
                            return Err(
                                format!("{} must be a directory", ancestor.display()).into()
                            );
                        }
                        break base;
                    }
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {
                        ancestor = ancestor.parent().ok_or("missing output parent")?
                    }
                    Err(e) => return Err(e.into()),
                }
            };
            Ok(base.join(path.strip_prefix(ancestor)?))
        }
        Err(e) => Err(e.into()),
    }
}

fn bytes(root: &Path, path: &Path) -> Result<Option<Vec<u8>>> {
    let file = root.join(path);
    match fs::symlink_metadata(&file) {
        Ok(meta) if meta.is_file() => Ok(Some(fs::read(file)?)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Ok(_) => Err(format!("{} must be a regular output file", file.display()).into()),
        Err(e) => Err(e.into()),
    }
}

fn temporary(path: &Path) -> PathBuf {
    path.with_file_name(format!(
        ".rpp-stage-{}",
        sha256::hex(path.to_str().unwrap().as_bytes())
    ))
}

fn remove(root: &Path, path: &Path) -> Result<()> {
    match fs::remove_file(root.join(path)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

fn replace(root: &Path, path: &Path, content: &[u8]) -> Result<()> {
    replace_with(root, path, content, || {})
}

fn replace_with(root: &Path, path: &Path, content: &[u8], staged: impl FnOnce()) -> Result<()> {
    let destination = root.join(path);
    fs::create_dir_all(destination.parent().ok_or("missing output parent")?)?;
    let tmp = temporary(path);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join(&tmp))?;
    let result = file.write_all(content);
    drop(file);
    let result = result.and_then(|()| {
        staged();
        fs::rename(root.join(&tmp), destination)
    });
    if result.is_err() {
        let _ = remove(root, &tmp);
    }
    result?;
    Ok(())
}

fn tree_entries(root: &Path) -> Result<(BTreeSet<PathBuf>, BTreeSet<PathBuf>)> {
    let mut files = BTreeSet::new();
    let mut dirs = BTreeSet::new();
    let mut pending = vec![PathBuf::new()];
    if !root.exists() {
        return Ok((files, dirs));
    }
    while let Some(dir) = pending.pop() {
        for item in fs::read_dir(root.join(dir))? {
            let item = item?;
            let path = item.path().strip_prefix(root)?.to_owned();
            let kind = item.file_type()?;
            if kind.is_file() {
                files.insert(path);
            } else if kind.is_dir() {
                dirs.insert(path.clone());
                pending.push(path);
            } else {
                return Err(format!(
                    "symlink or non-regular entry in generated tree: {}",
                    item.path().display()
                )
                .into());
            }
        }
    }
    Ok((files, dirs))
}

fn parents(path: &Path) -> impl Iterator<Item = &Path> {
    path.ancestors()
        .skip(1)
        .take_while(|p| !p.as_os_str().is_empty())
}

fn encode_owned(paths: &BTreeSet<PathBuf>) -> Vec<u8> {
    let mut out = format!("{OWNED_MAGIC}\0");
    for p in paths {
        out.push_str(p.to_str().unwrap());
        out.push('\0');
    }
    out.into_bytes()
}

fn fields(data: &[u8]) -> Result<impl Iterator<Item = &str>> {
    let text = std::str::from_utf8(data)?;
    Ok(text
        .strip_suffix('\0')
        .ok_or("invalid snapshot record terminator")?
        .split('\0'))
}

fn ownership(data: Option<&[u8]>) -> Result<BTreeSet<PathBuf>> {
    let Some(data) = data else {
        return Ok(BTreeSet::new());
    };
    let mut result = BTreeSet::new();
    let mut fields = fields(data)?;
    if fields.next() != Some(OWNED_MAGIC) {
        return Err("invalid snapshot ownership record".into());
    }
    for path in fields {
        let path = PathBuf::from(path);
        validate_relative(&path)?;
        if control(&path) || !result.insert(path) {
            return Err("invalid snapshot ownership entry".into());
        }
    }
    Ok(result)
}

pub(super) struct TreeSpec<'a> {
    pub entry: &'a Path,
    pub map: &'a Path,
    pub edition: &'a str,
}

fn manifest(
    spec: &TreeSpec<'_>,
    payload: &BTreeMap<PathBuf, Vec<u8>>,
    dep: Option<&Path>,
) -> Vec<u8> {
    let mut out = format!(
        "{MANIFEST_MAGIC}\0{}\0{}\0{}\0{}\0",
        spec.entry.to_str().unwrap(),
        spec.edition,
        spec.map.to_str().unwrap(),
        dep.map_or("", |p| p.to_str().unwrap())
    );
    for (p, content) in payload {
        out.push_str(p.to_str().unwrap());
        out.push('\0');
        out.push_str(&sha256::hex(content));
        out.push('\0');
    }
    out.into_bytes()
}

fn marker(manifest: &[u8]) -> Vec<u8> {
    format!("{}\n", sha256::hex(manifest)).into_bytes()
}

struct Manifest<'a> {
    entry: PathBuf,
    metadata: Option<(String, PathBuf)>,
    dep: Option<PathBuf>,
    payload: BTreeMap<PathBuf, &'a str>,
}

fn decode_manifest(manifest: &[u8]) -> Result<Manifest<'_>> {
    let mut entries = fields(manifest)?;
    let legacy = match entries.next() {
        Some("rpp-tree-v1") => true,
        Some(MANIFEST_MAGIC) => false,
        _ => return Err("invalid tree manifest version".into()),
    };
    let entry = PathBuf::from(entries.next().ok_or("missing tree entry")?);
    let metadata = if legacy {
        None
    } else {
        let edition = entries
            .next()
            .filter(|s| matches!(*s, "2021" | "2024"))
            .ok_or("missing or invalid tree edition")?;
        let map = PathBuf::from(entries.next().ok_or("missing tree source map")?);
        Some((edition.to_owned(), map))
    };
    let dep = entries.next().ok_or("missing depfile field")?;
    let dep = (!dep.is_empty()).then(|| PathBuf::from(dep));
    let mut payload = BTreeMap::new();
    while let Some(p) = entries.next() {
        let path = PathBuf::from(p);
        let hash = entries.next().ok_or("missing tree payload hash")?;
        if hash.len() != 64
            || !hash.bytes().all(|b| b.is_ascii_hexdigit())
            || payload.insert(path, hash).is_some()
        {
            return Err("invalid or duplicate manifest entry".into());
        }
    }
    for path in payload.keys().chain(&dep) {
        validate_relative(path)?;
        if control(path) || path == Path::new(".rpp") {
            return Err("reserved manifest path".into());
        }
    }
    if !payload.contains_key(&entry) || dep.as_ref().is_some_and(|p| payload.contains_key(p)) {
        return Err("invalid entry or depfile in manifest".into());
    }
    if metadata
        .as_ref()
        .is_some_and(|(_, map)| map == &entry || !payload.contains_key(map))
    {
        return Err("invalid source map in manifest".into());
    }
    Ok(Manifest {
        entry,
        metadata,
        dep,
        payload,
    })
}

/// Verify a generated tree before consuming it. Return the entry path and
/// snapshot identity so later compile/link integration can bind its record.
pub(crate) struct Verified {
    pub entry: PathBuf,
    pub identity: String,
    pub edition: String,
    pub map: PathBuf,
    pub payload: BTreeSet<PathBuf>,
}

pub(crate) fn verify(root: &Path) -> Result<Verified> {
    prospective_path(root)?;
    let (files, dirs) = tree_entries(root)?;
    let owned = ownership(bytes(root, Path::new(OWNED))?.as_deref())?;
    let manifest = bytes(root, Path::new(MANIFEST))?.ok_or("missing tree manifest")?;
    let complete = bytes(root, Path::new(COMPLETE))?.ok_or("missing tree completion marker")?;
    if complete != marker(&manifest) {
        return Err("tree completion marker does not match manifest".into());
    }
    let expected = decode_manifest(&manifest)?;
    let payload = expected.payload;
    for (path, hash) in &payload {
        let content = bytes(root, path)?
            .ok_or_else(|| format!("missing generated file: {}", path.display()))?;
        if sha256::hex(&content) != *hash {
            return Err(format!("altered generated file: {}", path.display()).into());
        }
    }
    let listed: BTreeSet<_> = payload.keys().cloned().chain(expected.dep).collect();
    let expected_dirs: BTreeSet<_> = owned
        .iter()
        .flat_map(|p| parents(p))
        .chain([Path::new(".rpp")])
        .collect();
    if owned != listed
        || files.iter().any(|p| {
            !owned.contains(p)
                && ![OWNED, COMPLETE, MANIFEST]
                    .iter()
                    .any(|s| p == Path::new(s))
        })
        || dirs.iter().any(|p| !expected_dirs.contains(p.as_path()))
    {
        return Err("unexpected generated tree contents".into());
    }
    let (edition, map) = expected
        .metadata
        .ok_or("legacy snapshot: regenerate the preprocessing tree")?;
    Ok(Verified {
        entry: expected.entry,
        identity: sha256::hex(&manifest),
        edition,
        map,
        payload: payload.into_keys().collect(),
    })
}

pub(super) fn publish(
    root: &Path,
    spec: TreeSpec<'_>,
    payload: BTreeMap<PathBuf, Vec<u8>>,
    dep: Option<(PathBuf, Vec<u8>)>,
    inputs: &[PathBuf],
) -> Result<()> {
    publish_with(root, spec, payload, dep, inputs, |_| {})
}

fn publish_with(
    root: &Path,
    spec: TreeSpec<'_>,
    mut payload: BTreeMap<PathBuf, Vec<u8>>,
    dep: Option<(PathBuf, Vec<u8>)>,
    inputs: &[PathBuf],
    checkpoint: impl Fn(&str),
) -> Result<()> {
    let dep_path = dep.as_ref().map(|(p, _)| p.clone());
    let metadata = manifest(&spec, &payload, dep_path.as_deref());
    if let Some((name, value)) = dep {
        payload.insert(name, value);
    }
    let desired: BTreeSet<_> = payload.keys().cloned().collect();
    for path in &desired {
        validate_relative(path)?;
        if control(path) || parents(path).any(|p| desired.contains(p)) {
            return Err(format!("conflicting generated paths: {}", path.display()).into());
        }
    }
    let (files, dirs) = tree_entries(root)?;
    let ledger = bytes(root, Path::new(OWNED))?;
    let existing_manifest = bytes(root, Path::new(MANIFEST))?;
    let old = if ledger.is_some() {
        ownership(ledger.as_deref())?
    } else if let Some(manifest) = &existing_manifest {
        // The last published manifest retains its owned paths even if the
        // ledger was deleted. It includes the optional disposable depfile.
        let old = decode_manifest(manifest)?;
        old.payload.into_keys().chain(old.dep).collect()
    } else {
        BTreeSet::new()
    };
    let temps: BTreeSet<_> = old
        .iter()
        .map(|p| temporary(p))
        .chain([
            temporary(Path::new(OWNED)),
            temporary(Path::new(MANIFEST)),
            temporary(Path::new(COMPLETE)),
        ])
        .collect();
    let previous_dirs: BTreeSet<_> = old
        .iter()
        .flat_map(|p| parents(p))
        .chain([Path::new(".rpp")])
        .collect();
    for path in &files {
        let known = if ledger.is_some() || existing_manifest.is_some() {
            old.contains(path)
                || [OWNED, MANIFEST, COMPLETE]
                    .iter()
                    .any(|p| path == Path::new(p))
                || temps.contains(path)
        } else {
            path == &temporary(Path::new(OWNED))
        };
        if !known
            || inputs
                .iter()
                .any(|p| artifacts::aliases(p, &root.join(path)))
        {
            return Err(format!(
                "unowned or input-aliased generated file: {}",
                path.display()
            )
            .into());
        }
    }
    for path in &dirs {
        if !previous_dirs.contains(path.as_path()) {
            return Err(format!("unowned generated directory: {}", path.display()).into());
        }
    }
    for path in &desired {
        if dirs.contains(path) || parents(path).any(|p| files.contains(p)) {
            return Err(format!(
                "output path changes file/directory shape: {}; use a fresh output directory",
                path.display()
            )
            .into());
        }
    }
    let changed = payload
        .into_iter()
        .filter_map(|(path, contents)| match bytes(root, &path) {
            Ok(previous) if previous.as_deref() == Some(&contents) => None,
            Ok(_) => Some(Ok((path, contents))),
            Err(error) => Some(Err(error)),
        })
        .collect::<Result<Vec<_>>>()?;
    let completion = marker(&metadata);
    if changed
        .iter()
        .all(|(path, _)| Some(path) == dep_path.as_ref())
        && old == desired
        && existing_manifest.as_deref() == Some(&metadata)
        && bytes(root, Path::new(COMPLETE))?.as_deref() == Some(&completion)
        && !files.iter().any(|p| temps.contains(p))
        && verify(root).is_ok()
    {
        // The depfile is disposable. Regenerate it independently and keep the
        // completion marker stable when the actual snapshot is unchanged.
        for (path, contents) in changed {
            replace(root, &path, &contents)?;
        }
        return Ok(());
    }

    // The ledger is independent of the manifest and survives invalidation.
    // Keeping old and new paths in it makes both stale-file cleanup and a
    // retry after interruption safe.
    let mut pending = old.clone();
    pending.extend(desired.iter().cloned());
    for path in files.intersection(&temps) {
        remove(root, path)?;
    }
    // Invalidate before the ownership record can describe a different tree.
    remove(root, Path::new(COMPLETE))?;
    checkpoint("invalidated");
    replace(root, Path::new(OWNED), &encode_owned(&pending))?;
    for (path, data) in changed {
        replace_with(root, &path, &data, || checkpoint("staged"))?;
        checkpoint("payload");
    }
    for path in old.difference(&desired) {
        remove(root, path)?;
    }
    let desired_dirs: BTreeSet<_> = desired
        .iter()
        .flat_map(|p| parents(p))
        .chain([Path::new(".rpp")])
        .collect();
    for dir in dirs
        .iter()
        .rev()
        .filter(|p| !desired_dirs.contains(p.as_path()))
    {
        fs::remove_dir(root.join(dir))?;
    }
    if existing_manifest.as_deref() != Some(&metadata) {
        replace(root, Path::new(MANIFEST), &metadata)?;
    }
    if pending != desired {
        replace(root, Path::new(OWNED), &encode_owned(&desired))?;
    }
    checkpoint("manifest");
    replace(root, Path::new(COMPLETE), &completion)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn write_fixture(root: &Path, new: bool, stop: impl Fn(&str)) -> Result<()> {
        let mut files = BTreeMap::from([(
            PathBuf::from("entry.rs"),
            if new {
                b"changed".to_vec()
            } else {
                b"initial".to_vec()
            },
        )]);
        let side = if new { "new/data.bin" } else { "old/data.bin" };
        files.insert(side.into(), vec![0, 255]);
        files.insert("map.json".into(), b"map".to_vec());
        let spec = TreeSpec {
            entry: Path::new("entry.rs"),
            map: Path::new("map.json"),
            edition: "2024",
        };
        publish_with(root, spec, files, None, &[], stop)
    }

    #[test]
    fn interrupted_snapshot_recovers() {
        const CHILD: &str = "RPP_TEST_SNAPSHOT_CHILD";
        const PHASE: &str = "RPP_TEST_SNAPSHOT_PHASE";
        if let Some(root) = std::env::var_os(CHILD) {
            let phase = std::env::var(PHASE).unwrap();
            write_fixture(Path::new(&root), true, |checkpoint| {
                if checkpoint == phase {
                    // Exit without unwinding, as with an interrupted writer.
                    std::process::exit(71);
                }
            })
            .unwrap();
            panic!("checkpoint did not interrupt publication");
        }
        let root = std::env::temp_dir().join(format!(
            "rpp-m4-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&root).unwrap();
        for first in [true, false] {
            for phase in ["invalidated", "staged", "payload", "manifest"] {
                let folder = root.join(format!("{first}-{phase}"));
                if !first {
                    write_fixture(&folder, false, |_| {}).unwrap();
                    assert!(verify(&folder).is_ok());
                }
                let result = Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "preprocess_cli::snapshot::tests::interrupted_snapshot_recovers",
                    ])
                    .env(CHILD, &folder)
                    .env(PHASE, phase)
                    .output()
                    .unwrap();
                assert_eq!(result.status.code(), Some(71), "{result:?}");
                assert!(verify(&folder).is_err(), "{phase}");
                write_fixture(&folder, true, |_| {}).unwrap();
                assert!(verify(&folder).is_ok(), "{phase}");
                assert!(!folder.join("old").exists(), "{phase}");
                fs::write(folder.join("entry.rs"), "bad").unwrap();
                assert!(verify(&folder).is_err());
                write_fixture(&folder, true, |_| {}).unwrap();
                fs::remove_file(folder.join("new/data.bin")).unwrap();
                assert!(verify(&folder).is_err());
                write_fixture(&folder, true, |_| {}).unwrap();
                assert!(verify(&folder).is_ok());
                let unexpected = folder.join("unexpected.rs");
                fs::write(&unexpected, "unowned").unwrap();
                assert!(verify(&folder).is_err());
                assert!(write_fixture(&folder, true, |_| {}).is_err());
                assert_eq!(fs::read(&unexpected).unwrap(), b"unowned");
            }
        }
        fs::remove_dir_all(root).unwrap();
    }
}
