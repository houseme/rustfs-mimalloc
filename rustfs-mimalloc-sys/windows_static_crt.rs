//! Compatibility with the pinned dev3 Windows startup implementation.

pub fn enabled(target_features: &str, rustflags: &str) -> bool {
    let mut enabled = target_features.split(',').any(|flag| flag == "crt-static");
    let mut codegen_next = false;
    for flag in rustflags
        .split(|ch: char| ch.is_ascii_whitespace() || ch == '\u{1f}')
        .filter(|flag| !flag.is_empty())
    {
        let codegen = if codegen_next {
            Some(flag)
        } else {
            flag.strip_prefix("-C")
                .or_else(|| flag.strip_prefix("--codegen="))
        };
        codegen_next = matches!(flag, "-C" | "--codegen");
        if let Some(features) = codegen.and_then(|flag| flag.strip_prefix("target-feature=")) {
            for feature in features.split(',') {
                match feature {
                    "+crt-static" => enabled = true,
                    "-crt-static" => enabled = false,
                    _ => {}
                }
            }
        }
    }
    enabled
}

pub fn replace_once(source: &str, before: &str, after: &str) -> String {
    assert_eq!(
        source.matches(before).count(),
        1,
        "upstream Windows startup changed; review the static CRT compatibility patch"
    );
    source.replacen(before, after, 1)
}

pub fn patch_theap(source: &str) -> String {
    const BEFORE: &str = "    // #if defined(_WIN32) && !defined(MI_SHARED_LIB)\n    // if (tld->thread_seq==0) {\n    //   _mi_random_init_weak(&theap->random);    // prevent allocation failure during bcrypt dll initialization with static linking (issue #1185)\n    // }\n    // else\n    // #endif\n    // {\n    //   _mi_random_init(&theap->random);\n    // }";
    const AFTER: &str = "    #if defined(_WIN32) && !defined(MI_SHARED_LIB)\n    if (tld->thread_seq==0) {\n      _mi_random_init_weak(&theap->random);  // reseeded by the late CRT callback\n    }\n    #endif";
    replace_once(source, BEFORE, AFTER)
}

pub fn patch_prim(source: &str) -> String {
    let source = patch_image_base(source);
    // `.CRT$XCT` runs after CRT initialization and before normal C++ initializers.
    // Restrict the startup edits to CRT_TLS; other strategies stay unchanged.
    let (prefix, rest) = source
        .split_once("#if defined(MI_WIN_INIT_USE_CRT_TLS)\n")
        .expect("upstream CRT_TLS initialization block is missing");
    let (crt_tls, suffix) = rest
        .split_once("#elif defined(MI_WIN_INIT_USE_RAW_DLLMAIN)\n")
        .expect("upstream CRT_TLS initialization block has changed");
    let crt_tls = replace_once(
        crt_tls,
        "  typedef int (mi_cdecl* mi_crt_callback_t)(void);",
        "  typedef void (mi_cdecl* mi_crt_callback_t)(void);",
    );
    let crt_tls = replace_once(
        &crt_tls,
        "  static int mi_cdecl mi_crt_init(void) {\n",
        "  static void mi_cdecl mi_crt_init(void) {\n    mi_win_main(NULL, DLL_PROCESS_ATTACH, NULL);\n",
    );
    let crt_tls = replace_once(
        &crt_tls,
        "    return 0;\n  }\n\n  // We also hook into the Windows loader TLS initialization and finalization.",
        "  }\n\n  // We also hook into the Windows loader TLS initialization and finalization.",
    );
    let crt_tls = replace_once(
        &crt_tls,
        "      // tls process attach is always called before crt init\n      mi_win_main(module, reason, reserved);",
        "      // Defer full initialization and strong RNG reseeding until the CRT is ready.\n      // Early allocations still use mi_process_init and the weak bootstrap seed.",
    );
    assert_eq!(crt_tls.matches(".CRT$XIB").count(), 3);
    // Run after UCRT's `__acrt_initialize` but before ordinary C++ constructors.
    let crt_tls = crt_tls.replace(".CRT$XIB", ".CRT$XCT");
    format!(
        "#define MI_STATIC_CRT_STARTUP_COMPAT 1\n#if defined(_MSC_VER) && defined(_DLL)\n#error Windows static CRT compatibility requires /MT, not /MD\n#endif\n{prefix}#if defined(MI_WIN_INIT_USE_CRT_TLS)\n{crt_tls}#elif defined(MI_WIN_INIT_USE_RAW_DLLMAIN)\n{suffix}"
    )
}

pub fn patch_image_base(source: &str) -> String {
    // mi_decl_externc is empty in C, so the upstream line is a tentative
    // definition. It shadows link.exe's special image-base symbol with BSS.
    // UCRT then computes RVA-based function table addresses from that BSS slot.
    replace_once(
        source,
        "  mi_decl_externc IMAGE_DOS_HEADER __ImageBase;   // supplied by the linker",
        "  #ifdef __cplusplus\n  extern \"C\" {\n  #endif\n  extern IMAGE_DOS_HEADER __ImageBase;  // declaration only: supplied by the linker\n  #ifdef __cplusplus\n  }\n  #endif",
    )
}
