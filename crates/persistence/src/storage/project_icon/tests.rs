use super::*;

const PNG_1X1: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x00,
];

fn raster_headers() -> Vec<(&'static str, Vec<u8>)> {
    let mut jpeg = vec![
        0xff, 0xd8, 0xff, 0xe0, 0, 2, 0, 0xff, 0xc0, 0, 17, 8, 0, 1, 0, 1,
    ];
    jpeg.extend_from_slice(&[0; 8]);
    let mut lossy = vec![0; 30];
    lossy[..4].copy_from_slice(b"RIFF");
    lossy[8..16].copy_from_slice(b"WEBPVP8 ");
    lossy[26] = 1;
    lossy[28] = 1;
    let mut lossless = lossy.clone();
    lossless[12..16].copy_from_slice(b"VP8L");
    let mut ico = vec![0; 22];
    ico[..6].copy_from_slice(&[0, 0, 1, 0, 1, 0]);
    ico[6] = 1;
    ico[7] = 1;
    vec![
        ("image/png", PNG_1X1.to_vec()),
        ("image/gif", b"GIF87a\x01\0\x01\0".to_vec()),
        ("image/gif", b"GIF89a\x01\0\x01\0".to_vec()),
        ("image/jpeg", jpeg),
        ("image/webp", lossy),
        ("image/webp", lossless),
        ("image/x-icon", ico),
    ]
}

#[test]
fn supported_raster_headers_preserve_mime_and_reject_missing_dimensions() {
    for (mime, bytes) in raster_headers() {
        let icon = validate_custom(&bytes).unwrap();
        assert_eq!(icon.mime_type, mime);
        assert_eq!(icon.bytes, bytes);
        assert_eq!(
            validate_automatic(&bytes, Path::new("favicon.png")),
            Some(icon)
        );
        assert!(validate_custom(&bytes[..6]).is_err(), "{mime}");
    }
    let mut invalid_jpeg = vec![0xff, 0xd8, 0xff, 0xe0, 0, 0];
    invalid_jpeg.extend_from_slice(&[0; 10]);
    assert!(validate_custom(&invalid_jpeg).is_err());
    let mut unsupported_webp = vec![0; 30];
    unsupported_webp[..4].copy_from_slice(b"RIFF");
    unsupported_webp[8..16].copy_from_slice(b"WEBP????");
    assert!(validate_custom(&unsupported_webp).is_err());
    for dimension in [0_u32, 1025] {
        let mut png = PNG_1X1.to_vec();
        png[16..20].copy_from_slice(&dimension.to_be_bytes());
        png[20..24].copy_from_slice(&dimension.to_be_bytes());
        assert!(validate_custom(&png).is_err());
    }
    assert!(validate_automatic(b"not svg", Path::new("favicon.svg")).is_none());
    assert!(validate_automatic(b"<svg/>", Path::new("favicon.txt")).is_none());
}

#[test]
fn discovery_searches_monorepos_and_bounds_recursive_and_oversized_candidates() {
    let fixture = tempfile::tempdir().unwrap();
    let root = fixture.path();
    let store = LocalProjectIconStore::new(root.join("icons"));
    for ignored in ["public/node_modules", "public/one/two/three"] {
        let directory = root.join(ignored);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("favicon.png"), PNG_1X1).unwrap();
    }
    assert!(
        store
            .find_automatic(root.to_str().unwrap())
            .unwrap()
            .is_none()
    );
    for relative in [
        "packages/web/public/nested/favicon-theme.png",
        "apps/web/icon.png",
    ] {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, PNG_1X1).unwrap();
        assert_eq!(find_automatic_path(root), Some(path.clone()));
        assert_eq!(
            store
                .find_automatic(root.to_str().unwrap())
                .unwrap()
                .unwrap()
                .mime_type,
            "image/png"
        );
        std::fs::remove_file(&path).unwrap();
    }
    let path = root.join("favicon.svg");
    std::fs::write(
        &path,
        vec![b' '; usize::try_from(MAX_AUTOMATIC_BYTES).unwrap() + 1],
    )
    .unwrap();
    assert!(
        store
            .find_automatic(root.to_str().unwrap())
            .unwrap()
            .is_none()
    );
    std::fs::write(&path, "\u{feff}  <svg/>").unwrap();
    assert_eq!(
        store
            .find_automatic(root.to_str().unwrap())
            .unwrap()
            .unwrap()
            .mime_type,
        "image/svg+xml"
    );
    assert_eq!(
        store.find_automatic(path.to_str().unwrap()),
        Err(ProjectIconStoreError::Io)
    );
}

#[test]
fn custom_icon_storage_distinguishes_missing_corrupt_and_failed_io() {
    let fixture = tempfile::tempdir().unwrap();
    let store = LocalProjectIconStore::new(fixture.path().join("icons"));
    store.remove_custom("missing").unwrap();
    assert!(store.read_custom("missing").unwrap().is_none());
    std::fs::create_dir_all(&store.icon_directory).unwrap();
    std::fs::write(store.path("corrupt"), b"broken").unwrap();
    assert!(store.read_custom("corrupt").unwrap().is_none());
    std::fs::create_dir(store.path("blocked")).unwrap();
    assert_eq!(store.read_custom("blocked"), Err(ProjectIconStoreError::Io));
    assert_eq!(
        store.remove_custom("blocked"),
        Err(ProjectIconStoreError::Io)
    );
    assert_eq!(
        store.write_custom("blocked", PNG_1X1),
        Err(ProjectIconStoreError::Io)
    );
    let bad_root = fixture.path().join("file");
    std::fs::write(&bad_root, b"file").unwrap();
    assert_eq!(
        LocalProjectIconStore::new(bad_root).write_custom("id", PNG_1X1),
        Err(ProjectIconStoreError::Io)
    );
}

#[test]
fn stores_reads_and_removes_validated_custom_icons() {
    let fixture = tempfile::tempdir().unwrap();
    let store = LocalProjectIconStore::new(fixture.path().join("icons"));
    store.write_custom("prj_a", PNG_1X1).unwrap();
    let icon = store.read_custom("prj_a").unwrap().unwrap();
    assert_eq!(icon.bytes, PNG_1X1);
    assert_eq!(icon.mime_type, "image/png");
    store.remove_custom("prj_a").unwrap();
    assert!(store.read_custom("prj_a").unwrap().is_none());
}

#[test]
fn rejects_unsupported_non_square_and_oversized_custom_icons() {
    let fixture = tempfile::tempdir().unwrap();
    let store = LocalProjectIconStore::new(fixture.path().join("icons"));
    assert_eq!(
        store.write_custom("prj_a", b"not an image"),
        Err(ProjectIconStoreError::Invalid)
    );
    let mut wide = PNG_1X1.to_vec();
    wide[19] = 2;
    assert_eq!(
        store.write_custom("prj_a", &wide),
        Err(ProjectIconStoreError::Invalid)
    );
    assert_eq!(
        store.write_custom("prj_a", &vec![0; MAX_CUSTOM_BYTES + 1]),
        Err(ProjectIconStoreError::Invalid)
    );
}

#[test]
fn automatic_discovery_prefers_priority_directories_and_accepts_svg() {
    let fixture = tempfile::tempdir().unwrap();
    let public = fixture.path().join("public");
    std::fs::create_dir(&public).unwrap();
    std::fs::write(fixture.path().join("favicon.png"), PNG_1X1).unwrap();
    std::fs::write(
        public.join("favicon.svg"),
        "<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>",
    )
    .unwrap();
    let store = LocalProjectIconStore::new(fixture.path().join("icons"));
    let icon = store
        .find_automatic(fixture.path().to_str().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(icon.mime_type, "image/svg+xml");
}
