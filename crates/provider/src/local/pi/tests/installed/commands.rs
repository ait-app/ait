use super::*;

#[tokio::test]
#[ignore = "requires Pi >= 1.2 on PATH; isolated resources and loopback model only"]
async fn installed_pi_catalog_names_invoke_native_resources_and_respect_trust() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let native = root.join("native");
    write_resources(&root, &native);
    let (address, requests, _server) = command_model().await;
    std::fs::write(native.join("models.json"), json!({"providers":{"ait-fixture":{
        "baseUrl":format!("http://{address}/v1"),"api":"openai-completions","apiKey":"fixture-only",
        "models":[{"id":"fixture-model","reasoning":false,"input":["text"],
        "contextWindow":8192,"maxTokens":128}]
    }}}).to_string()).unwrap();
    let mut client = PiClient::new("pi".into());
    client.environment = AgentEnvironment::try_from(BTreeMap::from([
        (
            "PI_CODING_AGENT_DIR".into(),
            native.to_str().unwrap().into(),
        ),
        ("HOME".into(), root.to_str().unwrap().into()),
    ]))
    .unwrap();
    client.session_environment = history::SessionEnvironment::isolated(&root, &native);
    let spec = AgentSessionSpec {
        provider: PROVIDER.into(),
        cwd: root.to_str().unwrap().into(),
        config: StoredAgentConfig {
            model: Some("ait-fixture/fixture-model".into()),
            ..StoredAgentConfig::default()
        },
        confinement: None,
    };
    assert_native_command_contract(&client, &spec).await;
    let catalog = client.commands(&spec).await.unwrap();
    for (name, kind) in [
        ("catalog-extension", "command"),
        ("user-template", "command"),
        ("skill:user-skill", "skill"),
    ] {
        let command = catalog.iter().find(|entry| entry["name"] == name).unwrap();
        assert_eq!(command["kind"], kind, "{catalog:?}");
    }
    assert!(!catalog.iter().any(|entry| matches!(
        entry["name"].as_str(),
        Some("project-template" | "skill:project-skill")
    )));
    assert!(!native.join("sessions").exists());
    assert!(!native.join("trust.json").exists());
    let mut session = client.create_session(&spec).await.unwrap();
    for name in ["catalog-extension", "user-template", "skill:user-skill"] {
        let command = catalog.iter().find(|entry| entry["name"] == name).unwrap();
        let input = format!("/{} catalog-argument", command["name"].as_str().unwrap());
        session.start_turn(&input, &spec.config).await.unwrap();
        let events = terminal(session.as_mut()).await;
        assert!(
            matches!(events.last(), Some(AgentTurnEvent::Completed(_))),
            "{events:?}"
        );
    }
    session.close().await.unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("extension-invoked")).unwrap(),
        "catalog-argument"
    );
    let recorded = requests.lock().unwrap().clone();
    assert_eq!(recorded.len(), 2);
    assert!(
        recorded[0]["messages"]
            .to_string()
            .contains("user-template-expanded catalog-argument")
    );
    assert!(
        recorded[1]["messages"]
            .to_string()
            .contains("user-skill-expanded")
    );
    // Set trust only in this test's isolated native settings, never the user's trust store.
    std::fs::write(
        native.join("settings.json"),
        r#"{"defaultProjectTrust":"always"}"#,
    )
    .unwrap();
    let trusted = client.commands(&spec).await.unwrap();
    for (name, kind) in [
        ("project-template", "command"),
        ("skill:project-skill", "skill"),
    ] {
        let command = trusted.iter().find(|entry| entry["name"] == name).unwrap();
        assert_eq!(command["kind"], kind, "{trusted:?}");
    }
    assert!(!native.join("trust.json").exists());
}

async fn assert_native_command_contract(client: &PiClient, spec: &AgentSessionSpec) {
    // Assert the native contract before projection: a peer change must fail this check even
    // if host mapping later starts filtering unsupported commands.
    let mut peer =
        transport::Transport::spawn(client, &spec.cwd, &["--no-session".into()], None).unwrap();
    let native_catalog = peer.request(json!({"type":"get_commands"})).await.unwrap();
    peer.close().await.unwrap();
    let projected = client.commands(spec).await.unwrap();
    let native_commands = native_catalog["commands"].as_array().unwrap();
    for (name, source) in [
        ("catalog-extension", "extension"),
        ("user-template", "prompt"),
        ("skill:user-skill", "skill"),
    ] {
        let command = native_commands
            .iter()
            .find(|entry| entry["name"] == name)
            .unwrap();
        let host = projected
            .iter()
            .find(|entry| entry["name"] == name)
            .unwrap();
        assert_eq!(
            host["argumentHint"],
            command["argumentHint"].as_str().unwrap_or_default()
        );
        assert_eq!(command["source"], source);
        assert!(command["description"].as_str().is_some());
    }
    assert!(
        !native_commands
            .iter()
            .any(|entry| matches!(entry["name"].as_str(), Some("settings" | "hotkeys"))),
        "TUI commands leaked into the native RPC catalog: {native_catalog:?}"
    );
}

fn write_resources(root: &std::path::Path, native: &std::path::Path) {
    for base in [native, &root.join(".pi")] {
        std::fs::create_dir_all(base.join("prompts")).unwrap();
        std::fs::create_dir_all(base.join("skills/catalog-skill")).unwrap();
        let scope = if base == native { "user" } else { "project" };
        std::fs::write(
            base.join(format!("prompts/{scope}-template.md")),
            format!(
                "---\ndescription: Catalog template\nargument-hint: <topic>\n---\n{scope}-template-expanded $ARGUMENTS"
            ),
        )
        .unwrap();
        std::fs::write(
            base.join("skills/catalog-skill/SKILL.md"),
            format!(
                "---\nname: {scope}-skill\ndescription: Catalog skill\n---\n{scope}-skill-expanded"
            ),
        )
        .unwrap();
    }
    std::fs::create_dir_all(native.join("extensions")).unwrap();
    std::fs::write(
        native.join("extensions/catalog.js"),
        r"
import { writeFileSync } from 'node:fs';
export default function(pi) {
  pi.registerCommand('catalog-extension', {
    description: 'Catalog extension',
    handler: async (args, ctx) => { writeFileSync('extension-invoked', args); }
  });
}
",
    )
    .unwrap();
}

async fn command_model() -> (
    std::net::SocketAddr,
    Arc<Mutex<Vec<Value>>>,
    AbortOnDropHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let observed = requests.clone();
    let app = Router::new().route(
        "/v1/chat/completions",
        post(move |Json(request): Json<Value>| {
            observed.lock().unwrap().push(request);
            async {
                let chunk = json!({"id":"catalog","object":"chat.completion.chunk","created":1,
                "model":"fixture-model","choices":[{"index":0,"delta":{"role":"assistant",
                "content":"Catalog invoked"},"finish_reason":"stop"}]});
                (
                    [("content-type", "text/event-stream")],
                    format!("data: {chunk}\n\ndata: [DONE]\n\n"),
                )
            }
        }),
    );
    let server = AbortOnDropHandle::new(tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    }));
    (address, requests, server)
}
