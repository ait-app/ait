use super::*;

#[test]
fn input_only_and_empty_transcript_remain_readable() {
    for tail in ["", "<transcript_delta></transcript_delta>"] {
        let text = format!("<realtime_delegation><input>hello</input>{tail}</realtime_delegation>");
        let (input, entries) = parse(&text).unwrap();
        assert_eq!(input, "hello");
        assert_eq!(entries, vec![json!({"role":"user","text":"hello"})]);
    }
}

#[test]
fn multiline_speech_decodes_xml_once_and_keeps_unknown_truncated_prefix() {
    let text = "<realtime_delegation>\n<source>transcript_tail_flush</source>\n<input></input>\n<transcript_delta>…truncated start\nuser: &lt;tag&gt; &amp; &amp;lt;\nsecond line\nassistant: done</transcript_delta>\n</realtime_delegation>";
    let (input, entries) = parse(text).unwrap();
    assert!(input.contains("<tag> & &lt;"));
    assert_eq!(
        entries,
        vec![
            json!({"role":"unknown","text":"…truncated start"}),
            json!({"role":"user","text":"<tag> & &lt;\nsecond line"}),
            json!({"role":"assistant","text":"done"}),
        ]
    );
}

#[test]
fn ordinary_examples_malformed_and_oversized_envelopes_are_preserved() {
    let envelope = "<realtime_delegation><input>hello</input></realtime_delegation>";
    for text in [
        "ordinary user text".to_owned(),
        format!("Explain this: {envelope}"),
        format!("```xml\n{envelope}\n```"),
        format!("{envelope} trailing text"),
        "<realtime_delegation><input>unfinished".to_owned(),
        "<realtime_delegation><input><tag></input></realtime_delegation>".to_owned(),
        "x".repeat(MAX_BYTES + 1),
        format!(
            "<realtime_delegation><input>hi</input><transcript_delta>{}</transcript_delta></realtime_delegation>",
            "user: hi\n".repeat(MAX_ENTRIES + 1)
        ),
    ] {
        let mut item = json!({"type":"user_message","text":text});
        project(&mut item);
        assert_eq!(item["text"], text);
        assert!(item.get("voiceTranscript").is_none());
    }
}

#[test]
fn text_envelopes_with_image_attachments_keep_the_original_message() {
    let mut item = json!({"type":"user_message", "text":"<realtime_delegation><input>hello</input></realtime_delegation>",
        "imageSources":["/host/photo.png"]});
    let original = item.clone();
    project(&mut item);
    assert_eq!(item, original);
}
