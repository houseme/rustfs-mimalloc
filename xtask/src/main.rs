mod abi;
mod api;
mod bench;
mod providers;
mod release;
mod util;

use anyhow::{Result, bail};
use util::Args;

fn main() -> Result<()> {
    let mut arguments = std::env::args().skip(1);
    let command = arguments.next().unwrap_or_else(|| "help".into());
    let mut args = Args(arguments.collect());
    if args.0.iter().any(|arg| arg == "--help" || arg == "-h") {
        return help();
    }
    match command.as_str() {
        "check-api" => api::run(&mut args),
        "check-abi" => abi::run(&mut args),
        "test-abi" => abi::controls(&mut args),
        "test-providers" => providers::run(&mut args),
        "verify-package" => providers::verify_package(&mut args),
        "verify-registry" => release::registry(&mut args),
        "release-resolve" => release::resolve(&mut args),
        "release-check" => release::check(&mut args),
        "release-notes" => release::notes(&mut args),
        "bench-heap-abba" => bench::criterion(&mut args, false),
        "bench-profile-abba" => bench::criterion(&mut args, true),
        "bench-profiling-abba" => bench::profiling(&mut args),
        "help" | "--help" | "-h" => help(),
        _ => bail!("unknown command: {command}; run cargo xtask help"),
    }
}

fn help() -> Result<()> {
    println!(
        "cargo xtask <command> [options]\n\
                check-api [--json PATH]\ncheck-abi [--cc COMMAND]\ntest-abi\n\
                test-providers [--provider all|rustfs|upstream] [--local-dependencies] [--release] [--profiling-dir PATH] [--output PATH]\n\
                verify-package [--local-dependencies] [--allow-dirty]\n\
                verify-registry PACKAGE VERSION [--archive PATH]\n\
                release-resolve\nrelease-check VERSION\nrelease-notes VERSION [--output PATH]\n\
                bench-heap-abba | bench-profile-abba [--offline] [--output PATH] [--sample-size N] [--warm-up-time S] [--measurement-time S] [--drift-limit FRACTION]\n\
                bench-profiling-abba --baseline PATH --candidate PATH --output PATH [--duration-ms N] [--case NAME]"
    );
    Ok(())
}

#[cfg(test)]
mod repository_policy {
    use anyhow::Result;
    use std::{fs, path::Path};

    fn inspect(directory: &Path) -> Result<()> {
        for entry in fs::read_dir(directory)? {
            let entry = entry?;
            let path = entry.path();
            if entry.file_type()?.is_dir() {
                inspect(&path)?;
            } else {
                assert!(
                    !path
                        .extension()
                        .is_some_and(|ext| ext == "py" || ext == "pyc" || ext == "pyi"),
                    "maintenance code must use Rust: {}",
                    path.display()
                );
            }
        }
        Ok(())
    }

    #[test]
    fn maintenance_code_and_workflows_need_no_interpreter() -> Result<()> {
        let root = crate::util::root();
        for directory in ["scripts", ".github", "xtask"] {
            inspect(&root.join(directory))?;
        }
        for workflow in ["ci.yml", "release.yml"] {
            let source = fs::read_to_string(root.join(".github/workflows").join(workflow))?;
            for forbidden in ["setup-python@", "python-version:", "python3 ", "python <<"] {
                assert!(
                    !source.contains(forbidden),
                    "workflow retained interpreter setup/code: {workflow}"
                );
            }
        }
        Ok(())
    }
}
