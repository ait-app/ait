use super::*;

mod literal_paths;
mod paseo;

#[test]
fn nested_copy_and_edit_limits_preserve_the_source_and_reject_nonfiles() {
    let (_temp, files, cwd) = fixture();
    let nested = Path::new(&cwd).join("nested");
    fs::create_dir(&nested).unwrap();
    fs::write(nested.join("note.txt"), "original").unwrap();
    assert_eq!(
        files.duplicate(&cwd, "nested/note.txt").unwrap(),
        "nested/note copy.txt"
    );
    assert_eq!(read(&files, &cwd, "nested/note copy.txt").1, b"original");
    let (info, _) = read(&files, &cwd, "nested/note.txt");
    let large = nested.join("large.txt");
    fs::write(
        &large,
        vec![b'x'; usize::try_from(MAX_EDITABLE).unwrap() + 1],
    )
    .unwrap();
    assert_eq!(
        files
            .write(&edit(&cwd, "nested/large.txt", &info, "replacement"))
            .unwrap_err()
            .0,
        "File is too large to edit"
    );
    assert_eq!(fs::metadata(&large).unwrap().len(), MAX_EDITABLE + 1);
    assert!(
        files
            .write(&edit(&cwd, "nested", &info, "replacement"))
            .is_err()
    );
    assert!(matches!(
        files.version(&cwd, "../outside"),
        FileVersion::Error(_)
    ));
    assert!(nested.is_dir());
}

#[test]
fn copy_budget_rejects_depth_entry_and_byte_limits_before_creating_a_destination() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let target = root.path().join("target");
    fs::write(&source, "content").unwrap();
    for (mut budget, depth) in [
        (CopyBudget::default(), 65),
        (
            CopyBudget {
                entries: 20_000,
                bytes: 0,
            },
            0,
        ),
        (
            CopyBudget {
                entries: 0,
                bytes: 64 * 1024 * 1024,
            },
            0,
        ),
    ] {
        assert_eq!(
            copy_entry(&source, &target, &mut budget, depth)
                .unwrap_err()
                .0,
            "Copy exceeds filesystem operation limit"
        );
        assert!(!target.exists());
        assert_eq!(fs::read_to_string(&source).unwrap(), "content");
    }
}

#[cfg(unix)]
#[test]
fn directory_links_are_listed_but_special_files_cannot_be_copied() {
    let (_temp, files, cwd) = fixture();
    let target = Path::new(&cwd).join("target");
    fs::create_dir(&target).unwrap();
    std::os::unix::fs::symlink(&target, Path::new(&cwd).join("alias")).unwrap();
    let listed = files.list(&cwd, ".").unwrap();
    assert_eq!(
        listed
            .1
            .iter()
            .find(|entry| entry.name == "alias")
            .unwrap()
            .kind,
        EntryKind::Directory
    );
    let socket_path = target.join("socket");
    let _socket = std::os::unix::net::UnixListener::bind(&socket_path).unwrap();
    assert!(
        files
            .duplicate(&cwd, "target/socket")
            .unwrap_err()
            .0
            .contains("Special files")
    );
    assert!(!target.join("socket copy").exists());
}

fn fixture() -> (tempfile::TempDir, LocalFiles, String) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("workspace");
    fs::create_dir(&root).unwrap();
    let files = LocalFiles::new(temp.path().to_owned(), &temp.path().join("data"));
    (temp, files, root.to_str().unwrap().to_owned())
}

fn read(files: &LocalFiles, cwd: &str, path: &str) -> (FileInfo, Vec<u8>) {
    let mut reader = files.open(cwd, path).unwrap();
    let info = reader.info().clone();
    let mut content = Vec::new();
    reader.read_to_end(&mut content).unwrap();
    reader.verify().unwrap();
    (info, content)
}

fn edit(cwd: &str, path: &str, info: &FileInfo, content: &str) -> FileWrite {
    FileWrite {
        cwd: cwd.to_owned(),
        path: path.to_owned(),
        content: content.to_owned(),
        expected_modified_at: info.modified_at.clone(),
        expected_revision: Some(info.revision.clone()),
    }
}

#[test]
fn writes_atomically_preserves_permissions_and_prefers_revision() {
    let (_temp, files, cwd) = fixture();
    fs::write(Path::new(&cwd).join("file.ex"), "before").unwrap();
    let (initial, _) = read(&files, &cwd, "file.ex");
    let mut request = edit(&cwd, "file.ex", &initial, "after");
    request.expected_modified_at = "wrong display time".to_owned();
    assert!(matches!(
        files.write(&request).unwrap(),
        FileWritten::Written(_)
    ));
    assert_eq!(read(&files, &cwd, "file.ex").1, b"after");
    assert!(matches!(
        files.write(&request).unwrap(),
        FileWritten::Conflict(FileVersion::Ready(_))
    ));
    assert_eq!(fs::read_dir(&cwd).unwrap().count(), 1);
}

#[test]
fn missing_write_never_creates_and_binary_or_large_edits_are_rejected() {
    let (_temp, files, cwd) = fixture();
    let dummy = FileInfo {
        root: cwd.clone(),
        absolute_path: Path::new(&cwd)
            .join("absent")
            .to_string_lossy()
            .into_owned(),
        path: "absent".to_owned(),
        file_name: String::new(),
        mime_type: String::new(),
        kind: FileKind::Text,
        size: 0,
        modified_at: String::new(),
        revision: String::new(),
    };
    assert!(matches!(
        files.write(&edit(&cwd, "absent", &dummy, "x")).unwrap(),
        FileWritten::Conflict(FileVersion::Missing)
    ));
    assert!(!Path::new(&cwd).join("absent").exists());
    fs::write(Path::new(&cwd).join("binary"), [0, 1, 2]).unwrap();
    let (info, _) = read(&files, &cwd, "binary");
    assert_eq!(
        files
            .write(&edit(&cwd, "binary", &info, "text"))
            .unwrap_err()
            .0,
        "Binary files cannot be edited"
    );
    assert_eq!(
        files
            .write(&edit(
                &cwd,
                "binary",
                &info,
                &"x".repeat(usize::try_from(MAX_EDITABLE).unwrap() + 1)
            ))
            .unwrap_err()
            .0,
        "File is too large to edit"
    );
}

#[test]
fn reads_text_images_json_binary_and_split_utf8_samples() {
    let (_temp, files, cwd) = fixture();
    for (name, content, kind, mime) in [
        (
            "unknown.ext",
            b"hello".as_slice(),
            FileKind::Text,
            "text/plain",
        ),
        (
            "data.json",
            b"{}".as_slice(),
            FileKind::Text,
            "application/json",
        ),
        (
            "image.PNG",
            b"image".as_slice(),
            FileKind::Image,
            "image/png",
        ),
        (
            "report.PDF",
            b"%PDF-1.4 ASCII document".as_slice(),
            FileKind::Binary,
            "application/pdf",
        ),
        ("nul", &[0, 1], FileKind::Binary, "application/octet-stream"),
        (
            "invalid",
            &[0xff],
            FileKind::Binary,
            "application/octet-stream",
        ),
    ] {
        fs::write(Path::new(&cwd).join(name), content).unwrap();
        let (info, actual) = read(&files, &cwd, name);
        assert_eq!(info.kind, kind);
        assert_eq!(info.mime_type, mime);
        assert_eq!(actual, content);
    }
    fs::write(
        Path::new(&cwd).join("utf8"),
        format!("{}界", "x".repeat(8191)),
    )
    .unwrap();
    assert_eq!(read(&files, &cwd, "utf8").0.kind, FileKind::Text);
    assert!(files.open(&cwd, ".").is_err());
    assert!(matches!(files.version(&cwd, "."), FileVersion::Error(_)));
}

#[test]
fn open_reader_detects_grow_shrink_and_overwrite() {
    let (_temp, files, cwd) = fixture();
    let path = Path::new(&cwd).join("file");
    for changed in ["longer", "x", "other"] {
        fs::write(&path, "start").unwrap();
        let reader = files.open(&cwd, "file").unwrap();
        fs::write(&path, changed).unwrap();
        assert!(reader.verify().is_err());
    }
}

#[test]
fn creates_files_and_directories_and_rejects_invalid_names() {
    let (_temp, files, cwd) = fixture();
    assert_eq!(
        files
            .create(&cwd, ".", " folder ", EntryKind::Directory)
            .unwrap(),
        "folder"
    );
    assert_eq!(
        files
            .create(&cwd, "folder", "a.txt", EntryKind::File)
            .unwrap(),
        "folder/a.txt"
    );
    assert_eq!(
        files
            .create(&cwd, "folder", "a.txt", EntryKind::File)
            .unwrap_err()
            .0,
        "\"a.txt\" already exists"
    );
    for name in ["", ".", "..", "a/b", "a\\b", "\0"] {
        assert!(files.create(&cwd, ".", name, EntryKind::File).is_err());
    }
    assert!(
        files
            .create(&cwd, "folder/a.txt", "child", EntryKind::File)
            .is_err()
    );
    assert!(files.list(&cwd, "folder/a.txt").is_err());
}

#[test]
fn duplicates_siblings_with_extensions_and_nested_directories() {
    let (_temp, files, cwd) = fixture();
    fs::write(Path::new(&cwd).join("a.txt"), "original").unwrap();
    assert_eq!(files.duplicate(&cwd, "a.txt").unwrap(), "a copy.txt");
    assert_eq!(files.duplicate(&cwd, "a.txt").unwrap(), "a copy 2.txt");
    files
        .create(&cwd, ".", "folder", EntryKind::Directory)
        .unwrap();
    fs::write(Path::new(&cwd).join("folder/a"), "nested").unwrap();
    assert_eq!(files.duplicate(&cwd, "folder").unwrap(), "folder copy");
    assert_eq!(read(&files, &cwd, "folder copy/a").1, b"nested");
    assert!(files.duplicate(&cwd, ".").is_err());
    assert!(files.duplicate(&cwd, "missing").is_err());
}

#[test]
fn renames_tracked_files_with_git_and_untracked_files_without_git() {
    let (_temp, files, cwd) = fixture();
    let root = Path::new(&cwd);
    crate::local::git::run(root, &["init", "--quiet"]).unwrap();
    fs::write(root.join("tracked"), "tracked").unwrap();
    crate::local::git::run(root, &["add", "tracked"]).unwrap();
    assert_eq!(files.rename(&cwd, "tracked", "renamed").unwrap(), "renamed");
    assert_eq!(
        crate::local::git::run(root, &["ls-files"]).unwrap(),
        "renamed"
    );
    fs::write(root.join("untracked"), "plain").unwrap();
    assert_eq!(
        files.rename(&cwd, "untracked", "Untracked").unwrap(),
        "Untracked"
    );
    assert!(files.rename(&cwd, "renamed", "Untracked").is_err());
    assert!(files.rename(&cwd, ".", "root").is_err());
    assert!(files.rename(&cwd, "missing", "new").is_err());
    assert!(files.rename(&cwd, "renamed", "../bad").is_err());
}

#[test]
fn deletes_entries_but_rejects_root_outside_and_missing() {
    let (_temp, files, cwd) = fixture();
    files
        .create(&cwd, ".", "folder", EntryKind::Directory)
        .unwrap();
    files
        .create(&cwd, "folder", "child", EntryKind::File)
        .unwrap();
    files.delete(&cwd, "folder").unwrap();
    assert!(files.delete(&cwd, "folder").is_err());
    for path in [".", "../outside", "/"] {
        assert!(files.delete(&cwd, path).is_err());
    }
}

#[test]
fn search_filters_and_exact_retrieval_bypasses_discovery_filters() {
    let (_temp, files, cwd) = fixture();
    for directory in [
        "src",
        "src/nested",
        "node_modules",
        ".github",
        ".ait",
        ".ait-server",
    ] {
        fs::create_dir_all(Path::new(&cwd).join(directory)).unwrap();
    }
    for file in [
        "src/lib.rs",
        "node_modules/hidden.rs",
        ".github/workflow.yml",
        ".ait/settings.json",
        ".ait-server/config.json",
        ".hidden",
    ] {
        fs::write(Path::new(&cwd).join(file), "content").unwrap();
    }
    let mut request = FileSearch {
        cwd: Some(cwd),
        query: "lib".to_owned(),
        include_files: true,
        include_directories: true,
        suffix: false,
        limit: 30,
    };
    assert_eq!(
        files.search(&request).unwrap(),
        [("src/lib.rs".to_owned(), EntryKind::File)]
    );
    request.query.clear();
    assert_eq!(
        files.search(&request).unwrap(),
        [("src".to_owned(), EntryKind::Directory)]
    );
    request.query = "workflow".to_owned();
    assert_eq!(files.search(&request).unwrap()[0].0, ".github/workflow.yml");
    request.query = "settings".to_owned();
    assert_eq!(files.search(&request).unwrap()[0].0, ".ait/settings.json");
    request.query = "config".to_owned();
    assert_eq!(
        files.search(&request).unwrap()[0].0,
        ".ait-server/config.json"
    );
    request.query = "node_modules/hidden.rs".to_owned();
    request.suffix = true;
    request.limit = 1;
    assert_eq!(
        files.search(&request).unwrap()[0].0,
        "node_modules/hidden.rs"
    );
    request.query = "../outside".to_owned();
    assert!(files.search(&request).unwrap().is_empty());
    request.cwd = None;
    request.query.clear();
    assert!(files.search(&request).unwrap().is_empty());
}

#[cfg(unix)]
#[test]
fn symlinks_remain_scoped_and_deletion_removes_only_the_link() {
    use std::os::unix::fs::symlink;
    let (temp, files, cwd) = fixture();
    fs::write(Path::new(&cwd).join("inside"), "inside").unwrap();
    fs::write(temp.path().join("outside"), "outside").unwrap();
    symlink("inside", Path::new(&cwd).join("alias")).unwrap();
    symlink(temp.path().join("outside"), Path::new(&cwd).join("escape")).unwrap();
    symlink("absent", Path::new(&cwd).join("dangling")).unwrap();
    symlink(temp.path(), Path::new(&cwd).join("outside-dir")).unwrap();
    assert_eq!(read(&files, &cwd, "alias").1, b"inside");
    assert!(files.open(&cwd, "escape").is_err());
    assert!(files.open(&cwd, "outside-dir/new").is_err());
    let names: Vec<_> = files
        .list(&cwd, ".")
        .unwrap()
        .1
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    assert_eq!(names.len(), 2);
    files.duplicate(&cwd, "alias").unwrap();
    assert!(
        fs::symlink_metadata(Path::new(&cwd).join("alias copy"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    files.delete(&cwd, "alias").unwrap();
    assert!(Path::new(&cwd).join("inside").exists());
}

#[test]
fn upload_persists_only_complete_files_and_sanitizes_names() {
    let (_temp, files, _cwd) = fixture();
    let metadata = UploadedFile {
        id: "upload_test".to_owned(),
        file_name: "../../evil?file.txt".to_owned(),
        mime_type: "text/plain".to_owned(),
        size: 3,
        path: String::new(),
    };
    let mut upload = files.upload(metadata.clone()).unwrap();
    upload.write_all(b"abc").unwrap();
    let result = upload.finish().unwrap();
    assert_eq!(result.file_name, "evil_file.txt");
    assert_eq!(fs::read(&result.path).unwrap(), b"abc");
    let mut unfinished = files.upload(metadata.clone()).unwrap();
    unfinished.write_all(b"a").unwrap();
    assert!(unfinished.finish().is_err());
    let mut oversized = files.upload(metadata.clone()).unwrap();
    assert!(oversized.write_all(b"toolong").is_err());
    drop(oversized);
    drop(files.upload(metadata).unwrap());
    assert_eq!(fs::read_dir(&files.uploads).unwrap().count(), 1);
}

#[test]
fn tilde_and_absolute_paths_are_checked_against_the_root() {
    let (_temp, files, cwd) = fixture();
    fs::write(Path::new(&cwd).join("inside"), "inside").unwrap();
    assert_eq!(read(&files, &cwd, "~/workspace/inside").1, b"inside");
    assert_eq!(read(&files, "~", "workspace/inside").1, b"inside");
    assert!(files.open(&cwd, "~/outside").is_err());
    assert!(files.open("", "inside").is_err());
}

#[test]
fn classification_scans_beyond_the_first_block_and_preserves_utf8_boundaries() {
    let (_temp, files, cwd) = fixture();
    let path = Path::new(&cwd).join("large");
    fs::write(&path, format!("{}界", "a".repeat(256 * 1024 - 1))).unwrap();
    assert_eq!(read(&files, &cwd, "large").0.kind, FileKind::Text);
    let mut bytes = vec![b'a'; 300_000];
    bytes.push(0);
    fs::write(&path, &bytes).unwrap();
    assert_eq!(read(&files, &cwd, "large").0.kind, FileKind::Binary);
    bytes.pop();
    bytes.extend_from_slice(&[0xe7, 0x95]);
    fs::write(&path, &bytes).unwrap();
    assert_eq!(read(&files, &cwd, "large").0.kind, FileKind::Binary);
    fs::write(&path, [1; 100]).unwrap();
    assert_eq!(read(&files, &cwd, "large").0.kind, FileKind::Binary);
}

#[cfg(unix)]
#[test]
fn atomic_edit_preserves_executable_bits_and_legacy_timestamp_guard() {
    use std::os::unix::fs::PermissionsExt;
    let (_temp, files, cwd) = fixture();
    let path = Path::new(&cwd).join("script");
    fs::write(&path, "echo before").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o751)).unwrap();
    let (info, _) = read(&files, &cwd, "script");
    let mut request = edit(&cwd, "script", &info, "echo after");
    request.expected_revision = None;
    assert!(matches!(
        files.write(&request).unwrap(),
        FileWritten::Written(_)
    ));
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o751
    );
    request.expected_modified_at = "stale".to_owned();
    assert!(matches!(
        files.write(&request).unwrap(),
        FileWritten::Conflict(_)
    ));
}

#[test]
fn directory_sorting_uses_newest_time_then_name() {
    let (_temp, files, cwd) = fixture();
    for name in ["b", "a", "newer"] {
        let file = File::create(Path::new(&cwd).join(name)).unwrap();
        let seconds = if name == "newer" { 20 } else { 10 };
        file.set_times(
            fs::FileTimes::new()
                .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(seconds)),
        )
        .unwrap();
    }
    let names: Vec<_> = files
        .list(&cwd, ".")
        .unwrap()
        .1
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    assert_eq!(names, ["newer", "a", "b"]);
}
