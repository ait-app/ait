use super::*;

fn layout(descriptions: &str) -> String {
    format!("{LAYOUT_HEADER}\n{descriptions}")
}

#[test]
fn grouping_retains_each_original_and_images_without_descriptions() {
    let text = layout(
        "Image 2: original image 2: overview of the full 4284x5712 image\nImage 3: original image 2: detail crop x=0, y=0, width=1514, height=1990; original pixel resolution\nImage 5: original image 4: overview of the full 8000x800 image\nImage 6: original image 4: detail crop x=10, y=0, width=2048, height=800; original pixel resolution",
    );
    assert_eq!(hidden_crops(&text, 7), Some(BTreeSet::from([3, 6])));
}

#[test]
fn incomplete_or_unrelated_layout_text_is_never_hidden() {
    let valid = "Image 1: original image 1: overview of the full 4284x5712 image\nImage 2: original image 1: detail crop x=0, y=0, width=1514, height=1990; original pixel resolution";
    for note in [
        "Image layout: an explanation written by the user".to_owned(),
        layout(""),
        layout("Image 1: original image 1: overview of the full 4284x5712 image"),
        layout(&valid.replace("Image 2:", "Image 3:")),
        layout(&valid.replace("Image 2:", "Image 1:")),
        layout(&valid.replace("original image 1: detail", "original image 2: detail")),
        layout(&valid.replace("width=1514", "width=0")),
        layout(&valid.replace("x=0", "x=bad")),
        layout(&format!("{valid}\nUser text")),
        format!("Quoted example:\n{}", layout(valid)),
    ] {
        assert_eq!(hidden_crops(&note, 2), None, "{note}");
    }
    assert_eq!(hidden_crops(&layout(valid), 1), None);
    assert_eq!(hidden_crops(&layout(valid), 2), Some(BTreeSet::from([2])));
}

#[test]
fn projection_preserves_local_paths_order_and_unrelated_text() {
    let native = json!({"id":"user","content":[
        {"type":"text","text":"Image layout: keep my words"},
        {"type":"localImage","path":"/tmp/photo (1).png"},
        {"type":"image","url":"https://example.test/second.jpg"},
        {"type":"text","text":"Context"}]});

    let item = project(&native, &ImageStore::default()).unwrap();

    assert_eq!(item["text"], "Image layout: keep my words\nContext");
    assert_eq!(
        item["imageSources"],
        json!(["/tmp/photo (1).png", "https://example.test/second.jpg"])
    );
}

#[test]
fn legacy_images_without_a_source_keep_an_honest_placeholder() {
    let item = project(
        &json!({"id":"user","content":[{"type":"image"}]}),
        &ImageStore::default(),
    )
    .unwrap();
    assert_eq!(item["text"], "[Image attachment]");
    assert!(item.get("imageSources").is_none());
}
