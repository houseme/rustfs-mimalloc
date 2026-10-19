#[path = "../unix_compat.rs"]
mod compatibility;

#[test]
fn no_thp_patch_changes_only_the_dangling_else() {
    let original = include_str!("../c_src/mimalloc/src/prim/unix/prim.c").replace("\r\n", "\n");
    let patched = compatibility::patch_no_thp(&original);
    assert_eq!(original.len() - patched.len(), "      else\n".len());
    assert!(patched.contains("unix_madvise(p, size, MADV_NOHUGEPAGE)"));
}

#[test]
#[should_panic(expected = "upstream Unix THP source changed")]
fn no_thp_patch_rejects_upstream_drift() {
    compatibility::patch_no_thp("changed upstream source");
}

#[cfg(unix)]
#[test]
fn native_parser_pmd_and_advice_controls() {
    use std::{fs, process::Command};
    fn function<'a>(source: &'a str, signature: &str) -> &'a str {
        let start = source.find(signature).expect("native function is missing");
        let end = start + source[start..].find("\n}").expect("function end") + 2;
        &source[start..end]
    }
    let unix = include_str!("../c_src/mimalloc/src/prim/unix/prim.c").replace("\r\n", "\n");
    let libc = include_str!("../c_src/mimalloc/src/util/libc.c");
    let parser = &libc
        [libc.find("static bool mi_isspace(").unwrap()..libc.find("#ifdef MI_NO_GETENV").unwrap()];
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let directory =
        std::env::temp_dir().join(format!("mimalloc-unix-{}-{nonce}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    for no_thp in [false, true] {
        let source = if no_thp {
            compatibility::patch_no_thp(&unix)
        } else {
            unix.clone()
        };
        let probe = include_str!("fixtures/unix_probe.c")
            .replace("/* @PARSER@ */", parser)
            .replace(
                "/* @DETECTOR@ */",
                function(&source, "static size_t unix_detect_large_page_size("),
            )
            .replace("/* @MMAP@ */", function(&source, "static void* unix_mmap("));
        let input = directory.join("probe.c");
        fs::write(&input, &probe).unwrap();
        for has_nohugepage in [false, true] {
            let executable = directory.join("probe");
            let mut command = Command::new("cc");
            command.args([
                "-std=c11",
                "-Wall",
                "-Wextra",
                "-Werror",
                "-Wno-unused-parameter",
                "-Wno-unused-function",
                "-Wno-sign-compare",
            ]);
            if no_thp {
                command.arg("-DMI_NO_THP=1");
            }
            if has_nohugepage {
                command.arg("-DTEST_HAS_NOHUGEPAGE=1");
            }
            let output = command
                .arg(&input)
                .arg("-o")
                .arg(&executable)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let output = Command::new(&executable).output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        if no_thp {
            let unpatched = probe.replace(
                function(&source, "static void* unix_mmap("),
                function(&unix, "static void* unix_mmap("),
            );
            fs::write(&input, unpatched).unwrap();
            let output = Command::new("cc")
                .args([
                    "-std=c11",
                    "-DMI_NO_THP=1",
                    "-DTEST_HAS_NOHUGEPAGE=1",
                    "-fsyntax-only",
                ])
                .arg(&input)
                .output()
                .unwrap();
            assert!(
                !output.status.success(),
                "negative control must reject the unpatched dangling else"
            );
        }
    }
    fs::remove_dir_all(directory).unwrap();
}
