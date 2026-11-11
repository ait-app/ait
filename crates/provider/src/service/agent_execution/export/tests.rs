mod cards;

use std::io::Read;

use serde_json::{Value, json};

use super::*;
use crate::protocol::timeline::NativeItem;
use crate::storage::timeline::Row;

fn entry(item: Value) -> Entry {
    Entry::from(&Row {
        seq: 1,
        provider: "codex".to_owned(),
        entry: NativeItem {
            key: "saved".to_owned(),
            timestamp: "2026-12-01T00:00:00Z".to_owned(),
            turn_id: Some("turn".to_owned()),
            item,
        },
    })
}

fn content(format: Format, scope: Scope, entries: &[Entry]) -> (String, u32) {
    let mut export = render("agent", format, scope, entries, CancellationToken::new()).unwrap();
    let mut text = String::new();
    export.artifact.file.read_to_string(&mut text).unwrap();
    assert_eq!(export.artifact.size, text.len() as u64);
    assert!(
        export
            .artifact
            .file_name
            .ends_with(if format == Format::Html {
                ".html"
            } else {
                ".md"
            })
    );
    (text, export.entries)
}

fn saved() -> Vec<Entry> {
    let items = [
        json!({"type":"user_message","text":"**Question**\nsecond line", "imageSources":["/saved/image.png"]}),
        json!({"type":"assistant_message","text":"Readable answer"}),
        json!({"type":"reasoning","text":"Saved reasoning"}),
        json!({"type":"tool_call","callId":"call","name":"Shell","status":"completed","error":null,
            "detail":{"type":"shell","command":"printf '```'","output":"complete output","exitCode":0}}),
        json!({"type":"tool_call","callId":"host-setup","name":"Worktree setup","status":"completed","error":null,
            "detail":{"type":"worktree_setup","worktreePath":"/saved/worktree","branchName":"feature",
                "log":"Host setup complete","commands":[{"index":0,"command":"npm ci","cwd":"/saved/worktree",
                    "log":"Dependencies installed","status":"completed","exitCode":0}]}}),
        json!({"type":"todo","items":[{"text":"Done task","completed":true,"status":"completed"}]}),
        json!({"type":"plugin","id":"host-result","pluginId":"ait","kind":"Host result","version":1,
            "data":{"title":"Stable result","body":"Host-owned saved card","count":0}}),
        json!({"type":"compaction","status":"completed","trigger":"manual","preTokens":4000}),
        json!({"type":"notification","level":"warning","message":"Saved notice"}),
        json!({"type":"error","message":"Saved failure"}),
        json!({"type":"voice","id":"voice","sessionId":"session","kind":"transcript",
            "role":"assistant","text":"Spoken answer","status":"completed"}),
        json!({"type":"voice","id":"boundary","sessionId":"session","kind":"session","status":"ended"}),
    ].into_iter().enumerate().map(|(index, item)| NativeItem {
        key: format!("saved:{index}"), timestamp: "2026-12-01T00:00:00Z".to_owned(),
        turn_id: Some("turn".to_owned()), item,
    }).collect::<Vec<_>>();
    let timeline = crate::storage::timeline::Timeline::memory().unwrap();
    timeline.append("agent", "codex", &items).unwrap();
    let rows = timeline
        .export_rows("agent", &CancellationToken::new())
        .unwrap();
    crate::timeline_projection::project(&rows)
}

#[test]
fn markdown_has_real_conversation_and_readable_cards_without_json_dumping_known_items() {
    let (markdown, count) = content(Format::Markdown, Scope::Timeline, &saved());
    assert_eq!(count, 12);
    for expected in [
        "## User\n",
        "**Question**\nsecond line",
        "## Assistant\n",
        "Readable answer",
        "## Shell\n",
        "**Command:**",
        "complete output",
        "- [x] Done task",
        "## Host result\n",
        "Stable result",
        "Host-owned saved card",
        "## Worktree setup\n",
        "Host setup complete",
        "Dependencies installed",
        "## Context compaction\n",
        "4000",
        "Saved notice",
        "Saved failure",
        "Spoken answer",
        "Image references",
        "/saved/image\\.png",
    ] {
        assert!(
            markdown.contains(expected),
            "Missing readable content: {expected}"
        );
    }
    assert!(!markdown.contains("```json"));
    assert!(markdown.contains("````\nprintf '```'\n````"));
}

#[test]
fn both_formats_apply_conversation_scope_and_retain_attachments_and_voice() {
    for format in [Format::Markdown, Format::Html] {
        let (text, count) = content(format, Scope::Conversation, &saved());
        assert_eq!(count, 3);
        for included in [
            "Question",
            "Readable answer",
            "Spoken answer",
            "Image references",
        ] {
            assert!(text.contains(included));
        }
        for excluded in [
            "Saved reasoning",
            "complete output",
            "Host-owned saved card",
            "Done task",
            "Saved failure",
        ] {
            assert!(!text.contains(excluded));
        }
    }
}

#[test]
fn html_is_semantic_readable_content_with_escaped_messages_fields_and_image_references() {
    let mut entries = saved();
    entries.push(entry(
        json!({"type":"user_message","text":"<script>alert('x')</script>\nA & B",
        "imageSources":["<img src=x onerror=alert(1)>"]}),
    ));
    entries.push(entry(
        json!({"type":"plugin","id":"result","pluginId":"ait","kind":"<unsafe title>",
        "version":1,"data":{"<field>":"<iframe src=https://external.invalid>"}}),
    ));
    let (html, count) = content(Format::Html, Scope::Timeline, &entries);
    assert_eq!(count, 14);
    assert!(html.contains("<h2>User</h2>"));
    assert!(html.contains("<p>Readable answer</p>"));
    assert!(html.contains("<dt>Command</dt>"));
    assert!(html.contains("<pre><code>printf"));
    assert!(html.contains("&lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt;"));
    assert!(html.contains("A &amp; B"));
    assert!(html.contains("&lt;iframe"));
    assert!(!html.contains("<script"));
    assert!(!html.contains("<iframe"));
    assert!(!html.contains("<img"));
    assert!(html.contains("default-src 'none'"));
}

#[test]
fn only_unknown_saved_types_use_structured_fallback() {
    let unknown = entry(json!({"type":"future_result",
        "payload":{"text":"preserved unknown ``` <script>中文</script>"}}));
    let (markdown, count) = content(
        Format::Markdown,
        Scope::Timeline,
        std::slice::from_ref(&unknown),
    );
    assert_eq!(count, 1);
    let saved_json = markdown
        .lines()
        .filter_map(|line| line.strip_prefix("    "))
        .collect::<Vec<_>>()
        .join("\n");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&saved_json).unwrap(),
        unknown.item
    );
    let (html, _) = content(Format::Html, Scope::Timeline, &[unknown]);
    assert!(html.contains("&lt;script&gt;中文&lt;/script&gt;"));
    assert!(!html.contains("<script>"));
}

#[test]
fn unknown_fallback_streaming_preserves_output_limit_errors() {
    let unknown = entry(json!({"type":"future_result","text":"&".repeat(4 * 1024 * 1024)}));
    assert!(matches!(
        render(
            "agent",
            Format::Html,
            Scope::Timeline,
            &[unknown],
            CancellationToken::new()
        ),
        Err(ErrorCode::ResourceExhausted)
    ));
}

#[test]
fn rendering_expansion_and_cancellation_fail_without_returning_partial_artifacts() {
    let expanding = entry(json!({"type":"assistant_message","text":"&".repeat(4 * 1024 * 1024)}));
    assert!(matches!(
        render(
            "agent",
            Format::Html,
            Scope::Timeline,
            &[expanding],
            CancellationToken::new()
        ),
        Err(ErrorCode::ResourceExhausted)
    ));
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    assert!(matches!(
        render(
            "agent",
            Format::Markdown,
            Scope::Timeline,
            &saved(),
            cancellation
        ),
        Err(ErrorCode::AgentIo)
    ));
}
