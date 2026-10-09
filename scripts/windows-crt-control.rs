// Independent of mimalloc: isolate Rust's static CRT startup and TLS behavior.
fn main() {
    let bytes = vec![0xA5u8; 64 * 1024];
    std::thread::spawn(move || assert!(bytes.iter().all(|byte| *byte == 0xA5)))
        .join()
        .unwrap();
    println!("Rust System allocator static CRT control passed");
}
