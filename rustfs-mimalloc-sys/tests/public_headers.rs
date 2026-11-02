#[path = "../public_headers.rs"]
mod headers;

#[test]
fn unchanged_exports_keep_timestamps_and_changed_inputs_are_refreshed() {
    use std::{
        fs,
        time::{Duration, SystemTime},
    };
    let directory = std::env::temp_dir().join(format!(
        "mimalloc-incremental-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let source = directory.join("source");
    let output = directory.join("output");
    fs::create_dir_all(source.join("mimalloc")).unwrap();
    fs::write(source.join("mimalloc.h"), "original base header").unwrap();
    fs::write(
        source.join("mimalloc/qsbr.h"),
        include_str!("../c_src/mimalloc/include/mimalloc/qsbr.h"),
    )
    .unwrap();
    headers::prepare(&source, &output).unwrap();
    let old = SystemTime::UNIX_EPOCH + Duration::from_secs(3600);
    for header in ["mimalloc.h", "mimalloc/qsbr.h"] {
        fs::File::options()
            .write(true)
            .open(output.join(header))
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(old))
            .unwrap();
    }
    headers::prepare(&source, &output).unwrap();
    for header in ["mimalloc.h", "mimalloc/qsbr.h"] {
        assert_eq!(
            fs::metadata(output.join(header))
                .unwrap()
                .modified()
                .unwrap(),
            old,
            "unchanged {header} was rewritten"
        );
    }
    fs::write(source.join("mimalloc.h"), "updated base header").unwrap();
    headers::prepare(&source, &output).unwrap();
    assert_eq!(
        fs::read_to_string(output.join("mimalloc.h")).unwrap(),
        "updated base header"
    );
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn qsbr_correction_is_guarded_and_preserves_the_original() {
    let copy = std::env::temp_dir().join(format!(
        "mimalloc-header-copy-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    headers::prepare(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("c_src/mimalloc/include"),
        &copy,
    )
    .unwrap();
    let source = include_str!("../c_src/mimalloc/include/mimalloc/qsbr.h");
    for source in [source.to_owned(), source.replace('\n', "\r\n")] {
        let corrected = headers::qsbr(&source).unwrap();
        assert!(corrected.contains("#include \"../mimalloc.h\""));
        assert!(corrected.contains("extern \"C\" {"));
        assert!(!corrected.contains("mi_decl_externc"));
        assert!(headers::qsbr(&source.replace("mi_heap_new_qsbr", "changed_api")).is_err());
    }
    let exported = std::fs::read_to_string(
        std::path::Path::new(env!("MIMALLOC_PUBLIC_INCLUDE")).join("mimalloc/qsbr.h"),
    )
    .unwrap();
    assert_eq!(exported, headers::qsbr(source).unwrap());
    let again = copy.with_extension("again");
    assert!(headers::prepare(&copy, &again).is_err());
    assert!(
        !again.exists(),
        "invalid headers must fail before creating output"
    );
    std::fs::remove_dir_all(copy).unwrap();
}

#[test]
fn overlapping_paths_and_invalid_headers_are_rejected_before_output_changes() {
    use std::{fs, io::ErrorKind, time::SystemTime};
    let directory = std::env::temp_dir().join(format!(
        "mimalloc-path-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let source = directory.join("source");
    fs::create_dir_all(source.join("mimalloc")).unwrap();
    fs::write(source.join("mimalloc.h"), "source sentinel").unwrap();
    fs::write(
        source.join("mimalloc/qsbr.h"),
        include_str!("../c_src/mimalloc/include/mimalloc/qsbr.h"),
    )
    .unwrap();
    for destination in [&source, &source.join("nested/output"), &directory] {
        assert_eq!(
            headers::prepare(&source, destination).unwrap_err().kind(),
            ErrorKind::InvalidInput
        );
    }
    assert!(!source.join("nested").exists());
    assert_eq!(
        fs::read_to_string(source.join("mimalloc.h")).unwrap(),
        "source sentinel"
    );
    #[cfg(unix)]
    {
        let alias = directory.join("alias");
        std::os::unix::fs::symlink(&source, &alias).unwrap();
        assert_eq!(
            headers::prepare(&source, &alias.join("output"))
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidInput
        );
        assert!(!source.join("output").exists());
    }
    fs::write(source.join("mimalloc/qsbr.h"), "changed declaration").unwrap();
    let destination = directory.join("untouched");
    fs::create_dir(&destination).unwrap();
    fs::write(destination.join("mimalloc.h"), "output sentinel").unwrap();
    assert_eq!(
        headers::prepare(&source, &destination).unwrap_err().kind(),
        ErrorKind::InvalidData
    );
    assert_eq!(
        fs::read_to_string(destination.join("mimalloc.h")).unwrap(),
        "output sentinel"
    );
    let missing = directory.join("not-created");
    assert!(headers::prepare(&source, &missing).is_err());
    assert!(!missing.exists());
    fs::remove_dir_all(directory).unwrap();
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn public_headers_compile_independently_and_cpp_resolves_c_symbols() {
    use std::{fs, path::Path, process::Command};
    let directory = std::env::temp_dir().join(format!(
        "mimalloc-headers-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&directory).unwrap();
    let include = Path::new(env!("MIMALLOC_PUBLIC_INCLUDE"));
    for header in [
        "mimalloc.h",
        "mimalloc-profile.h",
        "mimalloc-stats.h",
        "mimalloc/qsbr.h",
    ] {
        for (compiler, extension, standard) in
            [("cc", "c", "-std=c11"), ("c++", "cpp", "-std=c++11")]
        {
            let path = directory.join(format!("header.{extension}"));
            fs::write(&path, format!("#include \"{header}\"\n")).unwrap();
            let out = Command::new(compiler)
                .args([standard, "-Werror", "-fsyntax-only", "-I"])
                .arg(include)
                .arg(path)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{header}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }
    if !cfg!(target_env = "musl") {
        let source = directory.join("link.cpp");
        fs::write(&source, include_str!("fixtures/public_headers.cpp")).unwrap();
        let binary = directory.join("link");
        let mut command = Command::new("c++");
        command
            .args(["-std=c++11", "-I"])
            .arg(include)
            .arg(source)
            .arg(include.parent().unwrap().join("libmimalloc.a"))
            .args(["-pthread", "-lm"]);
        if cfg!(target_os = "linux") {
            command.args(["-ldl", "-lrt"]);
        }
        let out = command.arg("-o").arg(&binary).output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let out = Command::new(binary).output().unwrap();
        assert!(
            out.status.success(),
            "status={}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
    }
    fs::remove_dir_all(directory).unwrap();
}
