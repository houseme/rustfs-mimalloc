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
    startup::patch_theap(&include_str!("../c_src/mimalloc/src/theap.c").replace("\r\n", "\n"));
}

#[test]
fn dynamic_crt_patch_changes_only_the_image_base_declaration() {
    let source = include_str!("../c_src/mimalloc/src/prim/windows/prim.c").replace("\r\n", "\n");
    let patched = startup::patch_image_base(&source);
    assert!(patched.contains("extern IMAGE_DOS_HEADER __ImageBase;"));
    assert!(!patched.contains("mi_decl_externc IMAGE_DOS_HEADER __ImageBase;"));
    assert!(patched.contains(".CRT$XIB"));
    assert!(patched.contains(
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
