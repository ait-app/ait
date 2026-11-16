//! Outputs and rejection cases executed from Paseo's activity-curator.test.ts.

use super::*;

#[test]
fn fork_context_matches_recorded_upstream_cases() {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/paseo-fork-context.json"
    ))
    .unwrap();
    assert_eq!(
        fixture["source"]["revision"],
        "30178c4f58b67f8472901356e1484022bd835de0"
    );
    let cases = fixture["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 6);
    for case in cases {
        let input = &case["input"];
        let rows: Vec<_> = input["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| {
                let mut row = row(value["seq"].as_u64().unwrap(), value["item"].clone());
                row.entry.turn_id = value["turnId"].as_str().map(str::to_owned);
                row
            })
            .collect();
        let mut request = request();
        request.boundary_message_id = input["boundaryMessageId"].as_str().map(str::to_owned);
        if let Some(cursor) = input["cursorBoundary"].get("cursor") {
            request.boundary_cursor = Some(serde_json::from_value(cursor.clone()).unwrap());
        }
        let agent = json!({"title":input["agentTitle"],"cwd":input["cwd"]});
        let actual = export(
            &request,
            input["cursorBoundary"]["timelineEpoch"]
                .as_str()
                .unwrap_or("e"),
            &rows,
            &agent,
        );
        if case.get("error").is_some() {
            assert_eq!(actual, Err(ErrorCode::InvalidMessage), "{}", case["name"]);
        } else {
            let mut actual = actual.unwrap();
            let object = actual.as_object_mut().unwrap();
            object.remove("agentId");
            object.remove("error");
            // Keep the recorded upstream fixture intact while expecting Ait's branding.
            let mut expected = case["expected"].clone();
            let text = expected["attachment"]["text"].as_str().unwrap();
            expected["attachment"]["text"] = json!(text.replacen(
                "Chat history from a previous Paseo agent.",
                "Chat history from a previous Ait agent.",
                1,
            ));
            assert_eq!(actual, expected, "{}", case["name"]);
        }
    }
}

#[test]
fn rejects_a_cursor_inside_a_message_that_has_since_grown() {
    let rows = [
        row(
            1,
            json!({"type":"assistant_message","messageId":"reply","text":"A"}),
        ),
        row(
            2,
            json!({"type":"assistant_message","messageId":"reply","text":"B"}),
        ),
    ];
    let mut request = request();
    request.boundary_cursor = Some(Cursor {
        epoch: "e".to_owned(),
        seq: 1,
    });
    assert_eq!(
        export(&request, "e", &rows, &Value::Null),
        Err(ErrorCode::InvalidMessage)
    );
    request.boundary_cursor.as_mut().unwrap().seq = 2;
    let result = export(&request, "e", &rows, &Value::Null).unwrap();
    assert_eq!(result["itemCount"], 1);
    assert!(
        result["attachment"]["text"]
            .as_str()
            .unwrap()
            .contains("[Assistant] AB")
    );
}
