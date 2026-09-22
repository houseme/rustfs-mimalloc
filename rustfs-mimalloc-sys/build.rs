//! Build script for rustfs-mimalloc-sys.
//!
//! Compiles mimalloc V3 as a static library using the `cc` crate.
//! Handles platform-specific flags, feature gates, and linker requirements.

use std::env;

struct TargetCfg {
    triple: String,
    arch: String,
    env: String,
    os: String,
    vendor: String,
}

impl TargetCfg {
    fn from_env() -> Self {
        Self {
            triple: env::var("TARGET").unwrap_or_default(),
            arch: env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default(),
            env: env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default(),
            os: env::var("CARGO_CFG_TARGET_OS").unwrap_or_default(),
            vendor: env::var("CARGO_CFG_TARGET_VENDOR").unwrap_or_default(),
        }
    }

    fn is_apple(&self) -> bool {
        self.vendor == "apple"
    }

    fn is_msvc(&self) -> bool {
        self.env == "msvc"
    }

    fn is_musl(&self) -> bool {
        self.env == "musl"
    }

    fn is_windows(&self) -> bool {
        self.os == "windows"
    }

    fn supports_initial_exec_tls(&self) -> bool {
        self.is_apple() || matches!(self.os.as_str(), "linux" | "freebsd")
    }

    fn needs_armv6_atomic(&self) -> bool {
        self.arch == "arm"
            && (self.triple.starts_with("armv6") || self.triple.starts_with("arm-unknown"))
    }
}

fn main() {
    let target = TargetCfg::from_env();
    let is_debug = env::var("PROFILE").as_deref() == Ok("debug");

    // Tell Cargo to re-run if features change
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_SECURE");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_DEBUG");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_DEBUG_IN_DEBUG");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_OVERRIDE");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_LOCAL_DYNAMIC_TLS");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_WIN_DIRECT_TLS");
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_NO_THP");

    let mut build = cc::Build::new();
    build.file("c_src/mimalloc/src/static.c");
    build.include("c_src/mimalloc/include");
    build.include("c_src/mimalloc/src");

    // Optimization flags
    if !is_debug {
        build.opt_level(3);
        build.define("NDEBUG", None);
        build.define("MI_BUILD_RELEASE", None);
    } else {
        build.opt_level(0);
        build.debug(true);
    }

    // Debug mode: either explicit feature or auto-enable in debug builds
    if env::var_os("CARGO_FEATURE_DEBUG").is_some()
        || (env::var_os("CARGO_FEATURE_DEBUG_IN_DEBUG").is_some() && is_debug)
    {
        build.define("MI_DEBUG", "2");
    } else {
        build.define("MI_DEBUG", "0");
    }

    // Secure mode: encrypt heap allocations
    if env::var_os("CARGO_FEATURE_SECURE").is_some() {
        build.define("MI_SECURE", "4");
    }

    // TLS model: default is initial-exec for performance.
    // Use `local_dynamic_tls` feature to switch to local-dynamic model,
    // which fixes compatibility with projects like polars that have TLS issues.
    // See: https://github.com/purpleprotocol/mimalloc_rust/issues/138
    if env::var_os("CARGO_FEATURE_LOCAL_DYNAMIC_TLS").is_some() {
        build.flag_if_supported("-ftls-model=local-dynamic");
    } else if target.supports_initial_exec_tls() {
        build.flag_if_supported("-ftls-model=initial-exec");
    }

    if target.is_windows() && env::var_os("CARGO_FEATURE_WIN_DIRECT_TLS").is_some() {
        build.define("MI_WIN_DIRECT_TLS", "1");
    }

    // Disable THP on Linux/Android if requested
    // See: https://github.com/purpleprotocol/mimalloc_rust/pull/112
    if env::var_os("CARGO_FEATURE_NO_THP").is_some() {
        build.define("MI_NO_THP", "1");
    }

    // macOS: enable dyld interposing for proper override support
    // See: https://github.com/purpleprotocol/mimalloc_rust/pull/145
    if target.is_apple() && env::var_os("CARGO_FEATURE_OVERRIDE").is_some() {
        build.define("MI_OSX_ZONE", "1");
        build.define("MI_OSX_INTERPOSE", "1");
    }

    // Platform-specific compiler flags
    if target.is_msvc() {
        // MSVC: use correct runtime library based on debug/release
        // See: https://github.com/purpleprotocol/mimalloc_rust/pull/167
        if is_debug {
            build.flag("/MDd");
        } else {
            build.flag("/MD");
        }
        // Suppress common MSVC warnings
        build.flag("/wd4100"); // unreferenced formal parameter
        build.flag("/wd4127"); // conditional expression is constant
        build.flag("/wd4201"); // nameless struct/union
    } else {
        // Unix-like: standard warning suppression
        build.flag_if_supported("-Wno-unused-function");
        build.flag_if_supported("-Wno-unused-parameter");

        // Fix musl + release build failures with __DATE__ / __TIME__ macros
        // See: https://github.com/purpleprotocol/mimalloc_rust/pull/139
        if target.is_musl() {
            build.flag_if_supported("-Wno-error=date-time");
        }
    }

    // ARM-specific: do NOT force ARMv8.1-A (fixes Raspberry Pi 4 compatibility)
    // See: https://github.com/purpleprotocol/mimalloc_rust/issues/165
    // We let the compiler use the target's default architecture level.
    // If the user wants ARMv8.1-A optimizations, they can set RUSTFLAGS.

    build.compile("mimalloc");

    // Link required system libraries
    link_system_libs(&target);

    // Export include directory for downstream crates
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    println!("cargo:include={manifest_dir}/c_src/mimalloc/include");

    // Print version info
    println!("cargo:version=30503"); // MI_MALLOC_VERSION from mimalloc.h
}

/// Link required system libraries based on the target platform.
fn link_system_libs(target: &TargetCfg) {
    if target.is_windows() {
        // Windows: required for crypto (BCryptGenRandom), process info (psapi),
        // and token manipulation (advapi32 for large pages)
        // See: https://github.com/purpleprotocol/mimalloc_rust/issues/135
        println!("cargo:rustc-link-lib=advapi32");
        println!("cargo:rustc-link-lib=bcrypt");
        println!("cargo:rustc-link-lib=psapi");
        println!("cargo:rustc-link-lib=shell32");
        println!("cargo:rustc-link-lib=user32");
    }

    // ARMv6: needs libatomic for 64-bit atomic operations
    // See: https://github.com/purpleprotocol/mimalloc_rust/pull/115
    if target.needs_armv6_atomic() {
        println!("cargo:rustc-link-lib=atomic");
    }
}
