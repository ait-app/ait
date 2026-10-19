use super::decode;

#[test]
fn quoted_paths_preserve_control_characters_quotes_and_utf8_octets() {
    assert_eq!(
        decode(r#""b/quote\"name\tline\n.txt""#),
        "b/quote\"name\tline\n.txt"
    );
    assert_eq!(decode(r#""\344\270\255\346\226\207.txt""#), "中文.txt");
    assert_eq!(decode(" plain => name "), " plain => name ");
    assert_eq!(decode(r#""bad\q""#), r#""bad\q""#);
    assert_eq!(decode(r#""bad\777""#), r#""bad\777""#);
}
