//! Native options can initialize before Rust main. Each case starts a process
//! with its environment already installed; no in-process environment mutation.
use rustfs_mimalloc_sys::{c_long, mi_option_get, mi_option_get_size, mi_option_t};

fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--native-defaults") {
        // The environment is cleared by the parent before native startup.
        unsafe {
            assert_eq!(
                mi_option_get(mi_option_t::mi_option_stats_merge_threshold),
                4 * 1024 * 1024
            );
            assert_eq!(
                mi_option_get_size(mi_option_t::mi_option_stats_merge_threshold),
                4 * 1024 * 1024
            );
            assert_eq!(
                mi_option_get(mi_option_t::mi_option_profile_sample_rate),
                512 * 1024
            );
            assert_eq!(
                mi_option_get_size(mi_option_t::mi_option_profile_sample_rate),
                512 * 1024
            );
            assert_eq!(
                mi_option_get(mi_option_t::mi_option_profile_alloc_interval),
                1024 * 1024 * 1024
            );
            assert_eq!(
                mi_option_get_size(mi_option_t::mi_option_profile_alloc_interval),
                1024 * 1024 * 1024
            );
        }
        return;
    }
    if args.get(1).map(String::as_str) == Some("--child") {
        let expected: i64 = args[3].parse().unwrap();
        let actual = unsafe {
            match args[2].as_str() {
                "integer" => mi_option_get(mi_option_t::mi_option_max_warnings) as i64,
                "bytes" => mi_option_get_size(mi_option_t::mi_option_stats_merge_threshold) as i64,
                "kib" => mi_option_get_size(mi_option_t::mi_option_arena_purge_min_size) as i64,
                _ => panic!("unknown option case"),
            }
        };
        assert_eq!(actual, expected);
        return;
    }

    let long_max = if size_of::<c_long>() == 4 {
        i32::MAX as i64
    } else {
        i64::MAX
    };
    let long_min = if size_of::<c_long>() == 4 {
        i32::MIN as i64
    } else {
        i64::MIN
    };
    let cases = [
        ("integer", "42".to_owned(), 42),
        ("integer", " \t+42".to_owned(), 42),
        ("integer", "-42".to_owned(), -42),
        ("integer", "0x2a".to_owned(), 42),
        ("integer", "-0X2A".to_owned(), -42),
        ("integer", "0x00002A".to_owned(), 42),
        ("integer", long_max.to_string(), long_max),
        ("integer", long_min.to_string(), long_min),
        ("integer", "0".to_owned(), 0),
        // Malformed, overflowing and newly rejected zero-padded decimals
        // retain the native max_warnings default instead of a partial value.
        ("integer", "042".to_owned(), 32),
        ("integer", "00".to_owned(), 32),
        ("integer", "42junk".to_owned(), 32),
        ("integer", "0x".to_owned(), 32),
        ("integer", "42 ".to_owned(), 32),
        ("integer", format!("{long_max}0"), 32),
        ("integer", format!("{long_min}0"), 32),
        // Preserve opt-out and the previous threshold through the public option.
        ("bytes", "0".to_owned(), 0),
        ("bytes", "512KiB".to_owned(), 512 * 1024),
        ("bytes", "4MiB".to_owned(), 4 * 1024 * 1024),
        ("bytes", "2MiB".to_owned(), 2 * 1024 * 1024),
        ("bytes", "0x2MiB".to_owned(), 2 * 1024 * 1024),
        ("bytes", "2mb".to_owned(), 2 * 1024 * 1024),
        ("kib", "0x2MiB".to_owned(), 2 * 1024 * 1024),
        ("kib", "1025".to_owned(), 2 * 1024),
        ("bytes", "32KiB".to_owned(), 32 * 1024),
        ("bytes", "2kb".to_owned(), 2 * 1024),
        ("bytes", "0x20KiB".to_owned(), 32 * 1024),
        ("kib", "32KiB".to_owned(), 32 * 1024),
        ("kib", "2kb".to_owned(), 2 * 1024),
        ("kib", "0x20KiB".to_owned(), 32 * 1024),
    ];
    let executable = std::env::current_exe().unwrap();
    for (kind, value, expected) in &cases {
        let variable = match *kind {
            "integer" => "MIMALLOC_MAX_WARNINGS",
            "bytes" => "MIMALLOC_STATS_MERGE_THRESHOLD",
            "kib" => "MIMALLOC_ARENA_PURGE_MIN_SIZE",
            _ => unreachable!(),
        };
        let mut command = std::process::Command::new(&executable);
        // Native lookup is case-insensitive. Remove inherited spellings and
        // the legacy size-option name so one deterministic value is present.
        for (name, _) in std::env::vars_os() {
            if name.to_str().is_some_and(|name| {
                name.eq_ignore_ascii_case(variable)
                    || name.eq_ignore_ascii_case("MIMALLOC_MINIMAL_PURGE_SIZE")
            }) {
                command.env_remove(name);
            }
        }
        let output = command
            .env(variable, value)
            .args(["--child", kind, &expected.to_string()])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{variable}={value:?}, expected {expected}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let mut defaults = std::process::Command::new(&executable);
    for (name, _) in std::env::vars_os() {
        if name.to_str().is_some_and(|name| {
            let name = name.to_ascii_uppercase();
            name.starts_with("MIMALLOC_PROFILE") || name == "MIMALLOC_STATS_MERGE_THRESHOLD"
        }) {
            defaults.env_remove(name);
        }
    }
    let output = defaults.arg("--native-defaults").output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    println!(
        "native environment parsing passed {} isolated cases",
        cases.len() + 1
    );
}
