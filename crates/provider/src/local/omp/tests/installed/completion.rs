use super::*;
use crate::protocol::commands::CompletionInput;

const NESTED_COMPLETION_EXTENSION: &str = r#"
export default function(pi) {
  pi.on("session_start", (_event, ctx) => {
    ctx.ui.addAutocompleteProvider(base => ({
      getSuggestions(lines, line, col, signal) {
        if (lines[line] === "/native-no-op") {
          return { prefix: "/native-no-op", items: [{ value: "native-no-op", label: "No-op" }] };
        }
        return base.getSuggestions(lines, line, col, signal);
      },
      applyCompletion(lines, line, col, item, prefix) {
        if (item.value === "native-no-op") return { lines, cursorLine: line, cursorCol: col };
        return base.applyCompletion(lines, line, col, item, prefix);
      }
    }));
  });
  pi.registerCommand("native-nested", {
    description: "Native dynamic nested completion",
    getArgumentCompletions(prefix) {
      if (prefix === "") return [{ value: "first ", label: "first", description: "Native first" }];
      if (prefix === "first ") return [{ value: "first second ", label: "second", description: "Native second" }];
      if (prefix === "first second ") return [{ value: "first second third ", label: "third", description: "Native third" }];
      return null;
    },
    handler: async () => {}
  });
}
"#;

#[tokio::test]
#[ignore = "requires AIT_TEST_OMP_COMPLETION_BIN with the proposed upstream ACP extension"]
async fn upstream_completion_is_available_on_native_acp_without_a_second_rpc_runtime() {
    let program =
        std::env::var_os("AIT_TEST_OMP_COMPLETION_BIN").expect("set AIT_TEST_OMP_COMPLETION_BIN");
    let fixture = installed_with_program(program).await;
    let mut command = tokio::process::Command::new(&fixture.client.program);
    command
        .args([
            "--mode",
            "acp",
            "--no-pty",
            "--no-session",
            "--provider",
            "ait-local-fixture",
            "--model",
            "fixture",
        ])
        .envs(fixture.client.environment.entries());
    let mut transport = crate::local::acp_transport::Transport::spawn(
        command,
        &fixture.spec.cwd,
        Duration::from_secs(30),
        "Oh My Pi",
    )
    .unwrap();
    let initialized = transport
        .request(
            "initialize",
            json!({"protocolVersion":1,
        "clientCapabilities":{"elicitation":{"form":{}}},
        "clientInfo":{"name":"ait-completion-audit","version":"1"}}),
        )
        .await
        .unwrap();
    assert_eq!(initialized["_meta"]["ompCommandCompletion"]["version"], 1);
    let created = transport
        .request(
            "session/new",
            json!({"cwd":fixture.spec.cwd,"mcpServers":[]}),
        )
        .await
        .unwrap();
    let result = transport
        .request(
            "_omp/commands/complete",
            json!({
                "sessionId":created["sessionId"],"text":"/advisor o","cursor":10
            }),
        )
        .await
        .unwrap();
    assert_eq!(result["supported"], true);
    assert!(
        result["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["label"] == "on")
    );
    transport
        .request("session/close", json!({"sessionId":created["sessionId"]}))
        .await
        .unwrap();
    transport.close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires AIT_TEST_OMP_COMPLETION_BIN with the proposed upstream completion capability"]
async fn upstream_completion_runs_original_builtin_and_arbitrary_nested_plugin_completers() {
    let program =
        std::env::var_os("AIT_TEST_OMP_COMPLETION_BIN").expect("set AIT_TEST_OMP_COMPLETION_BIN");
    let fixture = installed_with_program(program).await;
    let extension =
        std::path::Path::new(&fixture.spec.cwd).join("agent/extensions/nested-completion.ts");
    std::fs::write(extension, NESTED_COMPLETION_EXTENSION).unwrap();
    let mut session = fixture.client.create_session(&fixture.spec).await.unwrap();
    let no_op = session
        .complete_commands(&CompletionInput {
            text: "/native-no-op".into(),
            cursor: 13,
        })
        .await
        .unwrap();
    assert!(no_op.supported);
    assert!(
        no_op.items.is_empty(),
        "unchanged edits must not trap Enter in completion"
    );
    for (text, label) in [
        ("/advisor o", "on"),
        ("/native-nested ", "first"),
        ("/native-nested first ", "second"),
        ("/native-nested first second ", "third"),
    ] {
        let input = CompletionInput {
            text: text.into(),
            cursor: text.encode_utf16().count(),
        };
        let result = session.complete_commands(&input).await.unwrap();
        assert!(result.supported);
        assert!(
            result.items.iter().any(|item| item.label == label),
            "{text}: {result:?}"
        );
        assert!(result.items.iter().all(|item| item.trigger_completion));
    }
    let result = session
        .complete_commands(&CompletionInput {
            text: "/native-nested first second no-match".into(),
            cursor: 36,
        })
        .await
        .unwrap();
    assert!(result.supported && result.items.is_empty());
    session
        .start_turn("/compat-vendor-alpha", &fixture.spec.config)
        .await
        .unwrap();
    let question = loop {
        if let AgentTurnEvent::PermissionRequested(question) = next(session.as_mut()).await.unwrap()
        {
            break question;
        }
    };
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        session.complete_commands(&CompletionInput {
            text: "/native-nested first ".into(),
            cursor: 21,
        }),
    )
    .await
    .expect("completion must overtake a command waiting on extension UI")
    .unwrap();
    assert_eq!(result.items[0].label, "second");
    assert_eq!(session.pending_permissions(), vec![question.clone()]);
    session
        .respond_permission(
            question["id"].as_str().unwrap(),
            &json!({"behavior":"allow",
        "updatedInput":{"answers":{"value":"A, B"}}}),
        )
        .await
        .unwrap();
    complete(session.as_mut()).await;
    session.close().await.unwrap();
}
