use super::{random_hex, random_hyphenated_id};

#[test]
fn hex_tokens_have_two_lowercase_digits_per_byte() {
    let token = random_hex::<4>().unwrap();
    assert_eq!(token.len(), 8);
    assert!(
        token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    );
}

#[test]
fn hyphenated_ids_use_the_8_4_4_4_12_layout() {
    let id = random_hyphenated_id().unwrap();
    let groups = id.split('-').map(str::len).collect::<Vec<_>>();
    assert_eq!(groups, [8, 4, 4, 4, 12]);
    assert_ne!(id, random_hyphenated_id().unwrap());
}
