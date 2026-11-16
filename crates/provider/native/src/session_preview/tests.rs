use super::*;

#[test]
fn previews_are_bounded_unicode_text_without_blank_rows() {
    assert_eq!(
        text(["  hello\n", " world "].into_iter()).as_deref(),
        Some("hello world")
    );
    assert!(text([" ", "\t"].into_iter()).is_none());
    let long = "图片".repeat(200);
    let value = text([long.as_str()].into_iter()).unwrap();
    assert_eq!(value.chars().count(), 300);
}
