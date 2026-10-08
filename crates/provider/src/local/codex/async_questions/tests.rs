use super::*;

fn item() -> Value {
    json!({"type":"agentMessage","id":"question","delivery":"async","questions":[
        {"title":"Which runtime?","options":["Rust","Python"]},{"title":"Any constraints?","options":null}]})
}

fn source_question(id: &str, turn: &str, timestamp: &str) -> (Value, NativeItem) {
    let mut question = item();
    question["id"] = json!(id);
    let entry = super::super::discovery::timeline_item(&question, turn, timestamp)
        .unwrap()
        .unwrap();
    (question, entry)
}

fn source_message(id: &str, turn: &str, timestamp: &str) -> NativeItem {
    super::super::discovery::timeline_item(
        &json!({"type":"agentMessage","id":id,"text":"Continue working"}),
        turn,
        timestamp,
    )
    .unwrap()
    .unwrap()
}

fn answers() -> Value {
    json!({"behavior":"allow","updatedInput":{"answers":{
        "Question 1":"Rust","Question 2":"No unsafe"
    }}})
}

#[test]
fn pending_survives_restart_and_answers_validate_before_completion() {
    let mut questions = Questions::default();
    let request = questions.receive(&item()).unwrap().unwrap();
    assert_eq!(request["input"]["questions"][0]["header"], "Question 1");
    assert!(questions.receive(&item()).unwrap().is_none());
    let mut restored = Questions::restore(questions.saved().as_ref()).unwrap();
    assert_eq!(restored.pending(), questions.pending());
    let id = request["id"].as_str().unwrap();
    assert!(
        restored
            .prepare(
                id,
                &json!({"behavior":"allow","updatedInput":{"answers":{"Question 1":"Rust"}}})
            )
            .is_err()
    );
    let response = json!({"behavior":"allow","updatedInput":{"answers":{"Question 1":" Rust ","Question 2":"No unsafe"}}});
    let prompt = restored.prepare(id, &response).unwrap().unwrap();
    assert_eq!(
        prompt.text,
        "Answers to your questions:\n\nWhich runtime?\nRust\n\nAny constraints?\nNo unsafe"
    );
    assert_eq!(
        prompt.client_message_id.as_deref(),
        Some(format!("async-answer:{:x}", Sha256::digest(id)).as_str())
    );
    let entry = restored.resolve(id, &response).unwrap();
    assert!(
        entry.item["detail"]["text"]
            .as_str()
            .unwrap()
            .contains("No unsafe")
    );
    assert!(restored.pending().is_empty());
    assert!(restored.prepare(id, &response).is_err());
    let restored = Questions::restore(restored.saved().as_ref()).unwrap();
    let mut history = vec![NativeItem {
        key: "native:turn:question".into(),
        turn_id: Some("turn".into()),
        timestamp: super::super::discovery::timestamp(),
        item: timeline(&item()).unwrap(),
    }];
    restored.history(&mut history).unwrap();
    assert_eq!(history[1].item, entry.item);
}

#[test]
fn dismissals_malformed_native_data_and_metadata_bounds_are_explicit() {
    let mut questions = Questions::default();
    questions.receive(&item()).unwrap();
    assert!(
        questions
            .prepare("permission-question", &json!({"behavior":"deny"}))
            .unwrap()
            .is_none()
    );
    questions
        .resolve("permission-question", &json!({"behavior":"deny"}))
        .unwrap();
    assert!(
        Questions::restore(questions.saved().as_ref())
            .unwrap()
            .pending()
            .is_empty()
    );
    let mut conflicting = item();
    conflicting["questions"][0]["title"] = json!("changed");
    assert!(questions.receive(&conflicting).is_err());
    for bad in [json!({}), json!([{"item":item(),"resolution":[1,2]}])] {
        assert!(Questions::restore(Some(&bad)).is_err());
    }
    let mut questions = Questions::default();
    for index in 0..32 {
        let mut item = item();
        item["id"] = json!(index.to_string());
        questions.receive(&item).unwrap();
    }
    assert!(questions.receive(&item()).is_err());
}

#[test]
fn restored_answers_keep_live_positions_times_and_order_without_replacing_the_timeline() {
    let (first, first_entry) = source_question("z", "turn", "2026-01-01T00:00:00Z");
    let (second, second_entry) = source_question("a", "turn", "2026-01-01T00:00:00Z");
    let anchor = source_message("working", "turn", "2026-01-01T00:00:00Z");
    let conclusion = source_message("done", "turn", "2026-01-01T00:00:00Z");
    let later = source_message("later", "next-turn", "2026-01-02T00:00:00Z");
    let mut questions = Questions::default();
    questions.receive(&first).unwrap();
    questions.receive(&second).unwrap();
    questions.observe(&anchor);
    let first_answer = questions.resolve("permission-z", &answers()).unwrap();
    let mut second_answer = questions
        .resolve("permission-a", &json!({"behavior":"deny"}))
        .unwrap();
    // Equal clock readings must preserve resolution order rather than request ID order.
    questions
        .records
        .get_mut("permission-a")
        .unwrap()
        .position
        .as_mut()
        .unwrap()
        .timestamp
        .clone_from(&first_answer.timestamp);
    second_answer.timestamp.clone_from(&first_answer.timestamp);
    let live = vec![
        first_entry.clone(),
        second_entry.clone(),
        anchor.clone(),
        first_answer,
        second_answer,
        conclusion.clone(),
        later.clone(),
    ];
    let mut history = vec![first_entry, second_entry, anchor, conclusion, later];

    let restored = Questions::restore(questions.saved().as_ref()).unwrap();
    restored.history(&mut history).unwrap();

    assert_eq!(history, live);
    restored.history(&mut history).unwrap();
    assert_eq!(history, live);
    let timeline = crate::storage::timeline::Timeline::memory().unwrap();
    let (epoch, _) = timeline.append("agent", "codex", &live).unwrap();
    assert_eq!(
        timeline.reconcile("agent", "codex", &history).unwrap(),
        epoch
    );
}

#[test]
fn legacy_answers_follow_their_questions_and_reuse_native_turns_and_times() {
    let (first, first_entry) = source_question("z", "first", "2026-01-01T00:00:00Z");
    let (second, second_entry) = source_question("a", "second", "2026-01-02T00:00:00Z");
    let first_end = source_message("first-end", "first", "2026-01-01T00:00:00Z");
    let second_end = source_message("second-end", "second", "2026-01-02T00:00:00Z");
    let saved = json!([
        {"item":second,"resolution":"dismissed"},
        {"item":first,"resolution":["Rust","No unsafe"]}
    ]);
    let questions = Questions::restore(Some(&saved)).unwrap();
    let mut history = vec![
        first_entry.clone(),
        first_end,
        second_entry.clone(),
        second_end,
    ];

    questions.history(&mut history).unwrap();

    assert_eq!(
        history
            .iter()
            .map(|entry| entry.key.as_str())
            .collect::<Vec<_>>(),
        [
            "native:first:z",
            "native:async-answer:permission-z",
            "native:first:first-end",
            "native:second:a",
            "native:async-answer:permission-a",
            "native:second:second-end"
        ]
    );
    assert_eq!(history[1].turn_id, first_entry.turn_id);
    assert_eq!(history[1].timestamp, first_entry.timestamp);
    assert_eq!(history[4].turn_id, second_entry.turn_id);
    assert_eq!(history[4].timestamp, second_entry.timestamp);
    let previous = history.clone();
    questions.history(&mut history).unwrap();
    assert_eq!(history, previous);

    // Replace an existing tail projection once, then preserve its corrected generation.
    let mut old_history = history.clone();
    old_history.retain(|entry| !entry.key.starts_with("native:async-answer:"));
    for index in [4, 1] {
        let mut answer = history[index].clone();
        answer.turn_id = None;
        answer.timestamp = "2026-10-08T00:00:00Z".into();
        old_history.push(answer);
    }
    let timeline = crate::storage::timeline::Timeline::memory().unwrap();
    let (old_epoch, _) = timeline.append("agent", "codex", &old_history).unwrap();
    let replacement = timeline.reconcile("agent", "codex", &history).unwrap();
    assert_ne!(replacement, old_epoch);
    assert_eq!(
        timeline.reconcile("agent", "codex", &history).unwrap(),
        replacement
    );
    assert_eq!(
        timeline
            .read("agent")
            .unwrap()
            .1
            .into_iter()
            .map(|row| row.entry)
            .collect::<Vec<_>>(),
        history
    );
}

#[test]
fn missing_anchors_fall_back_to_the_native_answer_prompt() {
    let (question, question_entry) = source_question("question", "first", "2026-01-01T00:00:00Z");
    let conclusion = source_message("done", "first", "2026-01-01T00:00:00Z");
    let removed = source_message("removed", "first", "2026-01-01T00:00:00Z");
    let mut questions = Questions::default();
    questions.receive(&question).unwrap();
    questions.observe(&removed);
    let prompt = questions
        .prepare("permission-question", &answers())
        .unwrap()
        .unwrap();
    let answer = questions
        .resolve("permission-question", &answers())
        .unwrap();
    let prompt_entry = super::super::discovery::timeline_item(
        &json!({"type":"userMessage","id":"reply","clientId":prompt.client_message_id,
            "content":[{"type":"text","text":prompt.text}]}),
        "second",
        "2026-01-02T00:00:00Z",
    )
    .unwrap()
    .unwrap();
    let later = source_message("later", "second", "2026-01-02T00:00:00Z");
    let mut history = vec![question_entry, conclusion, prompt_entry.clone(), later];

    Questions::restore(questions.saved().as_ref())
        .unwrap()
        .history(&mut history)
        .unwrap();

    assert_eq!(history[2], answer);
    assert_eq!(history[3], prompt_entry);
}

#[test]
fn absent_questions_do_not_restore_or_retain_answers() {
    let mut history = vec![source_message("later", "turn", "2026-01-01T00:00:00Z")];
    let previous = history.clone();
    let mut questions = Questions::default();
    questions.history(&mut history).unwrap();
    questions.receive(&item()).unwrap();
    questions.history(&mut history).unwrap();
    questions
        .resolve("permission-question", &answers())
        .unwrap();

    questions.history(&mut history).unwrap();

    assert_eq!(history, previous);
    questions.retain(&history);
    assert!(questions.saved().is_none());
}

#[test]
fn malformed_positions_and_oversized_answers_do_not_change_pending_questions() {
    let mut questions = Questions::default();
    questions.receive(&item()).unwrap();
    questions.observe(&source_message("anchor", "turn", "2026-01-01T00:00:00Z"));
    questions
        .resolve("permission-question", &answers())
        .unwrap();
    let saved = questions.saved().unwrap();
    for (field, value) in [
        ("timestamp", json!("not a timestamp")),
        ("order", json!(0)),
        ("anchor", json!({"key":"","turn_id":"turn"})),
        ("anchor", json!({"key":"anchor","turn_id":""})),
    ] {
        let mut malformed = saved.clone();
        malformed[0]["position"][field] = value;
        assert!(Questions::restore(Some(&malformed)).is_err());
    }
    let mut malformed = saved;
    malformed[0]["resolution"] = Value::Null;
    assert!(Questions::restore(Some(&malformed)).is_err());
    let question = json!({"type":"agentMessage","id":"large","delivery":"async",
        "questions":(0..32).map(|_| json!({"title":"Question"})).collect::<Vec<_>>()});
    let response = json!({"behavior":"allow","updatedInput":{"answers":
        (1..=32).map(|index| (format!("Question {index}"), json!("x".repeat(8192))))
            .collect::<serde_json::Map<_, _>>()}});
    let mut questions = Questions::default();
    questions.receive(&question).unwrap();
    let pending = questions.saved();

    assert!(questions.prepare("permission-large", &response).is_err());
    assert!(questions.resolve("permission-large", &response).is_err());

    assert_eq!(questions.saved(), pending);
    assert_eq!(questions.pending().len(), 1);
}
