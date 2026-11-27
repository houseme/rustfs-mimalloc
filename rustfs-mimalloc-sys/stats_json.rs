//! Append fitting JSON fragments in one copy until the optimization lands upstream.

pub fn patch(source: &str) -> String {
    let source = source.replace("\r\n", "\n");
    let before = r#"static void mi_json_buf_print(mi_json_buf_t* hbuf, const char* msg) {
  if (msg==NULL || hbuf==NULL) return;
  for (const char* src = msg; *src != 0; src++) {"#;
    let after = r#"static void mi_json_buf_print(mi_json_buf_t* hbuf, const char* msg) {
  if (msg==NULL || hbuf==NULL) return;
  const size_t length = strlen(msg);
  if (length < hbuf->size - hbuf->used) {
    mi_memcpy(hbuf->buf + hbuf->used, msg, length + 1);
    hbuf->used += length;
    return;
  }
  for (const char* src = msg; *src != 0; src++) {"#;
    assert_eq!(
        source.matches(before).count(),
        1,
        "upstream JSON append changed; review the bulk-copy patch"
    );
    source.replacen(before, after, 1)
}
