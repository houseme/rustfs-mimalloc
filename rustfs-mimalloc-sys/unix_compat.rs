//! Build compatibility for the pinned dev3 Unix implementation.

pub fn replace_once(source: &str, before: &str, after: &str) -> String {
    assert_eq!(
        source.matches(before).count(),
        1,
        "upstream Unix THP source changed; review the no_thp compatibility patch"
    );
    source.replacen(before, after, 1)
}

/// With MI_NO_THP, the positive-advice body is absent; omit its dangling else.
/// Retain the independent oversized-page MADV_NOHUGEPAGE protection.
pub fn patch_no_thp(source: &str) -> String {
    replace_once(
        source,
        "      else\n      #endif\n      #if !defined(MI_NO_THP)",
        "      #endif\n      #if !defined(MI_NO_THP)",
    )
}
