use super::*;

#[tokio::test]
async fn standard_acp_roles_use_string_choices_and_keep_the_public_session_identity() {
    let (_root, client, spec) = super::super::super::tests::fixture();
    let mut peer = Peer::new(client);
    let session = peer.open(&spec.cwd, false).await;
    for value in ["other/model", "native"] {
        let result = peer
            .request(
                "session/set_config_option",
                json!({"sessionId":session,
            "configId":"modelRole.smol","value":value}),
            )
            .await;
        assert!(result.get("error").is_none(), "{result:?}");
        let option = result["result"]["configOptions"]
            .as_array()
            .unwrap()
            .iter()
            .find(|option| option["id"] == "modelRole.smol")
            .unwrap();
        assert_eq!(option["currentValue"], value);
        assert!(
            option["options"]
                .as_array()
                .unwrap()
                .iter()
                .any(|option| option["value"] == value)
        );
        assert!(
            !option["options"]
                .as_array()
                .unwrap()
                .iter()
                .any(|option| option["value"] == "true")
        );
    }
    let rejected = peer
        .request(
            "session/set_config_option",
            json!({"sessionId":session,
        "configId":"modelRole.image","value":"other/model"}),
        )
        .await;
    assert!(rejected.get("error").is_some());
    let result = peer
        .request(
            "session/prompt",
            json!({"sessionId":session,"prompt":[{"type":"text","text":"after role changes"}]}),
        )
        .await;
    assert_eq!(result["result"]["stopReason"], "end_turn");
    peer.finish(&session).await;
}
