use serde_json::json;

use super::*;

#[test]
fn text_blocks_are_joined_and_images_keep_their_order() {
    let blocks = vec![
        json!({"type":"text","text":"history"}),
        json!({"type":"image","data":"first","mimeType":"image/png"}),
        json!({"type":"text","text":"request"}),
        json!({"type":"image","data":"second","mimeType":"image/png"}),
    ];

    let (text, images) = split(blocks);

    assert_eq!(text, "history\n\nrequest");
    assert_eq!(images.len(), 2);
    assert_eq!(images[0]["data"], "first");
    assert_eq!(images[1]["data"], "second");
}

#[test]
fn prompts_without_text_produce_empty_text() {
    assert_eq!(text(&[json!({"type":"image","data":"only"})]), "");
}
