#[path = "../no_profile.rs"]
mod compatibility;

#[test]
#[should_panic(expected = "upstream allocation profiling changed")]
fn sampler_source_drift_requires_a_new_review() {
    let source = include_str!("../c_src/mimalloc/src/profile/sample-profile.c")
        .replace("_mi_theap_set_profile_sample_rate", "changed_upstream_api");
    compatibility::patch(&source);
}
