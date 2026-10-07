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
        };
        let task = tokio::spawn(server.serve(async move {
            let _ = signal.await;
        }));
        let info: protocol::ServerInfo = reqwest::Client::builder()
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
        assert_eq!(info.capabilities.len(), 180);
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
