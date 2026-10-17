use super::*;
use crate::config::Cli;
use clap::Parser;

fn config(directory: &std::path::Path) -> Config {
    Config::load(
        Cli::parse_from([
            "server",
            "--data-dir",
            directory.to_str().unwrap(),
            "--listen",
            "127.0.0.1:0",
        ]),
        |name| {
            (name == "AIT_SERVER_TOKEN")
                .then(|| "offline-host-test-token-at-least-32-characters".into())
        },
        |token| api::validate_token(token).map_err(Into::into),
        |origin| api::validate_browser_origin(origin).map_err(Into::into),
    )
    .unwrap()
}

#[tokio::test]
async fn binding_failure_has_no_state_side_effects() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("absent");
    let reserved = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let mut configuration = config(&directory);
    configuration.listen = reserved.local_addr().unwrap();
    assert!(Server::bind(configuration).await.is_err());
    assert!(!directory.exists());
}

#[tokio::test]
async fn shutdown_releases_lock_and_restart_keeps_only_stable_identity() {
    let root = tempfile::tempdir().unwrap();
    let server = Server::bind(config(root.path())).await.unwrap();
    let address = server.address();
    assert!(Server::bind(config(root.path())).await.is_err());
    let (stop, signal) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(server.serve(async move {
        let _ = signal.await;
    }));
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let first: serde_json::Value = client
        .get(format!("http://{address}/v1/server/info"))
        .bearer_auth("offline-host-test-token-at-least-32-characters")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    stop.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), task)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    let restarted = Server::bind(config(root.path())).await.unwrap();
    assert_eq!(restarted.instance.server_id.to_string(), first["server_id"]);
    assert_ne!(
        restarted.instance.instance_id.to_string(),
        first["instance_id"]
    );
    restarted.serve(async {}).await.unwrap();
}

#[tokio::test]
async fn each_composite_service_installs_all_its_methods_without_other_components() {
    let root = tempfile::tempdir().unwrap();
    let configuration = config(root.path());
    let instance = Arc::new(InstanceLease::acquire(root.path()).unwrap());
    let mut services =
        compose_services(&configuration, "127.0.0.1:7316".parse().unwrap(), &instance).unwrap();
    let cases = [
        (
            Services {
                browser: services.browser.take(),
                ..Services::default()
            },
            ::browser::capabilities::implemented_methods().collect::<Vec<_>>(),
        ),
        (
            Services {
                metadata: services.metadata.take(),
                ..Services::default()
            },
            metadata::capabilities::implemented_methods().collect::<Vec<_>>(),
        ),
        (
            Services {
                filesystem: services.filesystem.take(),
                ..Services::default()
            },
            filesystem::capabilities::implemented_methods().collect::<Vec<_>>(),
        ),
        (
            Services {
                bot: services.bot.take(),
                ..Services::default()
            },
            ::bot::capabilities::implemented_methods().collect::<Vec<_>>(),
        ),
        (
            Services {
                provider: services.provider.take(),
                ..Services::default()
            },
            provider::capabilities::implemented_methods().collect::<Vec<_>>(),
        ),
    ];
    for (installed, methods) in cases {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let api = Api::new(
            address,
            instance.server_id.to_string(),
            instance.instance_id.to_string(),
            configuration.token.clone(),
            installed,
        )
        .unwrap();
        let (stop, signal) = tokio::sync::oneshot::channel();
        let server = Server {
            listener,
            api,
            instance: instance.clone(),
            managed: None,
        };
        let task = tokio::spawn(server.serve(async move {
            let _ = signal.await;
        }));
        let info: model::server::ServerInfo = reqwest::Client::builder()
            .no_proxy()
            .build()
            .unwrap()
            .get(format!("http://{address}/v1/server/info"))
            .bearer_auth("offline-host-test-token-at-least-32-characters")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        assert_eq!(info.implemented_capabilities.len(), 12 + methods.len());
        assert!(methods.iter().all(|method| {
            info.implemented_capabilities
                .iter()
                .any(|name| name == method.name)
        }));
        assert_eq!(info.capabilities.len(), 205);
        stop.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
    let remaining = Api::new(
        "127.0.0.1:7316".parse().unwrap(),
        instance.server_id.to_string(),
        instance.instance_id.to_string(),
        configuration.token,
        services,
    )
    .unwrap();
    remaining.begin_shutdown();
    remaining.wait_closed().await;
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod headless;
mod workspace_attention;

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test]
async fn headless_host_composes_exclusive_internal_control_without_contacting_center() {
    use host_link::{Binding, Credential, CredentialStore, Machine, State};
    let root = tempfile::tempdir().unwrap();
    let lease = InstanceLease::acquire(root.path()).unwrap();
    let server_id = lease.server_id;
    drop(lease);
    let store = crate::device::Store::open(root.path()).unwrap();
    store
        .save(&State {
            machine: Machine {
                server_id,
                display_name: "build-linux".into(),
                platform: "linux".into(),
                app_version: "previous".into(),
            },
            credential: Some(Credential {
                refresh_token: "test-private-refresh".into(),
                refresh_expires_at: Utc::now() + chrono::Duration::days(30),
                binding: Binding {
                    node_id: uuid::Uuid::new_v4(),
                    host_id: uuid::Uuid::new_v4(),
                    server_id,
                    grant_id: uuid::Uuid::new_v4(),
                },
            }),
            pending: None,
        })
        .unwrap();
    drop(store);
    let mut configuration = config(root.path());
    configuration.headless = true;
    let mut server = Server::bind(configuration).await.unwrap();
    assert!(server.managed.is_some());
    assert!(server.api.claim_managed_relay().is_err());
    assert!(crate::device::Store::open(root.path()).is_err());
    server.api.begin_shutdown();
    server.api.wait_closed().await;
    drop(server);
    assert!(crate::device::Store::open(root.path()).is_ok());
}
