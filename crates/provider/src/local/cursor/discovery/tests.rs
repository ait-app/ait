use super::*;

#[test]
fn searches_official_user_directory_without_using_the_editor() {
    let directory = tempfile::tempdir().unwrap();
    let bin = directory.path().join(".local/bin");
    std::fs::create_dir_all(&bin).unwrap();
    let program = bin.join("agent");
    std::fs::write(&program, "fixture").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    assert_eq!(resolve(None, Some(directory.path().to_owned())), program);
    let alias = bin.join("cursor-agent");
    std::fs::copy(&program, &alias).unwrap();
    assert_eq!(resolve(Some(bin.as_os_str()), None), alias);
    assert_eq!(resolve(None, None), PathBuf::from("cursor-agent"));
}
