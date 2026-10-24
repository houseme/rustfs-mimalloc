use crate::{
    api,
    util::{self, Args},
};
use anyhow::{Context, Result, bail, ensure};
use regex::Regex;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    time::Duration,
};

const NAMES: [&str; 3] = [
    "rustfs-mimalloc-sys",
    "rustfs-mimalloc",
    "rustfs-mimalloc-profiling",
];

fn version(value: &str) -> Result<()> {
    ensure!(
        Regex::new(r"\A[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?\z")?.is_match(value),
        "expected a semantic version"
    );
    Ok(())
}

fn resolved(event: &str, requested: &str, reference: &str) -> Result<String> {
    let value = match event {
        "push" => reference
            .strip_prefix('v')
            .context("expected a v<VERSION> tag")?,
        "workflow_dispatch" => requested,
        _ => bail!("unsupported release event: {event}"),
    };
    version(value)?;
    Ok(value.into())
}

pub fn resolve(args: &mut Args) -> Result<()> {
    args.finish()?;
    let value = resolved(
        &std::env::var("RELEASE_EVENT")?,
        &std::env::var("REQUESTED_VERSION").unwrap_or_default(),
        &std::env::var("RELEASE_REF")?,
    )?;
    let dry = std::env::var("RELEASE_DRY_RUN")?
        .parse::<bool>()
        .context("RELEASE_DRY_RUN must be true or false")?;
    let output = format!("version={value}\ntag=v{value}\nis_dry_run={dry}\n");
    if let Some(path) = std::env::var_os("GITHUB_OUTPUT") {
        OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)?
            .write_all(output.as_bytes())?;
    }
    println!("Validated release version: {value}\n{output}");
    Ok(())
}

fn section(changelog: &str, version: &str) -> Result<Option<String>> {
    let prefix = format!("## [{version}]");
    let mut body = Vec::new();
    let mut found = false;
    let mut inside = false;
    let reference = Regex::new(r"^\[[^]]+\]:\s")?;
    for line in changelog.lines() {
        if line.starts_with(&prefix)
            && line[prefix.len()..]
                .chars()
                .next()
                .is_none_or(char::is_whitespace)
        {
            ensure!(!found, "duplicate changelog section for {version}");
            found = true;
            inside = true;
        } else if line.starts_with("## ") || reference.is_match(line) {
            inside = false;
        } else if inside {
            body.push(line);
        }
    }
    Ok(found.then(|| body.join("\n").trim().to_owned()))
}

fn validate_metadata(metadata: &Value, expected: &str) -> Result<()> {
    let packages = metadata["packages"]
        .as_array()
        .context("missing packages")?;
    for name in NAMES {
        let package = packages
            .iter()
            .find(|p| p["name"] == name)
            .with_context(|| format!("missing package: {name}"))?;
        ensure!(
            package["version"] == expected && package["edition"] == "2024",
            "{name}: expected version {expected} and edition 2024"
        );
        for dependency in package["dependencies"]
            .as_array()
            .context("missing dependencies")?
        {
            if dependency["name"]
                .as_str()
                .is_some_and(|name| NAMES.contains(&name))
            {
                ensure!(
                    dependency["req"] == format!("^{expected}"),
                    "{name}: internal dependency version mismatch"
                );
            }
        }
    }
    Ok(())
}

pub fn check(args: &mut Args) -> Result<()> {
    let expected = args.positional()?;
    args.finish()?;
    version(&expected)?;
    validate_metadata(&util::metadata(&[], None, false)?, &expected)?;
    ensure!(
        section(
            &fs::read_to_string(util::root().join("CHANGELOG.md"))?,
            &expected
        )?
        .is_some_and(|s| !s.is_empty()),
        "missing changelog entries for {expected}"
    );
    println!("Version, edition, internal requirements and changelog OK: {expected}");
    Ok(())
}

fn previous_tag(tag: &str) -> String {
    util::git([
        "describe",
        "--tags",
        "--match",
        "v[0-9]*",
        "--abbrev=0",
        &format!("{tag}^"),
    ])
    .or_else(|_| {
        util::git([
            "describe",
            "--tags",
            "--match",
            "v[0-9]*",
            "--abbrev=0",
            "HEAD^",
        ])
    })
    .unwrap_or_default()
}

fn notes_text(value: &str, native: u64, body: &str, previous: &str, repository: &str) -> String {
    let fence = char::from(96).to_string().repeat(3);
    let mut notes = format!(
        "## rustfs-mimalloc v{value}\n\nmimalloc V3 (v{}.{}.{}) bindings for Rust.\n\n{body}\n\n### Installation\n\n\
        {fence}toml\n[dependencies]\nrustfs-mimalloc = \"{value}\"\nrustfs-mimalloc-profiling = {{ version = \"{value}\", features = [\"rustfs-allocator\"] }}\n{fence}\n",
        native / 10000,
        native / 100 % 100,
        native % 100
    );
    if !previous.is_empty() {
        notes += &format!(
            "\n**Full Changelog**: [{previous}...v{value}](https://github.com/{repository}/compare/{previous}...v{value})\n"
        );
    }
    notes
}

pub fn notes(args: &mut Args) -> Result<()> {
    let value = args.positional()?;
    let destination = util::path(args.value("--output", "target/release-notes.md")?);
    args.finish()?;
    version(&value)?;
    let tag = format!("v{value}");
    let previous = previous_tag(&tag);
    let body = section(
        &fs::read_to_string(util::root().join("CHANGELOG.md"))?,
        &value,
    )?
    .unwrap_or_default();
    let body = if !body.is_empty() {
        body
    } else if !previous.is_empty() {
        let commits = util::git([
            "log",
            "--pretty=format:- %s (%h)",
            &format!("{previous}..HEAD"),
            "--no-merges",
        ])?;
        format!(
            "### Changes\n\n{}",
            if commits.is_empty() {
                "- No user-facing changes recorded."
            } else {
                &commits
            }
        )
    } else {
        ensure!(
            value == "0.1.0",
            "refusing to emit Initial release for a non-initial version"
        );
        "### Changes\n\nInitial release.".into()
    };
    let header = api::headers()?;
    let native = Regex::new(r"#define\s+MI_MALLOC_VERSION\s+(\d+)")?
        .captures(&header)
        .context("missing MI_MALLOC_VERSION")?[1]
        .parse()?;
    let repository =
        std::env::var("REPOSITORY").unwrap_or_else(|_| "houseme/rustfs-mimalloc".into());
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        &destination,
        notes_text(&value, native, &body, &previous, &repository),
    )?;
    println!(
        "Release notes generated: {}; previous tag: {previous}",
        destination.display()
    );
    Ok(())
}

pub fn checksum(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    Sha256::digest(bytes)
        .iter()
        .flat_map(|byte| {
            [
                HEX[(byte >> 4) as usize] as char,
                HEX[(byte & 15) as usize] as char,
            ]
        })
        .collect()
}

fn verify_checksum(bytes: &[u8], index: &str, expected: Option<&str>) -> Result<()> {
    let actual = checksum(bytes);
    ensure!(
        actual == index && expected.is_none_or(|expected| actual == expected),
        "published archive checksum does not match"
    );
    Ok(())
}

fn index_release(index: &str, version: &str) -> Result<Option<Value>> {
    for line in index.lines() {
        let record: Value =
            serde_json::from_str(line).context("parse official sparse index record")?;
        if record["vers"] == version && record["yanked"] == false {
            return Ok(Some(record));
        }
    }
    Ok(None)
}

fn download(agent: &ureq::Agent, url: &str) -> Result<Vec<u8>> {
    Ok(agent
        .get(url)
        .header("User-Agent", "rustfs-mimalloc-release-verification")
        .header("Cache-Control", "no-cache")
        .call()?
        .body_mut()
        .with_config()
        .limit(64 * 1024 * 1024)
        .read_to_vec()?)
}

pub fn registry(args: &mut Args) -> Result<()> {
    let name = args.positional()?;
    let value = args.positional()?;
    let archive = args.option("--archive")?.map(util::path);
    args.finish()?;
    ensure!(
        NAMES.contains(&name.as_str()),
        "unknown release package: {name}"
    );
    version(&value)?;
    let expected = archive
        .as_deref()
        .map(fs::read)
        .transpose()?
        .map(|data| checksum(&data));
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .into();
    let mut last_error = None;
    for attempt in 0..40 {
        match download(&agent, &format!("https://index.crates.io/ru/st/{name}")) {
            Ok(data) => {
                let index = std::str::from_utf8(&data)?;
                if let Some(record) = index_release(index, &value)? {
                    match download(
                        &agent,
                        &format!("https://static.crates.io/crates/{name}/{name}-{value}.crate"),
                    ) {
                        Ok(data) => {
                            verify_checksum(
                                &data,
                                record["cksum"]
                                    .as_str()
                                    .context("missing registry checksum")?,
                                expected.as_deref(),
                            )?;
                            println!(
                                "Verified {name} {value}: official index and archive checksum match"
                            );
                            return Ok(());
                        }
                        Err(error) => last_error = Some(error),
                    }
                }
            }
            Err(error) => last_error = Some(error),
        }
        if attempt < 39 {
            std::thread::sleep(Duration::from_secs(5));
        }
    }
    match last_error {
        Some(error) => Err(error.context("official registry verification failed")),
        None => bail!("requested version is not visible in the official registry"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn release_events_and_versions_reject_unsafe_input() {
        assert_eq!(resolved("push", "ignored", "v0.7.2").unwrap(), "0.7.2");
        assert_eq!(
            resolved("workflow_dispatch", "0.7.2-rc.1", "main").unwrap(),
            "0.7.2-rc.1"
        );
        for v in ["../0.7.2", "0.7.2\n", "0.7.2; echo secret", "main"] {
            assert!(version(v).is_err());
        }
        assert!(resolved("push", "0.7.2", "main").is_err());
        assert!(resolved("unknown", "0.7.2", "main").is_err());
    }
    #[test]
    fn changelog_and_notes_preserve_release_boundaries() -> Result<()> {
        let notes = "## [0.7.2] - date\n\n### Added\n\n- Example\n\n## [0.7.1]\n\n- Previous";
        assert_eq!(section(notes, "0.7.2")?.unwrap(), "### Added\n\n- Example");
        assert!(section(&(notes.to_owned() + "\n## [0.7.2]\n- Duplicate"), "0.7.2").is_err());
        assert!(section(notes, "0.9.0")?.is_none());
        let result = notes_text(
            "0.7.2",
            30600,
            "- Example",
            "v0.7.1",
            "houseme/rustfs-mimalloc",
        );
        assert!(
            result.contains("mimalloc V3 (v3.6.0)") && result.contains("compare/v0.7.1...v0.7.2")
        );
        assert!(!result.contains("Initial release."));
        Ok(())
    }
    #[test]
    fn official_index_and_checksum_controls_reject_yanked_or_different_archives() -> Result<()> {
        assert_eq!(
            checksum(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let index = "{\"vers\":\"0.7.2\",\"yanked\":true}\n{\"vers\":\"0.7.1\",\"yanked\":false}";
        assert!(index_release(index, "0.7.2")?.is_none());
        assert!(index_release(index, "0.7.1")?.is_some());
        assert!(index_release("invalid", "0.7.1").is_err());
        assert!(verify_checksum(b"abc", &checksum(b"abc"), Some(&checksum(b"other"))).is_err());
        assert!(verify_checksum(b"abc", &checksum(b"other"), None).is_err());
        assert!(verify_checksum(b"abc", &checksum(b"abc"), Some(&checksum(b"abc"))).is_ok());
        Ok(())
    }
    #[test]
    fn metadata_checks_all_three_versions_and_internal_requirements() -> Result<()> {
        let mut metadata = util::metadata(&[], None, false)?;
        let expected = metadata["packages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == NAMES[0])
            .unwrap()["version"]
            .as_str()
            .unwrap()
            .to_owned();
        validate_metadata(&metadata, &expected)?;
        metadata["packages"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|p| p["name"] == NAMES[0])
            .unwrap()["version"] = "0.0.0".into();
        assert!(validate_metadata(&metadata, &expected).is_err());
        Ok(())
    }
}
