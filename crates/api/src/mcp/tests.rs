use super::*;

#[test]
fn agent_creation_requires_a_model_and_explains_how_to_select_one() {
    let tool = catalog::all()
        .iter()
        .find(|tool| tool.name == "create_agent")
        .unwrap();
    let mut arguments = json!({"provider":"provider", "cwd":"/repo", "title":"Shell test",
        "initialPrompt":"Run printf hello"});
    for provider in [
        json!("provider"),
        json!("provider/"),
        json!("/model"),
        json!("provider/ "),
        Value::Null,
    ] {
        arguments["provider"] = provider;
        let error = validation::arguments(tool, None, &arguments).unwrap_err();
        assert!(error.contains("provider/model"));
        assert!(error.contains("list_models"));
        assert!(error.contains("returned model id verbatim"));
    }
    for provider in [
        "provider/namespace/model",
        "provider/namespace/other-model",
        "provider/model",
    ] {
        arguments["provider"] = json!(provider);
        assert_eq!(validation::arguments(tool, None, &arguments), Ok(()));
    }
    arguments["model"] = Value::Null;
    assert_eq!(validation::arguments(tool, None, &arguments), Ok(()));
    arguments["provider"] = json!("provider");
    arguments["model"] = json!("namespace/model");
    assert_eq!(validation::arguments(tool, None, &arguments), Ok(()));
    for model in [Value::Null, json!(""), json!(" "), json!(false)] {
        arguments["model"] = model;
        assert!(validation::arguments(tool, None, &arguments).is_err());
    }
    arguments["model"] = json!("namespace/model");
    arguments["provider"] = json!("provider/namespace/model");
    assert!(validation::arguments(tool, None, &arguments).is_err());
    arguments["provider"] = json!("provider");
    arguments["unexpected"] = json!(true);
    assert_eq!(
        validation::arguments(tool, None, &arguments),
        Err("Arguments do not match the tool input schema")
    );
    let list = catalog::all()
        .iter()
        .find(|tool| tool.name == "list_models")
        .unwrap();
    assert_eq!(
        validation::arguments(list, None, &json!({"provider":"provider"})),
        Ok(())
    );
}

#[test]
fn model_discovery_rejects_combined_selections_with_guidance() {
    let tool = catalog::all()
        .iter()
        .find(|tool| tool.name == "list_models")
        .unwrap();
    for provider in ["codex/gpt-5", "codex/namespace/model", "codex/"] {
        let error = validation::arguments(tool, None, &json!({"provider":provider})).unwrap_err();
        assert!(error.contains("bare provider id"));
        assert!(error.contains("Do not include a model"));
    }
    assert_eq!(
        validation::arguments(tool, None, &json!({"provider":"codex"})),
        Ok(())
    );
}

#[test]
fn agent_creation_rejects_a_model_inside_settings() {
    let tool = catalog::all()
        .iter()
        .find(|tool| tool.name == "create_agent")
        .unwrap();
    let mut arguments = json!({"provider":"provider","model":"model","cwd":"/repo",
        "title":"Task","initialPrompt":"Run printf hello"});
    assert_eq!(validation::arguments(tool, None, &arguments), Ok(()));

    arguments["settings"] = json!({"model":"other-model"});

    assert_eq!(
        validation::arguments(tool, None, &arguments),
        Err("Arguments do not match the tool input schema")
    );
}

#[test]
fn schedule_model_updates_reject_blank_strings_with_clear_guidance() {
    let tool = catalog::all()
        .iter()
        .find(|tool| tool.name == "update_schedule")
        .unwrap();
    for model in ["", " ", " \t\n"] {
        assert_eq!(
            validation::arguments(tool, None, &json!({"id":"schedule","model":model})),
            Err("update_schedule model must be a nonblank string or null. \
                 Send null to clear the model and use the provider default.")
        );
    }
    for arguments in [
        json!({"id":"schedule"}),
        json!({"id":"schedule","model":null}),
        json!({"id":"schedule","model":" namespace/model "}),
    ] {
        assert_eq!(validation::arguments(tool, None, &arguments), Ok(()));
    }
}

#[test]
fn schedule_model_conflicts_provide_corrective_guidance() {
    let tool = catalog::all()
        .iter()
        .find(|tool| tool.name == "update_schedule")
        .unwrap();
    for model in [Value::Null, json!("other")] {
        let arguments = json!({"id":"x","provider":"codex/gpt","model":model});
        let message = validation::arguments(tool, None, &arguments).unwrap_err();
        assert!(message.contains("conflicts"));
        assert!(message.contains("bare provider id"));
        assert!(message.contains("model: null"));
    }
    for arguments in [
        json!({"id":"x","provider":"codex/gpt"}),
        json!({"id":"x","provider":" codex/namespace/gpt ","model":" namespace/gpt "}),
        json!({"id":"x","provider":"codex","model":null}),
    ] {
        assert_eq!(validation::arguments(tool, None, &arguments), Ok(()));
    }
}

#[test]
fn schedule_creation_accepts_combined_selection_and_rejects_a_separate_model() {
    let tool = catalog::all()
        .iter()
        .find(|tool| tool.name == "create_schedule")
        .unwrap();
    for provider in ["codex", "codex/namespace/model"] {
        let mut arguments = json!({"workspaceId":"workspace","cwd":"/repo",
            "provider":provider,"prompt":"Report status","cron":"0 * * * *"});
        assert_eq!(validation::arguments(tool, None, &arguments), Ok(()));

        for model in [json!("namespace/model"), Value::Null] {
            arguments["model"] = model;
            assert_eq!(
                validation::arguments(tool, None, &arguments),
                Err("Arguments do not match the tool input schema")
            );
        }
    }
}

#[test]
fn catalog_has_unique_registered_methods_and_closed_schemas() {
    let mut names = std::collections::BTreeSet::new();
    let methods: Vec<_> = crate::capabilities::implemented_methods()
        .map(|method| method.name)
        .collect();
    assert_eq!(catalog::all().len(), 48);
    for tool in catalog::all() {
        assert!(names.insert(&tool.name));
        for method in &tool.methods {
            assert!(
                methods.contains(&method.as_str()),
                "{}: {method}",
                tool.name
            );
        }
        let descriptor = tool.descriptor(None);
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
        assert!(
            result["instructions"]
                .as_str()
                .unwrap()
                .contains("separate provider and model fields")
        );
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
    let failure = tool_result(Err(model::ErrorCode::InvalidMessage.into()));
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

#[test]
fn every_catalog_tool_has_an_explicit_access_class() {
    let table: std::collections::BTreeSet<_> = domain::bot::permissions::TOOLS
        .iter()
        .map(|(name, _)| *name)
        .collect();
    for tool in catalog::all() {
        assert!(table.contains(tool.name.as_str()), "{}", tool.name);
    }
}

#[test]
fn caller_context_follows_the_credential_and_rejects_other_headers() {
    let scoped = |kind| Scope {
        caller: Caller {
            root: "wks_root".to_owned(),
            kind,
        },
        grant: bot::mcp::Grant {
            permissions: domain::bot::permissions::BotPermissions::default(),
            workspaces: std::collections::BTreeSet::new(),
        },
    };
    assert_eq!(claimed(None, None), Ok(None));
    assert_eq!(claimed(None, Some("agent")), Ok(Some("agent")));
    let bot = scoped(CallerKind::Bot {
        bot_id: domain::BotId::generate(),
        conversation_id: domain::ConversationId::generate(),
        session_id: "session".to_owned(),
    });
    assert_eq!(claimed(Some(&bot), None), Ok(None));
    assert_eq!(claimed(Some(&bot), Some("session")), Ok(None));
    let subtree = scoped(CallerKind::Subtree {
        agent_id: "child".to_owned(),
        ticket: None,
    });
    assert_eq!(claimed(Some(&subtree), None), Ok(Some("child")));
    assert_eq!(claimed(Some(&subtree), Some("child")), Ok(Some("child")));
    for (scope, header) in [(&bot, "child"), (&subtree, "session")] {
        assert_eq!(
            claimed(Some(scope), Some(header)),
            Err((-32602, "Caller does not match credential"))
        );
    }
}

#[tokio::test]
async fn a_detached_call_holds_its_permit_until_it_ends_and_is_dropped_at_its_own_bound() {
    let budget = Arc::new(tokio::sync::Semaphore::new(1));
    let wait = Duration::from_millis(1);
    let tasks = tokio_util::task::TaskTracker::new();
    let (release, released) = tokio::sync::oneshot::channel::<()>();
    let slow = async move {
        let _ = released.await;
        1
    };
    let permit = budget.clone().try_acquire_owned().unwrap();

    let waited = bounded(&tasks, slow, permit, Some(Duration::from_secs(5)), wait).await;
    let while_running = budget.available_permits();
    release.send(()).unwrap();
    tasks.close();
    tokio::time::timeout(Duration::from_secs(5), tasks.wait())
        .await
        .unwrap();

    assert_eq!((waited, while_running), (None, 0));
    assert_eq!(budget.available_permits(), 1);

    // A creation that never ends is dropped at the detached bound and frees its permit.
    let tasks = tokio_util::task::TaskTracker::new();
    let (alive, dropped) = tokio::sync::oneshot::channel::<()>();
    let stuck = async move {
        let _alive = alive;
        std::future::pending::<()>().await;
    };
    let permit = budget.clone().try_acquire_owned().unwrap();
    let waited = bounded(&tasks, stuck, permit, Some(Duration::from_millis(20)), wait).await;
    tasks.close();
    let drained = tokio::time::timeout(Duration::from_secs(5), tasks.wait()).await;

    assert_eq!(waited, None);
    assert!(
        drained.is_ok(),
        "a stuck detached call kept the tracker open"
    );
    assert!(dropped.await.is_err(), "the stuck work was not dropped");
    assert_eq!(budget.available_permits(), 1);
}

#[tokio::test]
async fn a_detached_call_finishes_its_work_after_the_wait_times_out() {
    let tasks = tokio_util::task::TaskTracker::new();
    let work = |done: tokio::sync::oneshot::Sender<()>| async move {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let _ = done.send(());
        7
    };
    let (detached_done, detached_finished) = tokio::sync::oneshot::channel();
    let (attached_done, attached_finished) = tokio::sync::oneshot::channel();

    let detach = Some(Duration::from_secs(5));
    let wait = Duration::from_millis(1);

    let detached = bounded(&tasks, work(detached_done), (), detach, wait).await;
    let attached = bounded(&tasks, work(attached_done), (), None, wait).await;
    let quick = bounded(&tasks, async { 3 }, (), detach, Duration::from_secs(5)).await;

    assert_eq!((detached, attached, quick), (None, None, Some(3)));
    assert!(detached_finished.await.is_ok(), "detached work was dropped");
    assert!(
        attached_finished.await.is_err(),
        "attached work outlived its wait"
    );
}

#[test]
fn schedule_model_type_errors_are_reported_before_conflicts() {
    let tool = catalog::all()
        .iter()
        .find(|tool| tool.name == "update_schedule")
        .unwrap();
    for model in [json!(42), json!(false), json!([]), json!({})] {
        assert_eq!(
            validation::arguments(
                tool,
                None,
                &json!({"id":"x","provider":"codex/gpt","model":model})
            ),
            Err("Arguments do not match the tool input schema")
        );
    }
}
