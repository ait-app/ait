use super::*;

#[tokio::test]
async fn socket_is_private_and_removed_when_its_owner_drops() {
    let root = tempfile::tempdir_in("/tmp").unwrap();
    let path = root.path().join("mcp.sock");
    let socket = McpSocket::bind(&path).await.unwrap();
    let metadata = std::fs::symlink_metadata(&path).unwrap();
    assert!(metadata.file_type().is_socket());
    assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    drop(socket);
    assert!(!path.exists());
}

#[tokio::test]
async fn active_sockets_are_preserved_and_inactive_sockets_can_be_rebound() {
    let root = tempfile::tempdir_in("/tmp").unwrap();
    let path = root.path().join("mcp.sock");
    let original = UnixListener::bind(&path).unwrap();
    let inode = std::fs::symlink_metadata(&path).unwrap().ino();
    assert!(McpSocket::bind(&path).await.is_err());
    assert_eq!(std::fs::symlink_metadata(&path).unwrap().ino(), inode);
    drop(original);
    let rebound = McpSocket::bind(&path).await.unwrap();
    assert!(UnixStream::connect(&path).await.is_ok());
    drop(rebound);
    assert!(!path.exists());
}

#[tokio::test]
async fn binding_preserves_non_socket_paths_and_cleanup_preserves_replacements() {
    let root = tempfile::tempdir_in("/tmp").unwrap();
    let file = root.path().join("file");
    std::fs::write(&file, "keep me").unwrap();
    let link = root.path().join("link");
    std::os::unix::fs::symlink(&file, &link).unwrap();
    for path in [&file, &link, root.path()] {
        assert!(McpSocket::bind(path).await.is_err());
    }
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "keep me");
    assert!(std::fs::symlink_metadata(&link).unwrap().is_symlink());
    assert!(
        McpSocket::bind(&root.path().join("missing/mcp.sock"))
            .await
            .is_err()
    );

    let path = root.path().join("mcp.sock");
    let socket = McpSocket::bind(&path).await.unwrap();
    std::fs::remove_file(&path).unwrap();
    let replacement = UnixListener::bind(&path).unwrap();
    drop(socket);
    assert!(UnixStream::connect(&path).await.is_ok());
    drop(replacement);
}
