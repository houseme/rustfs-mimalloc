//! Preserve the MI_PROFILE=0 contract when raw profilers are explicitly attached.

pub fn patch(source: &str) -> String {
    let mut source = source.replace("\r\n", "\n");
    for (before, after) in [
        (
            "static mi_profiler_t* mi_theap_get_enabled_profiler(const mi_theap_t* theap) {\n",
            "static mi_profiler_t* mi_theap_get_enabled_profiler(const mi_theap_t* theap) {\n  if (!MI_PROFILE) { return NULL; }\n",
        ),
        (
            "size_t _mi_theap_set_profile_sample_rate(mi_theap_t* theap, size_t sample_rate) {\n",
            "size_t _mi_theap_set_profile_sample_rate(mi_theap_t* theap, size_t sample_rate) {\n  if (!MI_PROFILE) { sample_rate = 0; }\n",
        ),
    ] {
        assert_eq!(
            source.matches(before).count(),
            1,
            "upstream allocation profiling changed; review MI_PROFILE=0 compatibility"
        );
        source = source.replacen(before, after, 1);
    }
    source
}
