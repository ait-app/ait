use super::*;

#[tokio::test]
async fn checkout_refresh_survives_busy_catalog_jobs_and_keeps_the_socket_usable() {
    for single in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let repository = root.path().join("repository");
        std::fs::create_dir(&repository).unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "--quiet", "--initial-branch=main"])
                .current_dir(&repository)
                .status()
                .unwrap()
                .success()
        );
        std::fs::write(repository.join("changed.txt"), "first\n").unwrap();
        let fixture = Fixture::with_services(Services {
            filesystem: Some(filesystem_service::service(root.path())),
            ..Services::default()
        })
        .await;
        let mut socket = fixture.socket().await;
        let mut greeting = hello();
        greeting["capabilities"] = json!([
            "connection.ping",
            "checkout.refresh.request",
            "checkout.status.get.request",
            "checkout.diff.subscribe.request"
        ]);
        if single {
            greeting["required_capabilities"] = json!(["connection.single.v1"]);
        }
        send(&mut socket, greeting).await;
        receive(&mut socket).await;
        let held = fixture
            .api
            .shared
            .jobs
            .clone()
            .acquire_many_owned(3)
            .await
            .unwrap();

        let reply = request(
            &mut socket,
            "checkout.refresh.request",
            json!({"cwd":repository}),
        )
        .await;
        assert_eq!(reply["type"], "response", "{reply}");
        assert_eq!(reply["result"]["success"], true);
        assert_eq!(
            request(&mut socket, "connection.ping", json!({"nonce":"alive"})).await["result"]["nonce"],
            "alive"
        );
        assert_live_diff(&mut socket, &repository).await;
        drop(held);
        assert_eq!(
            request(
                &mut socket,
                "checkout.status.get.request",
                json!({"cwd":repository})
            )
            .await["result"]["isGit"],
            true
        );
        socket.close(None).await.unwrap();
        fixture.stop().await;
    }
}

async fn assert_live_diff(socket: &mut Socket, repository: &std::path::Path) {
    let reply = request(
        socket,
        "checkout.diff.subscribe.request",
        json!({
            "cwd":repository, "subscriptionId":"refresh-diff", "compare":{"mode":"uncommitted"}
        }),
    )
    .await;
    assert_eq!(reply["type"], "response", "{reply}");
    assert_eq!(reply["result"]["files"][0]["path"], "changed.txt");
    assert_eq!(reply["result"]["files"][0]["additions"], 1);
    std::fs::write(repository.join("changed.txt"), "first\nsecond\n").unwrap();
    let update = receive(socket).await;
    assert_eq!(update["method"], "checkout.diff.update");
    assert_eq!(update["params"]["subscriptionId"], "refresh-diff");
    assert_eq!(update["params"]["files"][0]["additions"], 2);
    assert_eq!(
        request(socket, "connection.ping", json!({"nonce":"updated"})).await["result"]["nonce"],
        "updated"
    );
}
