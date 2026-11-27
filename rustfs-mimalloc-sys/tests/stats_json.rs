#[path = "../stats_json.rs"]
mod stats_json;

#[test]
fn fixed_json_buffer_preserves_truncated_prefix_and_terminator() {
    let prefix = b"{\n  \"stat_version\": ";
    for capacity in 1..=prefix.len() + 1 {
        let mut output = [0xa5_u8; 32];
        let result =
            unsafe { rustfs_mimalloc_sys::mi_stats_get_json(capacity, output.as_mut_ptr().cast()) };
        assert!(result.is_null());
        assert_eq!(&output[..capacity - 1], &prefix[..capacity - 1]);
        assert_eq!(output[capacity - 1], 0);
        assert!(output[capacity..].iter().all(|&byte| byte == 0xa5));
    }
}

#[test]
#[should_panic(expected = "upstream JSON append changed")]
fn json_append_source_drift_requires_review() {
    let source = include_str!("../c_src/mimalloc/src/util/stats.c").replace(
        "static void mi_json_buf_print(",
        "static void changed_append(",
    );
    stats_json::patch(&source);
}
