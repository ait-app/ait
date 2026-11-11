use std::io::Read;

use super::*;
use crate::files::ports::{
    EntryKind, FileEntry, FileKind, FileSearch, FileUpload, FileVersion, FileWrite, FileWritten,
    UploadedFile,
};

#[derive(Debug)]
struct MemoryFiles;

#[derive(Debug)]
struct Reader {
    info: FileInfo,
    bytes: std::io::Cursor<Vec<u8>>,
}

impl Read for Reader {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        self.bytes.read(bytes)
    }
}
impl FileReader for Reader {
    fn info(&self) -> &FileInfo {
        &self.info
    }
    fn verify(&self) -> Result<(), FileError> {
        Ok(())
    }
}

impl FileSystem for MemoryFiles {
    fn list(&self, _: &str, _: &str) -> Result<(String, Vec<FileEntry>), FileError> {
        Err(FileError("unused".to_owned()))
    }
    fn open(&self, _: &str, path: &str) -> Result<Box<dyn FileReader>, FileError> {
        if path == "missing" {
            return Err(FileError("missing".to_owned()));
        }
        Ok(Box::new(Reader {
            info: FileInfo {
                root: "/repo".to_owned(),
                absolute_path: format!("/repo/{}", path.strip_prefix("/repo/").unwrap_or(path)),
                path: path.to_owned(),
                file_name: "a".to_owned(),
                mime_type: "text/plain".to_owned(),
                kind: FileKind::Text,
                size: 3,
                modified_at: "now".to_owned(),
                revision: "v1".to_owned(),
            },
            bytes: std::io::Cursor::new(b"abc".to_vec()),
        }))
    }
    fn version(&self, _: &str, _: &str) -> FileVersion {
        FileVersion::Missing
    }
    fn write(&self, _: &FileWrite) -> Result<FileWritten, FileError> {
        Err(FileError("unused".to_owned()))
    }
    fn create(&self, _: &str, _: &str, _: &str, _: EntryKind) -> Result<String, FileError> {
        Err(FileError("unused".to_owned()))
    }
    fn rename(&self, _: &str, _: &str, _: &str) -> Result<String, FileError> {
        Err(FileError("unused".to_owned()))
    }
    fn duplicate(&self, _: &str, _: &str) -> Result<String, FileError> {
        Err(FileError("unused".to_owned()))
    }
    fn delete(&self, _: &str, _: &str) -> Result<(), FileError> {
        Err(FileError("unused".to_owned()))
    }
    fn search(&self, _: &FileSearch) -> Result<Vec<(String, EntryKind)>, FileError> {
        Err(FileError("unused".to_owned()))
    }
    fn upload(&self, _: UploadedFile) -> Result<Box<dyn FileUpload>, FileError> {
        Err(FileError("unused".to_owned()))
    }
}

#[test]
fn tokens_are_single_use_and_expiration_is_checked_before_opening() {
    let mut files = Files::new(Box::new(MemoryFiles));
    files
        .issue_download("token".to_owned(), "/repo", "a")
        .unwrap();
    assert_eq!(
        files.consume_download("token", None).unwrap().0.info().size,
        3
    );
    assert!(files.consume_download("token", None).is_err());
    files.downloads.insert(
        "expired".to_owned(),
        DownloadGrant {
            expires: Instant::now().checked_sub(Duration::from_secs(1)).unwrap(),
            source: DownloadSource::File {
                root: "/repo".to_owned(),
                path: "a".to_owned(),
            },
            file_name: "a".to_owned(),
            visitor: None,
            expiry_guard: None,
        },
    );
    assert!(files.consume_download("expired", None).is_err());
    assert!(files.downloads.is_empty());
}

#[test]
fn bound_grants_serve_only_their_visitor() {
    let mut files = Files::new(Box::new(MemoryFiles));
    files
        .issue_download("guest".to_owned(), "/repo", "a")
        .unwrap();
    assert!(files.bind_download("guest", "bob"));
    // Binding is one-shot; unknown grants cannot be bound.
    assert!(!files.bind_download("guest", "carol"));
    assert!(!files.bind_download("missing", "bob"));
    // Another visitor and the owner path are refused and leave the grant usable.
    assert!(files.consume_download("guest", Some("carol")).is_err());
    assert!(files.consume_download("guest", None).is_err());
    assert!(files.consume_download("guest", Some("bob")).is_ok());

    // An owner's grant serves no visitor.
    files
        .issue_download("owner".to_owned(), "/repo", "a")
        .unwrap();
    assert!(files.consume_download("owner", Some("bob")).is_err());
    assert!(files.consume_download("owner", None).is_ok());
}

#[test]
fn issuing_rejects_missing_paths_reused_ids_and_capacity_overflow() {
    let mut files = Files::new(Box::new(MemoryFiles));
    assert!(
        files
            .issue_download("bad".to_owned(), "root", "missing")
            .is_err()
    );
    assert!(files.downloads.is_empty());
    for index in 0..256 {
        files
            .issue_download(index.to_string(), "root", "a")
            .unwrap();
    }
    assert!(files.issue_download("256".to_owned(), "root", "a").is_err());
    assert!(files.issue_download("0".to_owned(), "root", "a").is_err());
}

fn artifact() -> model::download::GeneratedArtifact {
    use std::io::Write;
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(b"saved").unwrap();
    file.rewind().unwrap();
    model::download::GeneratedArtifact {
        agent_id: "agent".to_owned(),
        file_name: "timeline.md".to_owned(),
        mime_type: "text/markdown".to_owned(),
        size: 5,
        file,
    }
}

#[test]
fn generated_artifacts_keep_visitor_binding_single_use_and_transfer_capacity() {
    let mut files = Files::new(Box::new(MemoryFiles));
    let finished = files
        .issue_artifact("export".to_owned(), artifact())
        .unwrap();
    assert!(files.bind_download("export", "visitor"));
    assert!(files.consume_download("export", None).is_err());
    assert!(!finished.is_cancelled());
    let (mut reader, name) = files.consume_download("export", Some("visitor")).unwrap();
    assert!(finished.is_cancelled());
    assert_eq!(name, "timeline.md");
    assert_eq!(files.generated_slots.available_permits(), 7);
    let mut content = String::new();
    reader.read_to_string(&mut content).unwrap();
    reader.verify().unwrap();
    assert_eq!(content, "saved");
    assert!(files.consume_download("export", Some("visitor")).is_err());
    drop(reader);
    assert_eq!(files.generated_slots.available_permits(), 8);
}

#[test]
fn generated_artifact_cancel_expiry_and_dropped_transfer_release_owned_files() {
    let mut files = Files::new(Box::new(MemoryFiles));
    let finished = files
        .issue_artifact("cancel".to_owned(), artifact())
        .unwrap();
    assert!(!files.release_artifact("other", "cancel"));
    assert!(files.release_artifact("agent", "cancel"));
    assert!(finished.is_cancelled());
    assert!(!files.release_artifact("agent", "cancel"));
    assert_eq!(files.generated_slots.available_permits(), 8);
    let expired = files
        .issue_artifact("expired".to_owned(), artifact())
        .unwrap();
    files.downloads.get_mut("expired").unwrap().expires =
        Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
    assert!(files.consume_download("expired", None).is_err());
    assert!(expired.is_cancelled());
    for index in 0..8 {
        files.issue_artifact(index.to_string(), artifact()).unwrap();
    }
    assert!(
        files
            .issue_artifact("ninth".to_owned(), artifact())
            .is_err()
    );
    let reader = files.consume_download("0", None).unwrap();
    assert!(
        files
            .issue_artifact("still-full".to_owned(), artifact())
            .is_err()
    );
    drop(reader);
    assert!(
        files
            .issue_artifact("after-abort".to_owned(), artifact())
            .is_ok()
    );
}

#[test]
fn generated_artifacts_reject_false_size_position_and_oversized_files() {
    let mut files = Files::new(Box::new(MemoryFiles));
    let mut wrong = artifact();
    wrong.size = 0;
    assert!(files.issue_artifact("wrong".to_owned(), wrong).is_err());
    let mut large = artifact();
    large.size = model::download::MAX_ARTIFACT_BYTES + 1;
    large.file.set_len(large.size).unwrap();
    assert!(files.issue_artifact("large".to_owned(), large).is_err());
    let mut offset = artifact();
    offset.file.seek(std::io::SeekFrom::Start(1)).unwrap();
    assert!(files.issue_artifact("offset".to_owned(), offset).is_err());
    assert_eq!(files.generated_slots.available_permits(), 8);
}
