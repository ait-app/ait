use super::*;

fn pages(fixture: &Fixture, pages: &Value) {
    std::fs::write(
        fixture.cwd.join("experimental-features.json"),
        pages.to_string(),
    )
    .unwrap();
}

fn supported(fixture: &Fixture, enabled: bool) {
    pages(
        fixture,
        &json!({"":{"data":[{"name":"async_compaction",
        "stage":"underDevelopment","enabled":enabled,"defaultEnabled":false}]}}),
    );
}

#[tokio::test]
async fn async_compaction_discovers_paginated_native_enablement_and_session_overrides() {
    let fixture = Fixture::new();
    pages(
        &fixture,
        &json!({"":{"data":[{"name":"unrelated"}],"nextCursor":"next"},
        "next":{"data":[{"name":"async_compaction","stage":"beta","enabled":true}]}}),
    );
    let client = fixture.client();
    let mut spec = fixture.spec();
    let details = client.discover(&spec.cwd).await.unwrap();
    let feature = details
        .features
        .iter()
        .find(|feature| feature["id"] == "async_compaction")
        .unwrap();
    assert_eq!(feature["type"], "toggle");
    assert_eq!(feature["value"], true);
    assert_eq!(feature["label"], "Async compaction");
    spec.config =
        serde_json::from_value(json!({"providerOptions":{"features":{"async_compaction":false}}}))
            .unwrap();
    assert_eq!(client.settings(&spec.config)["features"][1]["value"], false);
    spec.config.feature_values = Some(std::collections::BTreeMap::from([(
        "async_compaction".into(),
        json!(true),
    )]));
    assert_eq!(client.settings(&spec.config)["features"][1]["value"], true);
    spec.config.model = Some("offline-model".into());
    let draft = client.draft_features(&spec).await.unwrap();
    assert!(
        draft
            .iter()
            .any(|feature| feature["id"] == "async_compaction")
    );
    let requests = fixture.requests();
    assert!(
        requests
            .iter()
            .any(|request| request["method"] == "experimentalFeature/list"
                && request["params"]["cursor"] == "next")
    );
    assert!(
        !requests
            .iter()
            .any(|request| request["method"] == "thread/start")
    );
}

#[tokio::test]
async fn async_compaction_toggle_requires_native_support_even_with_a_saved_selection() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let config =
        serde_json::from_value(json!({"featureValues":{"async_compaction":true}})).unwrap();
    assert!(
        !client
            .features(&config)
            .iter()
            .any(|feature| feature["id"] == "async_compaction")
    );

    supported(&fixture, false);
    let discovered = client.discover(&fixture.spec().cwd).await.unwrap();
    let feature = discovered
        .features
        .iter()
        .find(|feature| feature["id"] == "async_compaction")
        .unwrap();
    assert_eq!(feature["value"], false);
    assert!(
        client
            .features(&config)
            .iter()
            .any(|feature| feature["id"] == "async_compaction" && feature["value"] == true)
    );

    std::fs::remove_file(fixture.cwd.join("experimental-features.json")).unwrap();
    let discovered = client.discover(&fixture.spec().cwd).await.unwrap();
    assert!(
        !discovered
            .features
            .iter()
            .any(|feature| feature["id"] == "async_compaction")
    );
    assert!(
        !client
            .features(&config)
            .iter()
            .any(|feature| feature["id"] == "async_compaction")
    );
}

#[tokio::test]
async fn async_compaction_hides_unavailable_or_retired_flags_and_rejects_explicit_selections() {
    let fixture = Fixture::new();
    let client = fixture.client();
    let mut spec = fixture.spec();
    // The fixture's default is an older native server without the discovery method.
    for page in [
        None,
        Some(json!({"data":[]})),
        Some(json!({"data":[{"name":"async_compaction","stage":"removed","enabled":false}]})),
        Some(json!({"data":[{"name":"async_compaction","stage":"deprecated","enabled":true}]})),
        Some(json!({"data":[{"name":"async_compaction","stage":"unknown","enabled":true}]})),
    ] {
        if let Some(page) = page {
            pages(&fixture, &json!({"":page}));
        }
        let details = client.discover(&spec.cwd).await.unwrap();
        assert!(
            !details
                .features
                .iter()
                .any(|feature| feature["id"] == "async_compaction")
        );
        for enabled in [false, true] {
            spec.config =
                serde_json::from_value(json!({"featureValues":{"async_compaction":enabled}}))
                    .unwrap();
            assert_eq!(
                client.validate_selection(&spec).await,
                Err(AgentSessionError::Rejected)
            );
            spec.config = serde_json::from_value(
                json!({"providerOptions":{"features":{"async_compaction":enabled}}}),
            )
            .unwrap();
            assert_eq!(
                client.validate_selection(&spec).await,
                Err(AgentSessionError::Rejected)
            );
        }
    }
    spec.config = StoredAgentConfig::default();
    let mut session = client.create_session(&spec).await.unwrap();
    session.start_turn("legacy", &spec.config).await.unwrap();
    assert!(matches!(
        terminal(session.as_mut()).await.unwrap(),
        AgentTurnEvent::Completed(_)
    ));
    session.close().await.unwrap();
}

#[tokio::test]
async fn async_compaction_rejects_malformed_pages_and_cursor_loops() {
    let fixture = Fixture::new();
    let client = fixture.client();
    for page in [
        json!({}),
        json!({"data":[],"nextCursor":12}),
        json!({"data":[],"nextCursor":""}),
        json!({"data":[{"name":"async_compaction","stage":"stable","enabled":"true"}]}),
        json!({"data":vec![json!({"name":"other"});101]}),
    ] {
        pages(&fixture, &json!({"":page}));
        assert_eq!(
            client.discover(&fixture.spec().cwd).await.err(),
            Some(AgentSessionError::Failed)
        );
    }
    pages(
        &fixture,
        &json!({"":{"data":[],"nextCursor":"loop"},
        "loop":{"data":[],"nextCursor":"loop"}}),
    );
    assert_eq!(
        client.discover(&fixture.spec().cwd).await.err(),
        Some(AgentSessionError::Failed)
    );
    let mut catalog = serde_json::Map::new();
    catalog.insert(String::new(), json!({"data":[],"nextCursor":"0"}));
    for index in 0..64 {
        catalog.insert(
            index.to_string(),
            json!({"data":[],"nextCursor":(index + 1).to_string()}),
        );
    }
    pages(&fixture, &Value::Object(catalog));
    assert_eq!(
        client.discover(&fixture.spec().cwd).await.err(),
        Some(AgentSessionError::Failed)
    );
}

#[tokio::test]
async fn async_compaction_toggle_applies_on_next_turn_and_survives_resume() {
    let fixture = Fixture::new();
    supported(&fixture, false);
    let client = fixture.client();
    let mut spec = fixture.spec();
    spec.config =
        serde_json::from_value(json!({"featureValues":{"async_compaction":true}})).unwrap();
    let mut session = client.create_session(&spec).await.unwrap();
    let handle = session.persistence().unwrap();
    for selected in [Some(true), Some(false), Some(false), None] {
        spec.config.feature_values = selected.map(|enabled| {
            std::collections::BTreeMap::from([("async_compaction".into(), json!(enabled))])
        });
        session.start_turn("continue", &spec.config).await.unwrap();
        assert!(matches!(
            terminal(session.as_mut()).await.unwrap(),
            AgentTurnEvent::Completed(_)
        ));
        assert_eq!(session.persistence(), Some(handle.clone()));
    }
    session.close().await.unwrap();
    spec.config =
        serde_json::from_value(json!({"featureValues":{"async_compaction":true}})).unwrap();
    let mut restored = client
        .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
        .await
        .unwrap();
    restored.start_turn("resumed", &spec.config).await.unwrap();
    assert!(matches!(
        terminal(restored.as_mut()).await.unwrap(),
        AgentTurnEvent::Completed(_)
    ));
    restored.close().await.unwrap();
    let requests = fixture.requests();
    let starts: Vec<_> = requests
        .iter()
        .filter(|request| request["method"] == "thread/start")
        .collect();
    assert_eq!(starts.len(), 1);
    assert_eq!(
        starts[0]["params"]["config"]["features"]["async_compaction"],
        true
    );
    let resumes: Vec<_> = requests
        .iter()
        .filter(|request| request["method"] == "thread/resume")
        .collect();
    assert_eq!(resumes.len(), 3);
    assert_eq!(
        resumes[0]["params"]["config"]["features"]["async_compaction"],
        false
    );
    assert_eq!(resumes[1]["params"]["config"], json!({}));
    assert_eq!(
        resumes[2]["params"]["config"]["features"]["async_compaction"],
        true
    );
    assert!(
        resumes
            .iter()
            .all(|request| request["params"]["threadId"] == handle.session_id)
    );
    assert!(
        !requests
            .iter()
            .any(|request| request["method"] == "config/value/write"
                || request["method"] == "config/batchWrite")
    );
}

#[tokio::test]
async fn async_compaction_keeps_foreground_work_running_and_preserves_the_timeline() {
    let fixture = Fixture::new();
    supported(&fixture, true);
    let client = fixture.client();
    let mut spec = fixture.spec();
    let mut session = client.create_session(&spec).await.unwrap();
    // An active turn must never be killed just to change the thread configuration.
    let turn = session.start_turn("hang", &spec.config).await.unwrap();
    spec.config =
        serde_json::from_value(json!({"featureValues":{"async_compaction":false}})).unwrap();
    assert!(session.start_turn("too soon", &spec.config).await.is_err());
    session.cancel_turn(&turn).await.unwrap();
    assert_eq!(
        terminal(session.as_mut()).await.unwrap(),
        AgentTurnEvent::Cancelled
    );
    spec.config = StoredAgentConfig::default();
    session
        .start_turn("background-compaction", &spec.config)
        .await
        .unwrap();
    let mut compacted = 0;
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            assert!(!session.pending_foreground());
            match session.poll_turn().unwrap() {
                Some(AgentTurnEvent::Timeline(entry)) if entry.item["type"] == "compaction" => {
                    compacted += 1;
                    assert_eq!(entry.item["status"], "completed");
                }
                Some(AgentTurnEvent::Completed(_)) => break,
                Some(AgentTurnEvent::Failed | AgentTurnEvent::Cancelled) => {
                    panic!("foreground interrupted")
                }
                _ => {}
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(compacted, 1);
    session.close().await.unwrap();
    let history = client
        .history(&session.persistence().unwrap(), &spec.cwd)
        .await
        .unwrap();
    assert_eq!(
        history
            .iter()
            .filter(|entry| entry.item["type"] == "compaction")
            .count(),
        1
    );
    let requests = fixture.requests();
    let start = requests
        .iter()
        .find(|request| request["method"] == "thread/start")
        .unwrap();
    assert_eq!(start["params"]["config"], json!({}));
    assert!(
        !requests
            .iter()
            .any(|request| request["method"] == "thread/resume")
    );
}

#[test]
fn async_compaction_requires_boolean_values() {
    let fixture = Fixture::new();
    let client = fixture.client();
    for invalid in [Value::Null, json!("true"), json!(1), json!({})] {
        let config =
            serde_json::from_value(json!({"featureValues":{"async_compaction":invalid}})).unwrap();
        assert_eq!(
            client.validate_config(&config),
            Err(AgentSessionError::Unavailable)
        );
    }
    for enabled in [false, true] {
        let config =
            serde_json::from_value(json!({"featureValues":{"async_compaction":enabled}})).unwrap();
        client.validate_config(&config).unwrap();
    }
}

#[tokio::test]
async fn async_compaction_after_turn_completion_is_retained_once_without_blocking_input() {
    let fixture = Fixture::new();
    supported(&fixture, true);
    let client = fixture.client();
    let spec = fixture.spec();
    let mut session = client.create_session(&spec).await.unwrap();
    let turn = session
        .start_turn("background-compaction-after-turn", &spec.config)
        .await
        .unwrap();
    assert!(matches!(
        terminal(session.as_mut()).await.unwrap(),
        AgentTurnEvent::Completed(_)
    ));
    let entry = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Some(event) = session.poll_turn().unwrap() {
                break event;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let AgentTurnEvent::Timeline(entry) = entry else {
        panic!("expected a late compaction entry");
    };
    assert_eq!(entry.turn_id.as_deref(), Some(turn.as_str()));
    assert_eq!(entry.item["type"], "compaction");
    assert_eq!(session.poll_turn().unwrap(), None);
    assert!(!session.pending_foreground());
    session
        .start_turn("continue after compaction", &spec.config)
        .await
        .unwrap();
    assert!(matches!(
        terminal(session.as_mut()).await.unwrap(),
        AgentTurnEvent::Completed(_)
    ));
    session.close().await.unwrap();
}
