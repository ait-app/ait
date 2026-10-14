use super::*;

#[test]
fn catalog_has_unique_registered_methods_and_closed_schemas() {
    let mut names = std::collections::BTreeSet::new();
    let methods: Vec<_> = crate::capabilities::implemented_methods()
        .map(|method| method.name)
        .collect();
    assert_eq!(catalog::all().len(), 40);
    for tool in catalog::all() {
        assert!(names.insert(&tool.name));
        for method in &tool.methods {
            assert!(
                methods.contains(&method.as_str()),
                "{}: {method}",
                tool.name
            );
        }
        let descriptor = tool.descriptor();
        assert_eq!(descriptor["inputSchema"]["type"], "object");
        assert_eq!(descriptor["inputSchema"]["additionalProperties"], false);
        assert!(descriptor["description"].as_str().unwrap().len() > 10);
        assert!(!validation::matches(
            &tool.input_schema,
            &json!({"unexpected":true})
        ));
    }
}

#[test]
fn negotiates_supported_versions_and_latest_for_unknown_clients() {
    for version in VERSIONS.iter().copied().chain(["future"]) {
        let result = initialize(&json!({"protocolVersion":version,"capabilities":{},"clientInfo":{"name":"test","version":"1"}})).unwrap();
        assert_eq!(
            result["protocolVersion"],
            if version == "future" {
                "2025-11-25"
            } else {
                version
            }
        );
        assert_eq!(result["serverInfo"]["name"], "ait-daemon");
        assert_eq!(result["capabilities"], json!({"tools":{}}));
    }
    assert!(initialize(&json!({})).is_err());
    assert!(initialize(&json!({"protocolVersion":"2025-03-26"})).is_err());
}

#[test]
fn checks_header_media_types_and_protocol_versions() {
    let mut headers = HeaderMap::new();
    assert_eq!(
        validate_headers(&headers).unwrap_err().0,
        StatusCode::UNSUPPORTED_MEDIA_TYPE
    );
    headers.insert(
        "content-type",
        "application/json; charset=utf-8".parse().unwrap(),
    );
    assert_eq!(
        validate_headers(&headers).unwrap_err().0,
        StatusCode::NOT_ACCEPTABLE
    );
    headers.insert(
        "accept",
        "application/json, text/event-stream".parse().unwrap(),
    );
    assert!(validate_headers(&headers).is_ok());
    headers.insert("mcp-protocol-version", "invalid".parse().unwrap());
    assert_eq!(
        validate_headers(&headers).unwrap_err().0,
        StatusCode::BAD_REQUEST
    );
    headers.insert("mcp-protocol-version", "2025-11-25".parse().unwrap());
    assert!(validate_headers(&headers).is_ok());
    headers.insert(
        "accept",
        "application/json, text/event-stream;q=0".parse().unwrap(),
    );
    assert_eq!(
        validate_headers(&headers).unwrap_err().0,
        StatusCode::NOT_ACCEPTABLE
    );
}

#[test]
fn validation_rejects_unknown_nested_fields_invalid_ranges_and_types() {
    let schema = json!({"type":"object","required":["count"],"additionalProperties":false,
        "properties":{"count":{"type":"integer","minimum":1,"maximum":5},
            "text":{"type":["string","null"],"minLength":1,"maxLength":3},
            "flags":{"type":"array","items":{"type":"boolean"}},
            "labels":{"type":"object","additionalProperties":{"type":"string"}},
            "mode":{"type":"string","enum":["yes","no"]}}});
    assert!(validation::matches(
        &schema,
        &json!({"count":3,"text":"中文","flags":[true],"labels":{"a":"b"},"mode":"yes"})
    ));
    assert!(validation::matches(
        &schema,
        &json!({"count":3,"text":null})
    ));
    for invalid in [
        json!({}),
        json!({"count":0}),
        json!({"count":6}),
        json!({"count":1.5}),
        json!({"count":3,"text":""}),
        json!({"count":3,"text":"1234"}),
        json!({"count":3,"flags":[1]}),
        json!({"count":3,"labels":{"a":false}}),
        json!({"count":3,"mode":"wrong"}),
        json!({"count":3,"extra":true}),
    ] {
        assert!(!validation::matches(&schema, &invalid), "{invalid}");
    }
}

#[test]
fn tool_errors_are_safe_and_visible_while_structured_results_keep_text() {
    let success = tool_result(Ok(json!({"count":3})));
    assert_eq!(success["structuredContent"]["count"], 3);
    assert_eq!(success["content"][0]["text"], "{\"count\":3}");
    assert_eq!(success["isError"], false);
    for value in [
        json!({"error":"missing"}),
        json!({"success":false}),
        json!({"accepted":false}),
    ] {
        assert_eq!(tool_result(Ok(value))["isError"], true);
    }
    let failure = tool_result(Err(model::ErrorCode::InvalidMessage));
    assert_eq!(failure["isError"], true);
    assert!(failure.get("structuredContent").is_none());
    for id in [Value::Null, json!(true), json!(1.2), json!({})] {
        assert!(!valid_id(&id, true));
    }
    for id in [json!(1), json!("request")] {
        assert!(valid_id(&id, true));
    }
    assert!(valid_id(&Value::Null, false));
}
