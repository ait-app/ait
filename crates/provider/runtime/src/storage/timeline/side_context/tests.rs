use super::*;
use provider_core::protocol::timeline::NativeItem;
use serde_json::json;

fn entry(id: &str, item: serde_json::Value) -> NativeItem {
    NativeItem {
        key: format!("native:{id}"),
        turn_id: Some("turn".to_owned()),
        timestamp: "2026-11-11T00:00:00Z".to_owned(),
        item,
    }
}

#[test]
fn snapshot_excludes_live_tail_and_raw_tools_and_never_mutates_main_history() {
    let timeline = Timeline::memory().unwrap();
    timeline
        .append(
            "agent",
            "omp",
            &[
                entry(
                    "u1",
                    json!({"type":"user_message", "text":"committed alpha"}),
                ),
                entry(
                    "tool",
                    json!({"type":"tool_call", "name":"Read",
            "detail":{"input":{"token":"private-raw-payload"}}}),
                ),
                entry(
                    "a1",
                    json!({"type":"assistant_message", "text":"answer api_key=fixture-secret"}),
                ),
            ],
        )
        .unwrap();
    timeline
        .progress(
            "agent",
            "omp",
            "delta1",
            &entry(
                "a2",
                json!({"type":"assistant_message", "text":"INFLIGHT-BETA"}),
            ),
        )
        .unwrap();
    let before = timeline.read("agent").unwrap();
    let (context, truncated) = timeline.side_context("agent", 4096).unwrap();
    let encoded = serde_json::to_string(&context).unwrap();
    assert!(truncated);
    assert_eq!(context[0].text, "committed alpha");
    assert_eq!(context[1].text, "Read");
    assert!(!encoded.contains("INFLIGHT-BETA"));
    assert!(!encoded.contains("private-raw-payload"));
    assert!(!encoded.contains("fixture-secret"));
    assert_eq!(timeline.read("agent").unwrap(), before);
}

#[test]
fn empty_snapshot_does_not_force_main_timeline_registration() {
    let timeline = Timeline::memory().unwrap();
    assert_eq!(
        timeline.side_context("absent", 4096).unwrap(),
        (Vec::new(), false)
    );
    let count: i64 = timeline
        .database
        .lock()
        .unwrap()
        .query_row("SELECT count(*) FROM timelines", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn byte_budget_counts_json_escaping_and_keeps_a_readable_utf8_prefix() {
    let timeline = Timeline::memory().unwrap();
    timeline
        .append(
            "agent",
            "omp",
            &[
                entry("u1", json!({"type":"user_message", "text":"older"})),
                entry(
                    "a1",
                    json!({"type":"assistant_message", "text":"\"\\世界\n".repeat(2000)}),
                ),
            ],
        )
        .unwrap();
    let (context, truncated) = timeline.side_context("agent", 1000).unwrap();
    assert!(truncated);
    assert_eq!(context.len(), 1);
    assert!(context[0].text.starts_with("\"\\世界\n"));
    assert!(serde_json::to_vec(&context).unwrap().len() <= 1002);
}

#[test]
fn user_context_projects_current_and_legacy_input_envelopes_before_redaction() {
    use domain::input_envelope::{bot_instructions, bot_wake, schedule};

    let inputs = [
        (
            bot_instructions("hidden bot instructions", "visible api_key=fixture-secret"),
            "visible api_key=[redacted]",
        ),
        (
            schedule("schedule", "run", Some("hidden metadata"), "check </div>"),
            "check </div>",
        ),
        (bot_wake("notice </div>"), "notice </div>"),
        (
            "<ait-bot-instructions>\nhidden legacy instructions\n</ait-bot-instructions>\n\nlegacy user"
                .to_owned(),
            "legacy user",
        ),
        (
            "<ait-system>\nSchedule fired (id=schedule, run=run).\nlegacy schedule\n</ait-system>"
                .to_owned(),
            "legacy schedule",
        ),
        (
            "<ait-notifications>\nlegacy <\\/div>\n</ait-notifications>".to_owned(),
            "legacy </div>",
        ),
    ];
    let timeline = Timeline::memory().unwrap();
    let items = inputs
        .iter()
        .enumerate()
        .map(|(index, (input, _))| {
            entry(
                &index.to_string(),
                json!({"type":"user_message", "text":input}),
            )
        })
        .collect::<Vec<_>>();
    timeline.append("agent", "fixture", &items).unwrap();

    let (context, truncated) = timeline.side_context("agent", 4096).unwrap();

    assert!(!truncated);
    assert_eq!(context.len(), inputs.len());
    for (actual, (_, expected)) in context.iter().zip(&inputs) {
        assert_eq!(actual.role, "user");
        assert_eq!(&actual.text, expected);
    }
}

#[test]
fn assistant_tool_and_quoted_user_envelopes_remain_literal() {
    let assistant = domain::input_envelope::bot_instructions("quoted rules", "quoted text");
    let tool = "<ait-bot-instructions>quoted rules</ait-bot-instructions>quoted text";
    let quoted = "What does <ait-input kind=\"bot_wake\">notice</ait-input> mean?";
    let timeline = Timeline::memory().unwrap();
    timeline
        .append(
            "agent",
            "fixture",
            &[
                entry(
                    "assistant",
                    json!({"type":"assistant_message", "text":assistant}),
                ),
                entry("tool", json!({"type":"tool_call", "name":tool})),
                entry("user", json!({"type":"user_message", "text":quoted})),
            ],
        )
        .unwrap();

    let (context, truncated) = timeline.side_context("agent", 4096).unwrap();

    assert!(!truncated);
    assert_eq!(context.len(), 3);
    assert_eq!(context[0].role, "assistant");
    assert_eq!(context[0].text, assistant);
    assert_eq!(context[1].role, "tool");
    assert_eq!(context[1].text, tool);
    assert_eq!(context[2].text, quoted);
}

#[test]
fn byte_budget_counts_projected_and_redacted_text_without_hidden_instructions() {
    let timeline = Timeline::memory().unwrap();
    let wrapped = domain::input_envelope::bot_instructions(
        &"hidden instructions ".repeat(5000),
        "\"\\世界\napi_key=fixture-secret",
    );
    timeline
        .append(
            "agent",
            "fixture",
            &[
                entry("older", json!({"type":"user_message", "text":"older"})),
                entry("newer", json!({"type":"user_message", "text":wrapped})),
            ],
        )
        .unwrap();
    let expected = vec![
        SideContextEntry {
            role: "user".to_owned(),
            text: "older".to_owned(),
        },
        SideContextEntry {
            role: "user".to_owned(),
            text: "\"\\世界\napi_key=[redacted]".to_owned(),
        },
    ];
    // The context budget counts entries and separators; the enclosing brackets are external.
    let budget = encoded_size(&expected).unwrap() - 2;

    let (context, truncated) = timeline.side_context("agent", budget).unwrap();

    assert!(!truncated);
    assert_eq!(context, expected);
    let (context, truncated) = timeline
        .side_context("agent", encoded_size(&expected[1]).unwrap())
        .unwrap();
    assert!(truncated);
    assert_eq!(context, expected[1..]);
}

#[test]
fn instructions_without_display_text_do_not_consume_the_context_budget() {
    let timeline = Timeline::memory().unwrap();
    let current = domain::input_envelope::bot_instructions("hidden instructions", "");
    timeline
        .append(
            "agent",
            "fixture",
            &[
                entry("current", json!({"type":"user_message", "text":current})),
                entry(
                    "legacy",
                    json!({"type":"user_message", "text":
                    "<ait-bot-instructions>hidden instructions</ait-bot-instructions>"}),
                ),
            ],
        )
        .unwrap();

    assert_eq!(
        timeline.side_context("agent", 0).unwrap(),
        (Vec::new(), false)
    );
}
