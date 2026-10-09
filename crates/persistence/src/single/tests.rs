use super::*;

#[test]
fn atomic_writes_create_parents_and_replace_complete_contents() {
    let root = tempfile::tempdir().unwrap();
    let file = File::new(root.path().join("nested/file.txt"));
    assert_eq!(file.path(), root.path().join("nested/file.txt"));
    assert!(
        matches!(file.read(), Err(Error::Io(error)) if error.kind() == std::io::ErrorKind::NotFound)
    );
    file.write(b"first").unwrap();
    assert_eq!(file.read().unwrap(), b"first");
    file.write(b"replacement").unwrap();
    assert_eq!(file.read_text().unwrap(), "replacement");
    assert_eq!(file.read_limited(11).unwrap(), b"replacement");
    assert!(matches!(file.read_limited(10), Err(Error::TooLarge)));
    assert_eq!(
        fs::read_dir(file.path().parent().unwrap()).unwrap().count(),
        1
    );
}

#[test]
fn failed_reads_and_writes_preserve_existing_paths() {
    let root = tempfile::tempdir().unwrap();
    let parent = File::new(root.path().join("parent"));
    parent.write(b"retained").unwrap();
    assert!(
        File::new(parent.path().join("child"))
            .write(b"new")
            .is_err()
    );
    assert_eq!(parent.read().unwrap(), b"retained");
    assert!(File::new(root.path()).write(b"new").is_err());
    parent.write(&[0xff]).unwrap();
    assert!(parent.read_text().is_err());
    parent.write(b"").unwrap();
    assert!(parent.read_limited(0).unwrap().is_empty());
}
