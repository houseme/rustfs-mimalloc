#[path = "../windows_static_crt.rs"]
mod startup;

#[test]
fn crt_selection_handles_signed_and_encoded_codegen_flags() {
    assert!(startup::enabled("sse2,crt-static", ""));
    for flags in [
        "-C target-feature=+crt-static",
        "-C  \t target-feature=+crt-static",
        "-Ctarget-feature=+sse2,+crt-static",
        "-C\u{1f}target-feature=+crt-static",
        "--codegen\u{1f}target-feature=+crt-static",
        "--codegen=target-feature=+crt-static",
    ] {
        assert!(startup::enabled("sse2", flags), "{flags}");
    }
    assert!(!startup::enabled("", "-C target-feature=-crt-static"));
    assert!(!startup::enabled(
        "crt-static",
        "-Ctarget-feature=-crt-static"
    ));
    assert!(!startup::enabled("", "--cfg=feature=\"crt-static\""));
    assert!(!startup::enabled(
        "",
        "-Ctarget-feature=+crt-static -Ctarget-feature=-crt-static"
    ));
    assert!(startup::enabled(
        "",
        "-Ctarget-feature=-crt-static -Ctarget-feature=+crt-static"
    ));
}

#[test]
fn pinned_startup_sources_accept_patch_and_preserve_thread_cleanup() {
    let source = include_str!("../c_src/mimalloc/src/prim/windows/prim.c").replace("\r\n", "\n");
    let patched = startup::patch_prim(&source);
    assert!(patched.contains("#define MI_STATIC_CRT_STARTUP_COMPAT 1"));
    let before = source
        .split("  static void NTAPI mi_tls_detach")
        .nth(1)
        .unwrap();
    let after = patched
        .split("  static void NTAPI mi_tls_detach")
        .nth(1)
        .unwrap();
    assert_eq!(
        before.split("  // Set up TLS callbacks").next(),
        after.split("  // Set up TLS callbacks").next()
    );
    assert!(patched.contains(".CRT$XCT"));
    assert!(patched.contains(
        "static void mi_cdecl mi_crt_init(void) {\n    mi_win_main(NULL, DLL_PROCESS_ATTACH, NULL);"
    ));
    assert!(!patched.contains(
        "// tls process attach is always called before crt init\n      mi_win_main(module, reason, reserved);"
    ));
    let (_, before) = source
        .split_once("#elif defined(MI_WIN_INIT_USE_RAW_DLLMAIN)")
        .unwrap();
    let (_, after) = patched
        .split_once("#elif defined(MI_WIN_INIT_USE_RAW_DLLMAIN)")
        .unwrap();
    assert_eq!(before, after);
    let random = include_str!("../c_src/mimalloc/src/util/random.c").replace("\r\n", "\n");
    let patched = startup::patch_random(&random);
    assert!(patched.contains("use_weak || !_mi_prim_random_buf"));
    assert!(patched.contains("!use_weak && mi_atomic_exchange_relaxed"));
}

#[test]
fn upstream_image_base_is_external_without_a_local_patch() {
    let source = include_str!("../c_src/mimalloc/src/prim/windows/prim.c").replace("\r\n", "\n");
    assert!(source.contains("extern IMAGE_DOS_HEADER __ImageBase;"));
    assert!(!source.contains("mi_decl_externc IMAGE_DOS_HEADER __ImageBase;"));
    assert!(source.contains(".CRT$XIB"));
    assert!(source.contains(
        "// tls process attach is always called before crt init\n      mi_win_main(module, reason, reserved);"
    ));
}

#[test]
#[should_panic(expected = "upstream Windows startup changed")]
fn upstream_drift_fails_build_instead_of_silently_skipping_patch() {
    let source = include_str!("../c_src/mimalloc/src/prim/windows/prim.c")
        .replace("\r\n", "\n")
        .replace(
            "  static int mi_cdecl mi_crt_init(void) {",
            "changed upstream callback",
        );
    startup::patch_prim(&source);
}

#[cfg(unix)]
#[test]
fn bootstrap_entropy_defers_os_calls_and_retains_real_failure_warnings() {
    use std::{fs, process::Command};
    let original = include_str!("../c_src/mimalloc/src/util/random.c").replace("\r\n", "\n");
    let source = startup::patch_random(&original);
    let start = source
        .find("void _mi_random_init(mi_random_ctx_t* ctx) {")
        .unwrap();
    let end = start + source[start..].find("\n}").unwrap() + 2;
    let probe =
        include_str!("fixtures/random_probe.c").replace("/* @INIT@ */", &source[start..end]);
    let directory = std::env::temp_dir().join(format!(
        "mimalloc-entropy-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&directory).unwrap();
    let path = directory.join("probe.c");
    fs::write(&path, probe).unwrap();
    let executable = directory.join("probe");
    let result = Command::new("cc")
        .args(["-std=c11", "-Wall", "-Wextra", "-Werror"])
        .arg(path)
        .arg("-o")
        .arg(&executable)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(Command::new(executable).status().unwrap().success());
    fs::remove_dir_all(directory).unwrap();
}
