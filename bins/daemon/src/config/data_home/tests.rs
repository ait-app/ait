use super::*;

#[test]
fn missing_defaults_select_ait_without_creating_it() {
    let home = tempfile::tempdir().unwrap();

    let directory = resolve(home.path());

    assert_eq!(directory, home.path().join(".ait"));
    assert!(!directory.exists());
}

#[test]
fn existing_defaults_are_read_in_order_and_never_moved() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir(home.path().join(".ait-server")).unwrap();
    assert_eq!(resolve(home.path()), home.path().join(".ait-server"));

    std::fs::create_dir(home.path().join(".ait-server-desktop")).unwrap();
    assert_eq!(
        resolve(home.path()),
        home.path().join(".ait-server-desktop")
    );

    std::fs::create_dir(home.path().join(".ait")).unwrap();
    assert_eq!(resolve(home.path()), home.path().join(".ait"));

    for name in DATA_DIRECTORIES {
        assert!(home.path().join(name).is_dir());
    }
}
