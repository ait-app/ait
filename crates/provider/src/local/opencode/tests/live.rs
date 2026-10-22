//! Opt-in loading measurements with native configuration, isolated storage and no inference.
use std::{path::PathBuf, time::Instant};

use super::*;

fn live_profile() -> (TempDir, OpenCodeClient, AgentSessionSpec) {
    let binary = std::env::var_os("AIT_TEST_OPENCODE_BIN").expect("set AIT_TEST_OPENCODE_BIN");
    let configuration = std::env::var_os("AIT_TEST_OPENCODE_LIVE_CONFIG")
        .expect("set AIT_TEST_OPENCODE_LIVE_CONFIG to the private native configuration");
    let model = std::env::var("AIT_TEST_OPENCODE_LIVE_MODEL")
        .expect("set AIT_TEST_OPENCODE_LIVE_MODEL to an authorized model");
    let root = tempfile::tempdir().unwrap();
    let cwd = root
        .path()
        .canonicalize()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    std::fs::copy(configuration, root.path().join("opencode.jsonc")).unwrap();
    if let Some(auth) = std::env::var_os("AIT_TEST_OPENCODE_LIVE_AUTH") {
        let data = root.path().join("data/opencode");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::copy(auth, data.join("auth.json")).unwrap();
    }
    let mut client = OpenCodeClient::new(PathBuf::from(binary));
    client.environment = BTreeMap::from([
        ("XDG_DATA_HOME".into(), format!("{cwd}/data")),
        ("XDG_CONFIG_HOME".into(), format!("{cwd}/config")),
        ("XDG_CACHE_HOME".into(), format!("{cwd}/cache")),
        ("XDG_STATE_HOME".into(), format!("{cwd}/state")),
        ("OPENCODE_TEST_HOME".into(), cwd.clone()),
        ("OPENCODE_DB".into(), format!("{cwd}/opencode.db")),
        ("OPENCODE_CONFIG_DIR".into(), format!("{cwd}/config")),
        ("OPENCODE_CONFIG_CONTENT".into(), "{}".into()),
        ("OPENCODE_CLI_CONFIG_CONTENT".into(), "{}".into()),
    ]);
    let spec = AgentSessionSpec {
        provider: "opencode".into(),
        cwd,
        config: StoredAgentConfig {
            model: Some(model),
            ..Default::default()
        },
    };
    (root, client, spec)
}

async fn live_startup(client: &OpenCodeClient, spec: &AgentSessionSpec) -> Box<dyn AgentSession> {
    let started = Instant::now();
    if std::env::var_os("AIT_TEST_OPENCODE_LIVE_ACP_PROBE").is_some() {
        // Reproduce the old availability probe while retaining the same native catalog path.
        let cwd = std::env::current_dir().unwrap();
        let (mut probe, _) =
            launcher::spawn(client, cwd.to_str().unwrap(), &StoredAgentConfig::default())
                .await
                .unwrap();
        probe.close().await.unwrap();
    } else {
        assert!(client.is_available().await.unwrap());
    }
    eprintln!(
        "live phase=availability elapsed_ms={}",
        started.elapsed().as_millis()
    );
    let started = Instant::now();
    let catalog = client.discover(&spec.cwd).await.unwrap();
    assert!(
        catalog
            .models
            .iter()
            .any(|model| model["id"] == spec.config.model.as_deref().unwrap())
    );
    eprintln!(
        "live phase=catalog elapsed_ms={} models={}",
        started.elapsed().as_millis(),
        catalog.models.len()
    );
    let started = Instant::now();
    let session = client.create_session(spec).await.unwrap();
    eprintln!(
        "live phase=create elapsed_ms={}",
        started.elapsed().as_millis()
    );
    session
}

#[tokio::test]
#[ignore = "requires an installed CLI and authorized LIVE_CONFIG/MODEL; no model inference"]
async fn live_profile_startup_without_inference() {
    for _ in 0..3 {
        let (_root, client, spec) = live_profile();
        let mut session = live_startup(&client, &spec).await;
        let handle = session.persistence().unwrap();
        session.close().await.unwrap();
        let started = Instant::now();
        let mut restored = client
            .resume_session(&handle, &spec, AgentResumePurpose::Interactive)
            .await
            .unwrap();
        assert_eq!(
            restored.persistence().unwrap().session_id,
            handle.session_id
        );
        eprintln!(
            "live phase=resume elapsed_ms={}",
            started.elapsed().as_millis()
        );
        restored.close().await.unwrap();
    }
}
