use crate::util::{self, Args};
use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Component, Path},
};

pub fn reachable(metadata: &Value) -> Result<BTreeSet<String>> {
    let resolve = &metadata["resolve"];
    let nodes: BTreeMap<_, _> = resolve["nodes"]
        .as_array()
        .context("missing Cargo resolve nodes")?
        .iter()
        .map(|node| (node["id"].as_str().unwrap_or_default(), node))
        .collect();
    let mut pending = vec![
        resolve["root"]
            .as_str()
            .context("missing consumer resolve root")?,
    ];
    let mut seen = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        for dependency in nodes
            .get(id)
            .with_context(|| format!("missing resolve node: {id}"))?["deps"]
            .as_array()
            .context("missing deps")?
        {
            pending.push(
                dependency["pkg"]
                    .as_str()
                    .context("missing dependency package ID")?,
            );
        }
    }
    Ok(metadata["packages"]
        .as_array()
        .context("missing packages")?
        .iter()
        .filter(|p| p["id"].as_str().is_some_and(|id| seen.contains(id)))
        .map(|p| p["name"].as_str().unwrap_or_default().to_owned())
        .collect())
}

fn validate_graph(metadata: &Value, provider: &str) -> Result<BTreeSet<String>> {
    let selected = reachable(metadata)?;
    let expected: BTreeSet<_> = if provider == "rustfs" {
        ["rustfs-mimalloc", "rustfs-mimalloc-sys"]
    } else {
        ["mimalloc", "libmimalloc-sys"]
    }
    .into_iter()
    .map(str::to_owned)
    .collect();
    let all: BTreeSet<_> = [
        "rustfs-mimalloc",
        "rustfs-mimalloc-sys",
        "mimalloc",
        "libmimalloc-sys",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();
    ensure!(
        expected.is_subset(&selected)
            && (all
                .difference(&expected)
                .all(|name| !selected.contains(name))),
        "{provider}: unexpected allocator graph: {:?}",
        selected.intersection(&all).collect::<Vec<_>>()
    );
    Ok(selected)
}

fn requirement<'a>(dependencies: &'a toml::Value, name: &str) -> Result<&'a str> {
    let value = &dependencies[name];
    value
        .as_str()
        .or_else(|| value.get("version").and_then(toml::Value::as_str))
        .with_context(|| format!("missing workspace requirement: {name}"))
}

pub fn run(args: &mut Args) -> Result<()> {
    let provider = args.value("--provider", "all")?;
    let local = args.flag("--local-dependencies");
    let release = args.flag("--release");
    let profiling = util::path(args.value("--profiling-dir", "rustfs-mimalloc-profiling")?);
    let output = util::path(args.value("--output", "target/provider-tests")?);
    args.finish()?;
    ensure!(
        ["all", "rustfs", "upstream"].contains(&provider.as_str()),
        "unknown provider: {provider}"
    );
    test(&provider, local, release, &profiling, &output)
}

pub fn test(
    provider: &str,
    local: bool,
    release: bool,
    profiling: &Path,
    output: &Path,
) -> Result<()> {
    let root = util::root();
    let workspace: toml::Value = toml::from_str(&fs::read_to_string(root.join("Cargo.toml"))?)?;
    let dependencies = &workspace["workspace"]["dependencies"];
    let config = util::local_config(local);
    let mut evidence = serde_json::Map::new();
    for (key, package) in [("rustfs", "rustfs-mimalloc"), ("upstream", "mimalloc")] {
        if provider != "all" && provider != key {
            continue;
        }
        let consumer = output.join(key);
        fs::create_dir_all(&consumer)?;
        let mut native = format!("version = {:?}", requirement(dependencies, package)?);
        if key == "rustfs" && local {
            native += &format!(", path = {}", util::quoted_path(&root.join(package)));
        }
        let manifest = consumer.join("Cargo.toml");
        fs::write(
            &manifest,
            format!(
                "[package]\nname = \"profiling-provider-{key}\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[workspace]\n\n[dependencies]\n\
             rustfs-mimalloc-profiling = {{ path = {}, default-features = false }}\n{package} = {{ {native} }}\nprost = {:?}\nlibflate = {:?}\n\n\
             [[test]]\nname = \"profiling\"\npath = \"profiling.rs\"\n\n[[test]]\nname = \"mimalloc_release\"\npath = \"mimalloc_release.rs\"\n",
                util::quoted_path(profiling),
                requirement(dependencies, "prost")?,
                requirement(dependencies, "libflate")?
            ),
        )?;
        for suite in ["profiling", "mimalloc_release"] {
            fs::write(
                consumer.join(format!("{suite}.rs")),
                format!(
                    "use rustfs_mimalloc_profiling::SamplingAllocator;\n\n#[global_allocator]\nstatic ALLOC: SamplingAllocator<{}::MiMalloc> =\n    SamplingAllocator::with_allocator({}::MiMalloc);\n\n#[path = {}]\nmod tests;\n",
                    package.replace('-', "_"),
                    package.replace('-', "_"),
                    util::quoted_path(&profiling.join(format!("tests/support/{suite}.rs")))
                ),
            )?;
        }
        let lock = consumer.join("Cargo.lock");
        if !lock.exists() {
            fs::copy(root.join("Cargo.lock"), lock)?;
        }
        let target = std::env::var_os("CARGO_TARGET_DIR")
            .map(util::path)
            .unwrap_or_else(|| root.join("target/provider-build"));
        let metadata: Value = serde_json::from_str(&util::capture(
            util::cargo(&config, &root)
                .args(["metadata", "--format-version", "1", "--manifest-path"])
                .arg(&manifest)
                .env("CARGO_TARGET_DIR", &target),
        )?)?;
        evidence.insert(key.into(), json!(validate_graph(&metadata, key)?));
        let mut command = util::cargo(&config, &root);
        command
            .args(["test", "--locked", "--manifest-path"])
            .arg(&manifest)
            .env("CARGO_TARGET_DIR", &target);
        if release {
            command.args(["--release", "--test", "mimalloc_release"]);
        }
        command.args(["--", "--test-threads=1"]);
        util::run(&mut command)?;
        println!("{key}: shared contracts PASS; allocator graph isolated");
        util::write_json(
            &output.join("dependency-graphs.json"),
            &Value::Object(evidence.clone()),
        )?;
    }
    Ok(())
}

pub fn extract(reader: impl Read, destination: &Path, prefix: &str) -> Result<()> {
    for entry in tar::Archive::new(reader).entries()? {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        ensure!(
            entry.header().entry_type().is_file() || entry.header().entry_type().is_dir(),
            "unexpected package archive entry type"
        );
        ensure!(
            path.components()
                .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
                && path
                    .components()
                    .next()
                    .is_some_and(|c| c.as_os_str() == prefix),
            "unexpected package archive path: {}",
            path.display()
        );
        ensure!(
            entry.unpack_in(destination)?,
            "archive path escaped destination"
        );
    }
    Ok(())
}

pub fn verify_package(args: &mut Args) -> Result<()> {
    let local = args.flag("--local-dependencies");
    let dirty = args.flag("--allow-dirty");
    args.finish()?;
    let root = util::root();
    let config = util::local_config(local);
    let workspace = util::metadata(&config, None, false)?;
    let package = workspace["packages"]
        .as_array()
        .context("missing packages")?
        .iter()
        .find(|p| p["name"] == "rustfs-mimalloc-profiling")
        .context("missing profiling package")?;
    let version = package["version"].as_str().context("missing version")?;
    let mut command = util::cargo(&config, &root);
    command.args(["package", "--locked", "-p", "rustfs-mimalloc-profiling"]);
    if dirty {
        command.arg("--allow-dirty");
    }
    util::run(&mut command)?;
    let target = Path::new(
        workspace["target_directory"]
            .as_str()
            .context("missing target_directory")?,
    );
    let prefix = format!("rustfs-mimalloc-profiling-{version}");
    let archive = fs::File::open(target.join(format!("package/{prefix}.crate")))?;
    let temporary = tempfile::tempdir()?;
    extract(
        flate2::read::GzDecoder::new(archive),
        temporary.path(),
        &prefix,
    )?;
    let extracted = temporary.path().join(&prefix);
    let manifest = extracted.join("Cargo.toml");
    let neutral = util::metadata(&config, Some(&manifest), false)?;
    let graph = reachable(&neutral)?;
    for forbidden in [
        "rustfs-mimalloc",
        "rustfs-mimalloc-sys",
        "mimalloc",
        "libmimalloc-sys",
        "ruzstd",
    ] {
        ensure!(
            !graph.contains(forbidden),
            "default profiling archive selected {forbidden}"
        );
    }
    let resolved = util::metadata(&config, Some(&manifest), true)?;
    let allocators: Vec<_> = resolved["packages"]
        .as_array()
        .context("missing packages")?
        .iter()
        .filter(|p| p["name"] == "rustfs-mimalloc" || p["name"] == "rustfs-mimalloc-sys")
        .collect();
    ensure!(
        allocators.len() == 2 && allocators.iter().all(|p| p["version"] == version),
        "archive must use matching allocator/sys versions"
    );
    ensure!(
        local
            || allocators.iter().all(|p| p["source"]
                .as_str()
                .is_some_and(|s| s.starts_with("registry+"))),
        "archive verification must use registry allocator dependencies"
    );
    println!(
        "Allocator versions match {version} ({})",
        if local { "local patches" } else { "registry" }
    );
    for flags in [
        vec!["--no-default-features"],
        vec!["--all-features"],
        vec!["--release", "--test", "mimalloc_release"],
    ] {
        util::run(
            util::cargo(&config, &extracted)
                .args(["test", "--locked"])
                .args(flags)
                .arg("--manifest-path")
                .arg(&manifest)
                .args(["--", "--test-threads=1"]),
        )?;
    }
    test(
        "all",
        local,
        false,
        &extracted,
        &target.join("package-provider-tests"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reachable_graph_ignores_unused_packages_but_rejects_two_providers() -> Result<()> {
        let mut metadata = json!({"resolve":{"root":"root","nodes":[{"id":"root","deps":[{"pkg":"r"},{"pkg":"s"}]},{"id":"r","deps":[]},{"id":"s","deps":[]}]},
            "packages":[{"id":"root","name":"test"},{"id":"r","name":"rustfs-mimalloc"},{"id":"s","name":"rustfs-mimalloc-sys"},{"id":"u","name":"mimalloc"}]});
        assert!(validate_graph(&metadata, "rustfs").is_ok());
        metadata["resolve"]["nodes"][0]["deps"]
            .as_array_mut()
            .unwrap()
            .push(json!({"pkg":"u"}));
        metadata["resolve"]["nodes"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":"u","deps":[]}));
        assert!(validate_graph(&metadata, "rustfs").is_err());
        Ok(())
    }
    fn bundle(name: &str, kind: tar::EntryType) -> Result<Vec<u8>> {
        let mut header = tar::Header::new_gnu();
        header.set_size(0);
        header.set_mode(0o644);
        header.set_entry_type(kind);
        header.as_mut_bytes()[..name.len()].copy_from_slice(name.as_bytes());
        header.set_cksum();
        let mut bundle = tar::Builder::new(Vec::new());
        bundle.append(&header, &[][..])?;
        Ok(bundle.into_inner()?)
    }
    #[test]
    fn archive_paths_and_links_cannot_escape_temporary_directory() -> Result<()> {
        let temp = tempfile::tempdir()?;
        for (name, kind) in [
            ("../escape", tar::EntryType::Regular),
            ("/absolute", tar::EntryType::Regular),
            ("pkg/link", tar::EntryType::Symlink),
        ] {
            assert!(extract(bundle(name, kind)?.as_slice(), temp.path(), "pkg").is_err());
        }
        extract(
            bundle("pkg/file", tar::EntryType::Regular)?.as_slice(),
            temp.path(),
            "pkg",
        )?;
        assert!(temp.path().join("pkg/file").is_file());
        Ok(())
    }
}
