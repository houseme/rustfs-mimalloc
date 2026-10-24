use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
};

pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned()
}

pub fn path(value: impl AsRef<Path>) -> PathBuf {
    let path = value.as_ref();
    if path.is_absolute() {
        path.to_owned()
    } else {
        root().join(path)
    }
}

pub fn read_text(path: &Path) -> Result<String> {
    // Git may check out CRLF on Windows. Source mutation controls use LF
    // patterns, so normalize text before parsing and applying those controls.
    Ok(fs::read_to_string(path)
        .with_context(|| format!("read {}", path.display()))?
        .replace("\r\n", "\n")
        .replace('\r', "\n"))
}

pub struct Args(pub Vec<String>);

impl Args {
    pub fn flag(&mut self, name: &str) -> bool {
        if let Some(index) = self.0.iter().position(|s| s == name) {
            self.0.remove(index);
            true
        } else {
            false
        }
    }
    pub fn option(&mut self, name: &str) -> Result<Option<String>> {
        let Some(index) = self.0.iter().position(|s| s == name) else {
            return Ok(None);
        };
        self.0.remove(index);
        anyhow::ensure!(
            index < self.0.len() && !self.0[index].starts_with("--"),
            "missing value for {name}"
        );
        Ok(Some(self.0.remove(index)))
    }
    pub fn value(&mut self, name: &str, default: &str) -> Result<String> {
        Ok(self.option(name)?.unwrap_or_else(|| default.into()))
    }
    pub fn required(&mut self, name: &str) -> Result<String> {
        self.option(name)?
            .with_context(|| format!("missing {name}"))
    }
    pub fn positional(&mut self) -> Result<String> {
        anyhow::ensure!(
            !self.0.is_empty() && !self.0[0].starts_with("--"),
            "missing positional argument"
        );
        Ok(self.0.remove(0))
    }
    pub fn finish(&self) -> Result<()> {
        anyhow::ensure!(self.0.is_empty(), "unexpected arguments: {:?}", self.0);
        Ok(())
    }
}

pub fn output(command: &mut Command) -> Result<Output> {
    command
        .output()
        .with_context(|| format!("execute {command:?}"))
}

pub fn capture(command: &mut Command) -> Result<String> {
    let result = output(command)?;
    if !result.status.success() {
        bail!(
            "{command:?} failed ({}):\n{}\n{}",
            result.status,
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
    String::from_utf8(result.stdout).context("command output is not UTF-8")
}

pub fn run(command: &mut Command) -> Result<()> {
    let status = command
        .status()
        .with_context(|| format!("execute {command:?}"))?;
    anyhow::ensure!(status.success(), "{command:?} failed ({status})");
    Ok(())
}

pub fn cargo(config: &[String], cwd: &Path) -> Command {
    let mut command = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    command.args(config).current_dir(cwd);
    if let Some(target) = std::env::var_os("CARGO_TARGET_DIR") {
        command.env("CARGO_TARGET_DIR", path(target));
    }
    command
}

pub fn metadata(config: &[String], manifest: Option<&Path>, features: bool) -> Result<Value> {
    let mut command = cargo(config, &root());
    command.args(["metadata", "--format-version", "1"]);
    if let Some(manifest) = manifest {
        command.arg("--manifest-path").arg(manifest);
        command.arg(if features {
            "--all-features"
        } else {
            "--no-default-features"
        });
    } else {
        command.args(["--locked", "--no-deps"]);
    }
    serde_json::from_str(&capture(&mut command)?).context("parse Cargo metadata")
}

pub fn local_config(enabled: bool) -> Vec<String> {
    if !enabled {
        return Vec::new();
    }
    ["rustfs-mimalloc", "rustfs-mimalloc-sys"]
        .into_iter()
        .flat_map(|name| {
            vec![
                "--config".into(),
                format!(
                    "patch.crates-io.{name}.path={}",
                    quoted_path(&root().join(name))
                ),
            ]
        })
        .collect()
}

pub fn quoted_path(path: &Path) -> String {
    // JSON strings are valid TOML basic strings and Rust string literals.
    serde_json::to_string(&path.to_string_lossy().replace('\\', "/")).unwrap()
}

pub fn write_json(path: &Path, value: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, format!("{}\n", serde_json::to_string_pretty(value)?))?;
    Ok(())
}

pub fn git<I, S>(args: I) -> Result<String>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Ok(capture(Command::new("git").current_dir(root()).args(args))?
        .trim()
        .to_owned())
}

pub fn fresh_directory(path: &Path, allow_empty: bool) -> Result<()> {
    if path.exists() {
        anyhow::ensure!(
            allow_empty && path.read_dir()?.next().is_none(),
            "choose a fresh output directory: {}",
            path.display()
        );
    }
    fs::create_dir_all(path)?;
    Ok(())
}
