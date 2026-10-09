use super::*;

#[test]
fn separate_users_join_chunks_and_bound_unicode() {
    let mut prompts = Prompts::default();
    prompts.push("first", "  first\n");
    prompts.push("first", "user text ");
    prompts.push("last", &"中".repeat(2000));
    let (first, last) = prompts.finish();
    assert_eq!(first.as_deref(), Some("first user text"));
    assert_eq!(last.unwrap().chars().count(), 300);
}

#[test]
fn empty_history_has_no_prompt() {
    assert_eq!(Prompts::default().finish(), (None, None));
}
