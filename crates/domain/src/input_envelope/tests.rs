use std::borrow::Cow;

use serde_json::json;

use super::{Envelope, Origin, bot_instructions, bot_wake, parse, schedule};

fn scheduled(name: Option<&str>) -> Origin {
    Origin::Schedule {
        schedule_id: "s1".to_owned(),
        run_id: "r1".to_owned(),
        name: name.map(str::to_owned),
    }
}

#[test]
fn schedule_round_trips_name_and_escaped_prompt() {
    let name = "nightly \"report\" <&>";
    let prompt = "Check </ait-input> and </div>";

    let input = schedule("s1", "r1", Some(name), prompt);

    assert_eq!(
        input,
        "<ait-input kind=\"schedule\" schedule-id=\"s1\" run-id=\"r1\" \
         name=\"nightly &quot;report&quot; &lt;&amp;&gt;\">\n\
         Check <\\/ait-input> and <\\/div>\n</ait-input>"
    );
    assert_eq!(
        parse(&input),
        Some(Envelope {
            origin: scheduled(Some(name)),
            text: Cow::Borrowed(prompt),
        })
    );
}

#[test]
fn unnamed_schedule_and_wake_round_trip() {
    let unnamed = schedule("s1", "r1", None, "hello");
    let wake = bot_wake("- agent_finished at 2026-11-12T00:00:00Z: done\n  </b> summary");

    assert_eq!(parse(&unnamed).map(|e| e.origin), Some(scheduled(None)));
    assert_eq!(
        parse(&wake),
        Some(Envelope {
            origin: Origin::BotWake,
            text: Cow::Borrowed("- agent_finished at 2026-11-12T00:00:00Z: done\n  </b> summary"),
        })
    );
}

#[test]
fn bot_instructions_keep_only_the_user_text() {
    let input = bot_instructions("rules\n<workspace-memory>\nx\n</workspace-memory>", "hello");

    assert_eq!(
        parse(&input),
        Some(Envelope {
            origin: Origin::BotInstructions,
            text: Cow::Borrowed("hello"),
        })
    );
}

#[test]
fn legacy_tags_still_parse() {
    let named =
        "<ait-system>\nSchedule \"a \" fired (b\" fired (id=s1, run=r1).\nhello\n</ait-system>";
    let unnamed = "<ait-system>\nSchedule fired (id=s1, run=r1).\nline 1\nline 2\n</ait-system>";
    let wake = "<ait-notifications>\n- x at t: <\\/b>\n</ait-notifications>";
    let instructions = "<ait-bot-instructions>\nrules\n</ait-bot-instructions>\n\nhi";

    assert_eq!(
        parse(named),
        Some(Envelope {
            origin: scheduled(Some("a \" fired (b")),
            text: Cow::Borrowed("hello"),
        })
    );
    assert_eq!(
        parse(unnamed).map(|e| (e.origin, e.text)),
        Some((scheduled(None), Cow::Borrowed("line 1\nline 2")))
    );
    assert_eq!(
        parse(wake).map(|e| (e.origin, e.text)),
        Some((Origin::BotWake, Cow::Borrowed("- x at t: </b>")))
    );
    assert_eq!(
        parse(instructions).map(|e| (e.origin, e.text)),
        Some((Origin::BotInstructions, Cow::Borrowed("hi")))
    );
}

#[test]
fn user_text_that_quotes_or_breaks_a_fence_is_not_an_envelope() {
    let inputs = [
        "what does <ait-input kind=\"schedule\"> mean?".to_owned(),
        format!("{} trailing", schedule("s1", "r1", None, "x")),
        "<ait-input kind=\"schedule\" schedule-id=\"s1\">\nx\n</ait-input>".to_owned(),
        "<ait-input kind=\"future\">\nx\n</ait-input>".to_owned(),
        "<ait-input kind=\"bot_wake\">\nunterminated".to_owned(),
        "<ait-system>\nnot a heading\nx\n</ait-system>".to_owned(),
        "<ait-system>\nSchedule fired (id=s1).\nx\n</ait-system>".to_owned(),
        "plain text".to_owned(),
    ];

    for input in inputs {
        assert_eq!(parse(&input), None, "{input}");
    }
}

#[test]
fn origin_serializes_for_the_timeline() {
    assert_eq!(
        serde_json::to_value(scheduled(Some("n"))).ok(),
        Some(json!({"kind":"schedule","scheduleId":"s1","runId":"r1","name":"n"}))
    );
    assert_eq!(
        serde_json::to_value(scheduled(None)).ok(),
        Some(json!({"kind":"schedule","scheduleId":"s1","runId":"r1"}))
    );
    assert_eq!(
        serde_json::to_value(Origin::BotWake).ok(),
        Some(json!({"kind":"bot_wake"}))
    );
}
