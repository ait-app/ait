use std::fmt::Write as _;

use super::*;

#[test]
fn single_user_image_projects_a_source_without_a_placeholder() {
    let root = tempfile::tempdir().unwrap();
    let images = crate::local::images::ImageStore::new(root.path().join("images"));
    let native = json!({"type":"userMessage","id":"user","content":[
        {"type":"text","text":"Inspect"},
        {"type":"image","url":"data:image/png;base64,aGVsbG8="}]});

    let entries = timeline_items(&native, "turn", "time", &images).unwrap();

    assert_eq!(entries[0].item["text"], "Inspect");
    assert_eq!(entries[0].item["imageSources"].as_array().unwrap().len(), 1);
}

#[test]
fn tiled_user_image_projects_one_preview_and_only_user_authored_text() {
    let root = tempfile::tempdir().unwrap();
    let images = crate::local::images::ImageStore::new(root.path().join("images"));
    let mut content = vec![json!({"type":"text","text":"这是什么植物？"})];
    content.extend((0..10).map(|_| json!({"type":"image","url":"data:image/png;base64,aGVsbG8="})));
    let mut layout = "Image layout: overviews show the full image; detail crops preserve readable pixels and overlap. Crop coordinates refer to the top-left of the original image.\nImage 1: original image 1: overview of the full 4284x5712 image".to_owned();
    for index in 2..=10 {
        write!(layout, "\nImage {index}: original image 1: detail crop x=0, y=0, width=1514, height=1990; original pixel resolution").unwrap();
    }
    content.push(json!({"type":"text","text":layout}));
    let native = json!({"type":"userMessage","id":"user","clientId":"mobile-message",
        "content":content});

    let entries = timeline_items(&native, "turn", "time", &images).unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].item["text"], "这是什么植物？");
    assert_eq!(entries[0].item["clientMessageId"], "mobile-message");
    assert_eq!(entries[0].item["imageSources"].as_array().unwrap().len(), 1);
    assert!(!entries[0].item.to_string().contains("aGVsbG8="));
    assert_eq!(
        entries,
        timeline_items(&native, "turn", "time", &images).unwrap()
    );
    let timeline = crate::storage::timeline::Timeline::memory().unwrap();
    let old = timeline_item(&native, "turn", "time").unwrap().unwrap();
    timeline.append("agent", "codex", &[old]).unwrap();
    timeline.reconcile("agent", "codex", &entries).unwrap();
    assert_eq!(timeline.read("agent").unwrap().1[0].entry, entries[0]);
}
