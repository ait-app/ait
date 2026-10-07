use super::*;
use crate::{
    local::opencode::tests::fixture::Fixture,
    ports::agent_session::{AgentClient, AgentResumePurpose, AgentSessionSpec},
};

#[tokio::test]
async fn lists_and_imports_external_history_without_mutations_then_resumes() {
    for version in [Version::V1, Version::V2] {
        let fixture = Fixture::start(version).await;
        let client = OpenCodeClient::new(fixture.binary.clone());
        let cwd = fixture.cwd.to_str().unwrap();
        {
            let mut state = fixture.state.lock().unwrap();
            state.model = json!({"providerID":"local","id":"test-model","variant":"high"});
            state.history = match version {
                Version::V1 => vec![
                    json!({"info":{"id":"user1","sessionID":"ses_one","role":"user","model":{"providerID":"local","modelID":"test-model"},"time":{"created":1}},"parts":[{"id":"part_user","sessionID":"ses_one","messageID":"user1","type":"text","text":"native prompt"}]}),
                    json!({"info":{"id":"answer1","sessionID":"ses_one","role":"assistant","time":{"created":2,"completed":3}},"parts":[{"id":"part_answer","sessionID":"ses_one","messageID":"answer1","type":"text","text":"native answer"}]}),
                ],
                Version::V2 => vec![
                    json!({"id":"user1","type":"user","text":"native prompt","time":{"created":1}}),
                    json!({"id":"answer1","type":"assistant","content":[{"type":"text","text":"native answer"}],"time":{"created":2,"completed":3}}),
                ],
            };
        }
        assert!(client.supports_session_import());
        let listed = client
            .list_sessions(&ListOptions {
                cwd: Some(cwd.into()),
                scan_limit: 20,
            })
            .await
            .unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(
            listed[0].first_prompt_preview.as_deref(),
            Some("native prompt")
        );
        assert_eq!(
            listed[0].last_prompt_preview.as_deref(),
            Some("native prompt")
        );
        assert_eq!(listed[0].title.as_deref(), Some("Existing session"));
        let mut handle = AgentPersistenceHandle {
            provider: "opencode".into(),
            session_id: listed[0].provider_handle_id.clone(),
            native_handle: None,
            metadata: None,
        };
        let history = client
            .inspect_session(&handle, cwd)
            .await
            .unwrap_or_else(|error| panic!("inspect {version:?}: {error:?}"));
        assert_eq!(history.entries.len(), 2);
        assert_eq!(history.config.model.as_deref(), Some("local/test-model"));
        assert_eq!(
            history.descriptor.first_prompt_preview.as_deref(),
            Some("native prompt")
        );
        assert_eq!(fixture.state.lock().unwrap().submissions, 0);
        assert_eq!(fixture.state.lock().unwrap().permission_updates, 0);
        handle.metadata = Some(history.resume_metadata);
        let spec = AgentSessionSpec {
            provider: "opencode".into(),
            cwd: cwd.into(),
            config: history.config,
        };
        let mut reader = client
            .resume_session(&handle, &spec, AgentResumePurpose::History)
            .await
            .unwrap();
        reader.close().await.unwrap();
        assert_eq!(fixture.state.lock().unwrap().permission_updates, 0);
        {
            let mut writer = client
                .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
                .await
                .unwrap();
            assert_eq!(writer.persistence().unwrap().session_id, "ses_one");
            writer.close().await.unwrap();
        }
    }
}

#[tokio::test]
async fn rejects_wrong_directory_busy_history_and_invalid_scan_limits() {
    let fixture = Fixture::start(Version::V2).await;
    let client = OpenCodeClient::new(fixture.binary.clone());
    let handle = AgentPersistenceHandle {
        provider: "opencode".into(),
        session_id: "ses_one".into(),
        native_handle: None,
        metadata: None,
    };
    assert!(client.inspect_session(&handle, "/").await.is_err());
    {
        let mut state = fixture.state.lock().unwrap();
        state.model = json!({"providerID":"local","id":"test-model"});
        state.busy = true;
    }
    assert!(
        client
            .inspect_session(&handle, fixture.cwd.to_str().unwrap())
            .await
            .is_err()
    );
    for scan_limit in [0, 4097] {
        assert!(
            client
                .list_sessions(&ListOptions {
                    cwd: None,
                    scan_limit
                })
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn native_default_variant_imports_without_requiring_a_catalog_variant() {
    let fixture = Fixture::start(Version::V2).await;
    fixture.state.lock().unwrap().model =
        json!({"providerID":"local","id":"test-model","variant":"default"});
    let client = OpenCodeClient::new(fixture.binary.clone());
    let handle = AgentPersistenceHandle {
        provider: "opencode".into(),
        session_id: "ses_one".into(),
        native_handle: None,
        metadata: None,
    };
    let history = client
        .inspect_session(&handle, fixture.cwd.to_str().unwrap())
        .await
        .unwrap();
    assert_eq!(history.config.thinking_option_id, None);
    assert!(history.entries.is_empty());
}

#[test]
fn validates_native_descriptors_and_timestamps() {
    let info = json!({"id":"ses_one","location":{"directory":"/tmp"},"time":{"updated":0}});
    assert_eq!(
        descriptor(Version::V2, &info).unwrap().last_activity_at,
        "1970-01-01T00:00:00+00:00"
    );
    for (pointer, invalid) in [
        ("/id", json!("../escape")),
        ("/location/directory", json!("relative")),
        ("/time/updated", Value::Null),
    ] {
        let mut malformed = info.clone();
        *malformed.pointer_mut(pointer).unwrap() = invalid;
        assert!(descriptor(Version::V2, &malformed).is_err());
    }
}

#[tokio::test]
async fn follows_session_cursors_and_preserves_directory_filters() {
    let fixture = Fixture::start(Version::V2).await;
    let client = OpenCodeClient::new(fixture.binary.clone());
    let row = json!({"id":"ses_first","location":{"directory":fixture.cwd},"time":{"updated":2}});
    let mut second = row.clone();
    second["id"] = json!("ses_second");
    fixture.state.lock().unwrap().session_pages = vec![
        json!({"data":[row],"cursor":{"next":"page + & 2"}}),
        json!({"data":[second],"cursor":{"next":null}}),
    ];
    let result = client
        .list_sessions(&ListOptions {
            cwd: Some(fixture.cwd.to_str().unwrap().into()),
            scan_limit: 150,
        })
        .await
        .unwrap();
    assert_eq!(result.len(), 2);
    let state = fixture.state.lock().unwrap();
    assert_eq!(state.session_queries.len(), 2);
    assert!(state.session_queries[0].contains(&("order".into(), "desc".into())));
    assert!(state.session_queries[1].contains(&("cursor".into(), "page + & 2".into())));
    for query in &state.session_queries {
        assert_eq!(
            query.iter().filter(|(key, _)| key == "directory").count(),
            1
        );
    }
}

#[tokio::test]
async fn global_listing_is_unscoped_and_repeated_or_malformed_cursors_fail() {
    for version in [Version::V1, Version::V2] {
        let fixture = Fixture::start(version).await;
        let client = OpenCodeClient::new(fixture.binary.clone());
        client
            .list_sessions(&ListOptions {
                cwd: None,
                scan_limit: 20,
            })
            .await
            .unwrap();
        assert!(
            fixture.state.lock().unwrap().session_queries[0]
                .iter()
                .all(|(key, _)| key != "directory")
        );
    }
    for next in [json!("repeated"), json!(5)] {
        let fixture = Fixture::start(Version::V2).await;
        let client = OpenCodeClient::new(fixture.binary.clone());
        fixture.state.lock().unwrap().session_pages = vec![
            json!({"data":[],"cursor":{"next":next}}),
            json!({"data":[],"cursor":{"next":next}}),
        ];
        assert!(
            client
                .list_sessions(&ListOptions {
                    cwd: None,
                    scan_limit: 20
                })
                .await
                .is_err()
        );
    }
}

#[tokio::test]
#[ignore = "reads existing native sessions; requires AIT_TEST_OPENCODE_BIN and AIT_TEST_OPENCODE_CWD"]
async fn installed_existing_sessions_are_discovered_and_inspected_without_submission() {
    let binary = std::env::var("AIT_TEST_OPENCODE_BIN").expect("set AIT_TEST_OPENCODE_BIN");
    let cwd = std::env::var("AIT_TEST_OPENCODE_CWD").expect("set AIT_TEST_OPENCODE_CWD");
    let client = OpenCodeClient::new(binary.into());
    let entries = client
        .list_sessions(&ListOptions {
            cwd: Some(cwd),
            scan_limit: 100,
        })
        .await
        .unwrap();
    assert!(
        !entries.is_empty(),
        "the requested directory must contain existing sessions"
    );
    let mut inspected = 0;
    for entry in &entries {
        let handle = AgentPersistenceHandle {
            provider: "opencode".into(),
            session_id: entry.provider_handle_id.clone(),
            native_handle: None,
            metadata: None,
        };
        if let Ok(history) = client.inspect_session(&handle, &entry.cwd).await {
            assert_eq!(
                history.descriptor.provider_handle_id,
                entry.provider_handle_id
            );
            inspected += 1;
        }
    }
    eprintln!(
        "discovered {} existing sessions; inspected {inspected}",
        entries.len()
    );
    assert!(inspected > 0, "no existing session could be inspected");
}

#[test]
fn discovery_previews_ignore_tools_assistants_and_synthetic_user_parts() {
    assert!(prompt(Version::V2, &json!({"type":"assistant","text":"not user"})).is_none());
    let message = json!({"info":{"role":"user"},"parts":[
        {"type":"text","synthetic":true,"text":"injected"},
        {"type":"text","ignored":true,"text":"ignored"},
        {"type":"file","text":"binary"},
        {"type":"text","text":"first\n question"},
        {"type":"text","text":"continued"}
    ]});
    assert_eq!(
        prompt(Version::V1, &message).as_deref(),
        Some("first question continued")
    );
}

#[tokio::test]
async fn previews_keep_first_and_last_user_text_separate_from_assistant_messages() {
    let fixture = Fixture::start(Version::V2).await;
    fixture.state.lock().unwrap().history = vec![
        json!({"type":"user","text":"first question"}),
        json!({"type":"assistant","text":"answer"}),
        json!({"type":"user","text":"last question"}),
    ];
    let client = OpenCodeClient::new(fixture.binary.clone());
    let entries = client
        .list_sessions(&ListOptions {
            cwd: Some(fixture.cwd.to_str().unwrap().into()),
            scan_limit: 20,
        })
        .await
        .unwrap();
    assert_eq!(
        entries[0].first_prompt_preview.as_deref(),
        Some("first question")
    );
    assert_eq!(
        entries[0].last_prompt_preview.as_deref(),
        Some("last question")
    );
    assert_eq!(fixture.state.lock().unwrap().submissions, 0);
}
