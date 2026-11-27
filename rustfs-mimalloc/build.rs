fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rustc-check-cfg=cfg(allocator_api_stable)");
    if rustversion::cfg!(any(since(1.101), all(since(1.100), not(nightly)))) {
        println!("cargo:rustc-cfg=allocator_api_stable");
    }
}
