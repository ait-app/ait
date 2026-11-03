use super::*;

fn private_directory() -> tempfile::TempDir {
    let root = tempfile::tempdir_in("/tmp").unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    root
}

#[tokio::test]
async fn socket_is_private_and_removed_when_its_owner_drops() {
    let root = private_directory();
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
    let root = private_directory();
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
    let root = private_directory();
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

#[tokio::test]
async fn socket_rejects_accessible_or_symlinked_parent_before_binding() {
    let root = private_directory();
    let parent = root.path().join("shared");
    std::fs::create_dir(&parent).unwrap();
    let path = parent.join("mcp.sock");
    for mode in [0o755, 0o770, 0o777] {
        std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(mode)).unwrap();
        let error = McpSocket::bind(&path).await.unwrap_err();
        assert!(error.to_string().contains("mode 0700"));
        assert!(!path.exists());
    }
    std::fs::set_permissions(&parent, std::fs::Permissions::from_mode(0o700)).unwrap();
    let alias = root.path().join("alias");
    std::os::unix::fs::symlink(&parent, &alias).unwrap();
    assert!(McpSocket::bind(&alias.join("mcp.sock")).await.is_err());
    assert!(!path.exists());
    assert!(McpSocket::bind(&path).await.is_ok());
}
