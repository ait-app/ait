use super::*;

fn requested(captured: Captured) -> Value {
    let Captured::Events(events) = captured else {
        panic!("expected a host request: {captured:?}");
    };
    let [AgentTurnEvent::PermissionRequested(request)] = events.as_slice() else {
        panic!("expected one host request: {events:?}");
    };
    request.clone()
}

fn reply(captured: Captured) -> Value {
    let Captured::Reply(reply) = captured else {
        panic!("expected an automatic reply: {captured:?}");
    };
    reply
}

/// Answer `id` with `response`, send nothing, and record the answer as sent.
fn answer(dialogs: &mut Dialogs, id: &str, response: &Value) -> Value {
    let answer = dialogs.response(id, response).unwrap();
    let native = answer.native.clone();
    dialogs.resolve(id, answer);
    native
}

fn allow(answer: &Value) -> Value {
    json!({"behavior":"allow","updatedInput":{"answers":{"value":answer}}})
}

fn select(id: &str, title: &str, options: &[&str]) -> Value {
    json!({"type":"extension_ui_request","id":id,"method":"select","title":title,
        "options":options})
}

fn native(id: &str, value: &str) -> Value {
    json!({"type":"extension_ui_response","id":id,"value":value})
}

/// An OMP 18.18.8 `ask` call: the assistant tool call carries an HTML form.
fn ask_started(dialogs: &mut Dialogs) {
    dialogs.observe(
        &json!({"type":"message_end","message":{"role":"assistant","content":[
        {"type":"toolCall","id":"ask-call","name":"ask","arguments":{"i":"pick",
            "html":"<form>…</form>"}}]}}),
    );
}

#[test]
fn every_interactive_method_matches_the_shared_client_contract_and_native_reply() {
    let fixtures: Vec<Value> =
        serde_json::from_str(include_str!("../../tests/fixtures/omp_dialogs.json")).unwrap();
    for fixture in fixtures {
        let mut dialogs = Dialogs::default();
        assert_eq!(
            requested(dialogs.capture(&fixture["native"]).unwrap()),
            fixture["request"],
            "{}",
            fixture["name"]
        );
        let id = fixture["native"]["id"].as_str().unwrap();
        assert_eq!(dialogs.pending(), vec![fixture["request"].clone()]);
        assert_eq!(
            dialogs.response(id, &fixture["response"]).unwrap().native,
            fixture["nativeResponse"]
        );
        assert_eq!(
            dialogs
                .response(id, &json!({"behavior":"deny"}))
                .unwrap()
                .native,
            json!({"type":"extension_ui_response","id":id,"cancelled":true})
        );
        assert_eq!(
            dialogs
                .capture(&json!({"method":"cancel","targetId":id}))
                .unwrap(),
            Captured::Events(vec![AgentTurnEvent::PermissionResolved(id.to_owned())])
        );
        assert_eq!(dialogs.pending(), Vec::<Value>::new());
        assert!(dialogs.response(id, &fixture["response"]).is_err());
    }
}

#[test]
fn structured_question_matches_the_client_wire_contract() {
    let fixture: Value =
        serde_json::from_str(include_str!("../../tests/fixtures/omp_question.json")).unwrap();
    let mut dialogs = Dialogs::default();

    let request = requested(dialogs.capture(&fixture["native"]).unwrap());

    assert_eq!(request, fixture["request"]);
    assert_eq!(dialogs.pending(), vec![fixture["request"].clone()]);
    assert_eq!(
        dialogs
            .response("ask-1", &fixture["response"])
            .unwrap()
            .native,
        fixture["nativeResponse"]
    );
}

#[test]
fn optional_dialog_descriptions_are_strings_or_absent() {
    for method in ["select", "confirm", "input", "editor", "ask"] {
        for message in [
            None,
            Some(Value::Null),
            Some(json!(42)),
            Some(json!("Details")),
        ] {
            let mut dialogs = Dialogs::default();
            let mut native = json!({"id":"dialog","method":method,"title":"Question",
                "options":["A"],"questions":[{"id":"choice","question":"Choose",
                    "options":[{"label":"A"}]}]});
            if let Some(message) = &message {
                native["message"] = message.clone();
            }

            let request = requested(dialogs.capture(&native).unwrap());

            assert_eq!(
                request.get("description"),
                message.as_ref().filter(|v| v.is_string())
            );
        }
    }
}

#[test]
fn structured_questions_preserve_multiselect_and_commas() {
    let mut dialogs = Dialogs::default();
    let native = json!({"id":"ask","method":"ask","questions":[
        {"id":"db","question":"Database?","options":[{"label":"A,B"}]},
        {"id":"features","question":"Features?","multi":true,"options":[{"label":"Auth"},{"label":"Search"}]}]});
    dialogs.capture(&native).unwrap();
    let response = dialogs
        .response(
            "ask",
            &json!({"behavior":"allow","updatedInput":{"answers":{
        "db":["A,B"],"features":["Auth","Search"]}}}),
        )
        .unwrap()
        .native;
    assert_eq!(response["answers"][0]["selectedOptions"], json!(["A,B"]));
    assert_eq!(
        response["answers"][1]["selectedOptions"],
        json!(["Auth", "Search"])
    );
    for invalid in [
        json!({"db":["A","B"],"features":[]}),
        json!({"db":["A,B"],"features":["Auth","Auth"]}),
    ] {
        assert!(
            dialogs
                .response(
                    "ask",
                    &json!({"behavior":"allow","updatedInput":{"answers":invalid}})
                )
                .is_err()
        );
    }
}

#[test]
fn confirmations_are_yes_no_questions_answered_with_confirmed() {
    let mut dialogs = Dialogs::default();
    let request = requested(
        dialogs
            .capture(
                &json!({"type":"extension_ui_request","id":"confirm","method":"confirm",
                "title":"Continue?","message":"Run tool"}),
            )
            .unwrap(),
    );
    assert_eq!(request["kind"], "question");
    assert_eq!(
        request["input"]["questions"][0]["question"],
        "Continue?\n\nRun tool"
    );
    for (answer, confirmed) in [("Yes", true), ("No", false), (" yes ", true)] {
        assert_eq!(
            dialogs
                .response("confirm", &allow(&json!(answer)))
                .unwrap()
                .native["confirmed"],
            confirmed
        );
    }
    for invalid in [
        json!({"behavior":"allow"}),
        json!({"behavior":"allow","updatedInput":{}}),
        allow(&json!("Maybe")),
    ] {
        assert!(dialogs.response("confirm", &invalid).is_err());
    }
    assert_eq!(
        dialogs
            .response("confirm", &json!({"behavior":"deny"}))
            .unwrap()
            .native["cancelled"],
        true
    );
    assert_eq!(
        dialogs
            .capture(&json!({"method":"cancel","targetId":"confirm"}))
            .unwrap(),
        Captured::Events(vec![AgentTurnEvent::PermissionResolved(
            "confirm".to_owned()
        )])
    );
    assert!(dialogs.response("confirm", &allow(&json!("Yes"))).is_err());
    for method in ["notify", "open_url", "setWidget", "future_method"] {
        assert_eq!(
            dialogs
                .capture(&json!({"method":method,"id":"notice"}))
                .unwrap(),
            Captured::Ignored
        );
    }
}

#[test]
fn optional_inputs_allow_an_empty_answer_and_name_the_card_as_paseo_does() {
    for (title, placeholder, shown) in [
        ("Notes", "Optional", "Optional response"),
        ("Add a comment", "Press enter to skip", "Optional comment"),
        ("Notes", "Optional comment", "Optional comment"),
    ] {
        let mut dialogs = Dialogs::default();
        let request = requested(
            dialogs
                .capture(&json!({"id":"note","method":"input","title":title,
                    "placeholder":placeholder}))
                .unwrap(),
        );
        let question = &request["input"]["questions"][0];
        assert_eq!(request["title"], shown);
        assert_eq!(question["header"], shown);
        assert_eq!(question["question"], title);
        assert_eq!(question["allowEmpty"], true);
        assert_eq!(question["dismissLabel"], "Skip");
        assert_eq!(
            dialogs
                .response(
                    "note",
                    &json!({"behavior":"allow","updatedInput":{"answers":{shown:""}}})
                )
                .unwrap()
                .native,
            native("note", "")
        );
    }
    let mut dialogs = Dialogs::default();
    let request = requested(
        dialogs
            .capture(&json!({"id":"name","method":"input","title":"Name",
                "placeholder":"Skipping stones"}))
            .unwrap(),
    );
    assert_eq!(request["title"], "Name");
    assert!(request["input"]["questions"][0].get("allowEmpty").is_none());
}

#[test]
fn selects_and_timeouts_reject_invalid_answers() {
    let mut dialogs = Dialogs::default();
    dialogs
        .capture(&select("select", "Choose", &["A", "B"]))
        .unwrap();
    assert!(dialogs.response("select", &allow(&json!("C"))).is_err());
    assert_eq!(
        dialogs
            .response("select", &allow(&json!("A")))
            .unwrap()
            .native,
        native("select", "A")
    );
    dialogs
        .capture(&json!({"id":"expired","method":"input","title":"Name","timeout":0}))
        .unwrap();
    assert!(dialogs.response("expired", &allow(&json!("x"))).is_err());
    assert_eq!(dialogs.pending().len(), 1);
    assert_eq!(dialogs.clear().len(), 2);
}

#[test]
fn tool_approvals_become_tool_permissions_with_native_detail_and_scopes() {
    let cases = [
        (
            "Allow tool: bash\nCommand: printf 'a\nb'",
            vec!["Approve", "Approve bash for this session", "Deny"],
            json!({"type":"shell","command":"printf 'a\nb'"}),
            "Command: printf 'a\nb'",
        ),
        (
            "Allow tool: edit\nFile: src/lib.rs\nFile: src/main.rs",
            vec!["Approve", "Approve edit for this session", "Deny"],
            json!({"type":"edit","filePath":"src/lib.rs"}),
            "File: src/lib.rs",
        ),
        (
            "Allow tool: write\nReason: target not yet approved\nPath: a.txt\nContent:\none\n\
             two\nFolder: /work — 0 entries (0 folders, 0 files)",
            vec![
                "Approve Directory: /work",
                "Approve File",
                "Always Approve Directory: /work",
                "Always Approve File",
                "Approve Once",
                "Deny",
            ],
            json!({"type":"write","filePath":"a.txt","content":"one\ntwo"}),
            "Path: a.txt",
        ),
    ];
    for (title, options, detail, description) in cases {
        let mut dialogs = Dialogs::default();
        let request = requested(
            dialogs
                .capture(&select("approval", title, &options))
                .unwrap(),
        );
        let tool = detail["type"].as_str().unwrap();
        let tool = if tool == "shell" { "bash" } else { tool };
        assert_eq!(request["kind"], "tool");
        assert_eq!(request["name"], tool);
        assert_eq!(request["title"], format!("Allow tool: {tool}"));
        assert_eq!(request["detail"], detail);
        assert_eq!(request["description"], description);
        let actions = request["actions"].as_array().unwrap();
        let once = options
            .iter()
            .find(|option| ["Approve", "Approve Once"].contains(option))
            .unwrap();
        assert_eq!(actions.len(), options.len());
        assert_eq!(
            actions[0],
            json!({"id":"deny","label":"Deny","behavior":"deny",
            "variant":"danger","intent":"dismiss"})
        );
        assert_eq!(actions[1]["id"], "approve");
        assert_eq!(actions[1]["label"], *once);
        assert_eq!(
            dialogs
                .response("approval", &json!({"behavior":"allow"}))
                .unwrap()
                .native,
            native("approval", once)
        );
        assert_eq!(
            dialogs
                .response("approval", &json!({"behavior":"deny"}))
                .unwrap()
                .native,
            native("approval", "Deny")
        );
        assert_eq!(
            dialogs
                .response(
                    "approval",
                    &json!({"behavior":"allow","selectedActionId":actions[2]["id"]})
                )
                .unwrap()
                .native,
            native("approval", actions[2]["label"].as_str().unwrap())
        );
        for invalid in [
            json!({"behavior":"allow","selectedActionId":"deny"}),
            json!({"behavior":"deny","selectedActionId":"approve"}),
            json!({"behavior":"allow","selectedActionId":"missing"}),
            json!({"behavior":"maybe"}),
        ] {
            assert!(dialogs.response("approval", &invalid).is_err(), "{invalid}");
        }
    }
}

#[test]
fn selects_that_are_not_tool_approvals_stay_generic_questions() {
    for (title, options) in [
        ("Allow tool: read\nPath: a.txt", vec!["Approve", "Deny"]),
        ("Allow tool: bash", vec!["Approve", "Deny"]),
        (
            "Allow tool: bash\nCommand: ls",
            vec!["Approve", "Deny", "Later"],
        ),
        ("Allow tool: bash\nCommand: ls", vec!["Approve"]),
        ("Allow tool: write\nPath: a.txt", vec!["Approve", "Deny"]),
        ("Write /tmp/probe.html", vec!["Approve", "Deny"]),
    ] {
        let mut dialogs = Dialogs::default();
        let request = requested(dialogs.capture(&select("other", title, &options)).unwrap());
        assert_eq!(request["kind"], "question", "{title}");
        assert_eq!(request["name"], "extension_ui");
    }
}

#[test]
fn ask_choices_become_one_card_and_replay_omp_follow_ups() {
    let mut dialogs = Dialogs::default();
    ask_started(&mut dialogs);
    let options = [
        "Auth",
        "Search",
        "Other (type your own)",
        "✔ Done selecting",
    ];
    let request = requested(
        dialogs
            .capture(&select("s1", "Which features?", &options))
            .unwrap(),
    );
    assert_eq!(request["name"], "ask");
    assert_eq!(
        request["input"]["questions"][0],
        json!({"header":"Which features?","question":"Which features?","answerKey":"value",
            "options":[{"label":"Auth"},{"label":"Search"}],"multiSelect":true,
            "allowOther":true,"answerFormat":"array"})
    );
    assert_eq!(
        answer(
            &mut dialogs,
            "s1",
            &allow(&json!(["Search", "Auth", "Custom feat"]))
        ),
        native("s1", "Search")
    );
    assert_eq!(
        reply(
            dialogs
                .capture(&select("s2", "(1 selected) Which features?", &options))
                .unwrap()
        ),
        native("s2", "Auth")
    );
    assert_eq!(
        reply(
            dialogs
                .capture(&select("s3", "(2 selected) Which features?", &options))
                .unwrap()
        ),
        native("s3", "Other (type your own)")
    );
    assert_eq!(
        reply(
            dialogs
                .capture(&json!({"id":"i1","method":"input","title":"Which features?"}))
                .unwrap()
        ),
        native("i1", "Custom feat")
    );
    assert_eq!(
        reply(
            dialogs
                .capture(&select("s4", "(3 selected) Which features?", &options))
                .unwrap()
        ),
        native("s4", "✔ Done selecting")
    );
    assert_eq!(dialogs.pending(), Vec::<Value>::new());
    // The replay is over: the next field asks the host again.
    let next = requested(
        dialogs
            .capture(&select(
                "s5",
                "Which database?",
                &["A", "Other (type your own)"],
            ))
            .unwrap(),
    );
    assert_eq!(next["input"]["questions"][0]["multiSelect"], false);
}

#[test]
fn ask_single_choices_type_custom_text_and_finish_late_revealed_multi_choices() {
    let mut dialogs = Dialogs::default();
    ask_started(&mut dialogs);
    let single = ["A,B", "C", "Other (type your own)"];
    requested(
        dialogs
            .capture(&select("s1", "Which database?", &single))
            .unwrap(),
    );
    assert_eq!(
        answer(&mut dialogs, "s1", &allow(&json!("Custom DB"))),
        native("s1", "Other (type your own)")
    );
    assert_eq!(
        reply(
            dialogs
                .capture(&json!({"id":"i1","method":"input","title":"Which database?"}))
                .unwrap()
        ),
        native("i1", "Custom DB")
    );
    // OMP moved on; a same-titled select without `(n selected)` is a new question.
    requested(
        dialogs
            .capture(&select("s2", "Which database?", &single))
            .unwrap(),
    );
    assert_eq!(
        answer(&mut dialogs, "s2", &allow(&json!("A,B"))),
        native("s2", "A,B")
    );
    // A required checkbox group offers Done only once something is checked.
    assert_eq!(
        reply(
            dialogs
                .capture(&select(
                    "s3",
                    "(1 selected) Which database?",
                    &["A,B", "C", "Other (type your own)", "✓ Done selecting"]
                ))
                .unwrap()
        ),
        native("s3", "✓ Done selecting")
    );
    assert!(dialogs.response("s2", &allow(&json!("C"))).is_err());
}

#[test]
fn ask_replay_cancels_without_done_and_ends_on_deny_or_tool_end() {
    let mut dialogs = Dialogs::default();
    ask_started(&mut dialogs);
    let options = ["Red", "Blue", "Other (type your own)"];
    let request = requested(
        dialogs
            .capture(&select("s1", "Colors? (1/2)", &options))
            .unwrap(),
    );
    assert_eq!(request["input"]["questions"][0]["multiSelect"], false);
    dialogs.observe(
        &json!({"type":"tool_execution_start","toolCallId":"ask-call",
        "toolName":"ask","args":{}}),
    );
    answer(&mut dialogs, "s1", &allow(&json!("Red")));
    assert_eq!(
        reply(
            dialogs
                .capture(&select("s2", "(1 selected) Colors? (1/2)", &options))
                .unwrap()
        ),
        json!({"type":"extension_ui_response","id":"s2","cancelled":true})
    );
    requested(
        dialogs
            .capture(&select("s3", "Colors? (1/2)", &options))
            .unwrap(),
    );
    assert_eq!(
        answer(&mut dialogs, "s3", &json!({"behavior":"deny"})),
        json!({"type":"extension_ui_response","id":"s3","cancelled":true})
    );
    requested(
        dialogs
            .capture(&select("s4", "(1 selected) Colors? (1/2)", &options))
            .unwrap(),
    );
    answer(&mut dialogs, "s4", &allow(&json!(["Red", "Blue"])));
    dialogs.observe(&json!({"type":"tool_execution_end","toolCallId":"ask-call",
        "toolName":"ask","result":{},"isError":true}));
    // Without a running ask, the same select is a generic question.
    let generic = requested(
        dialogs
            .capture(&select("s5", "(1 selected) Colors? (1/2)", &options))
            .unwrap(),
    );
    assert_eq!(generic["name"], "extension_ui");
}

#[test]
fn ask_cards_keep_legacy_question_descriptions_and_reject_malformed_answers() {
    let mut dialogs = Dialogs::default();
    dialogs.observe(
        &json!({"type":"tool_execution_start","toolCallId":"ask-call",
        "toolName":"ask","args":{"questions":[{"id":"colors","question":"Which colors?",
            "multi":true,"options":[{"label":"Red","description":"Warm"},
                {"label":"Blue","description":"Cool"}]}]}}),
    );
    let request = requested(
        dialogs
            .capture(&select(
                "s1",
                "Which colors? (1/2)",
                &["Blue", "Red (Recommended)", "Other (type your own)"],
            ))
            .unwrap(),
    );
    let question = &request["input"]["questions"][0];
    assert_eq!(request["title"], "Which colors?");
    assert_eq!(question["multiSelect"], true);
    assert_eq!(
        question["options"],
        json!([{"label":"Blue","description":"Cool"},
            {"label":"Red (Recommended)","description":"Warm"}])
    );
    for invalid in [
        allow(&json!(["Blue", "Blue"])),
        allow(&json!(["x", "y"])),
        allow(&json!(42)),
        allow(&json!([" "])),
        json!({"behavior":"allow","updatedInput":{"answers":{"other":["Blue"]}}}),
        json!({"behavior":"allow","selectedActionId":"deny","updatedInput":{"answers":{
            "value":["Blue"]}}}),
    ] {
        assert!(dialogs.response("s1", &invalid).is_err(), "{invalid}");
    }
    // Paseo's joined form is split on exact labels.
    assert_eq!(
        dialogs
            .response(
                "s1",
                &json!({"behavior":"allow","updatedInput":{"answers":{
                    "Which colors?":"Red (Recommended), Blue, Violet"}}})
            )
            .unwrap()
            .native,
        native("s1", "Red (Recommended)")
    );
    assert_eq!(
        dialogs.response("s1", &allow(&json!([]))).unwrap().native,
        json!({"type":"extension_ui_response","id":"s1","cancelled":true})
    );
}

#[test]
fn multi_select_answers_split_on_exact_labels_longest_first() {
    let labels = ["A", "A, B", "C"];
    for (answer, selected, custom) in [
        ("A, B, C", vec!["A, B", "C"], None),
        ("C, A", vec!["C", "A"], None),
        ("A, Violet, C", vec!["A"], Some("Violet, C")),
        ("Violet", vec![], Some("Violet")),
        ("", vec![], None),
    ] {
        assert_eq!(
            ask::split_multi_select_answer(answer, &labels),
            (selected, custom.map(str::to_owned)),
            "{answer}"
        );
    }
}

#[test]
fn ask_multi_selects_without_other_accept_only_their_options() {
    let mut dialogs = Dialogs::default();
    ask_started(&mut dialogs);
    let options = ["Red", "Blue", "✔ Done selecting"];
    let request = requested(dialogs.capture(&select("s1", "Colors", &options)).unwrap());
    assert_eq!(request["input"]["questions"][0]["allowOther"], false);
    assert!(
        dialogs
            .response("s1", &allow(&json!(["Red", "Violet"])))
            .is_err()
    );
    assert_eq!(
        answer(&mut dialogs, "s1", &allow(&json!(["Blue"]))),
        native("s1", "Blue")
    );
    assert_eq!(
        reply(
            dialogs
                .capture(&select("s2", "(1 selected) Colors", &options))
                .unwrap()
        ),
        native("s2", "✔ Done selecting")
    );
}

#[test]
fn oversized_approvals_show_bounded_detail_and_other_questions_are_cancelled() {
    let mut dialogs = Dialogs::default();
    let content = "é".repeat(40 * 1024);
    let title = format!("Allow tool: write\nPath: big.txt\nContent:\n{content}");
    let request = requested(
        dialogs
            .capture(&select("write", &title, &["Approve", "Deny"]))
            .unwrap(),
    );
    let shown = request["detail"]["content"].as_str().unwrap();
    assert!(shown.len() < 17 * 1024);
    let kept = shown
        .strip_suffix("\n[Truncated; the full text remains in OMP.]")
        .unwrap();
    assert!(!kept.is_empty() && content.starts_with(kept));
    let command = "x".repeat(80 * 1024);
    let title = format!("Allow tool: bash\nCommand: {command}");
    let request = requested(
        dialogs
            .capture(&select("bash", &title, &["Approve", "Deny"]))
            .unwrap(),
    );
    assert!(request.to_string().len() < 64 * 1024);
    assert_eq!(
        answer(&mut dialogs, "bash", &json!({"behavior":"allow"})),
        native("bash", "Approve")
    );

    for question in [
        select("large-select", &"x".repeat(70 * 1024), &["A", "B"]),
        json!({"type":"extension_ui_request","id":"large-input","method":"input",
            "title":"x".repeat(70 * 1024)}),
    ] {
        let id = question["id"].as_str().unwrap();
        assert_eq!(
            dialogs.capture(&question).unwrap(),
            Captured::Declined(json!({"type":"extension_ui_response","id":id,"cancelled":true}))
        );
    }
    let pending: Vec<_> = dialogs
        .pending()
        .into_iter()
        .map(|r| r["id"].clone())
        .collect();
    assert_eq!(pending, [json!("write")]);
}
