//! `mcp.acceptPeers` reaches the daemon's peer publication without a restart.

use super::*;

async fn changed(accepting: &mut tokio::sync::watch::Receiver<bool>) -> bool {
    tokio::time::timeout(Duration::from_secs(5), accepting.changed())
        .await
        .expect("a change within the guard")
        .expect("API alive");
    *accepting.borrow_and_update()
}

#[tokio::test]
async fn peer_acceptance_follows_committed_configuration() {
    let scoped = Scoped::start().await;
    let mut accepting = scoped.fixture.api.peer_acceptance();
    assert!(
        !*accepting.borrow_and_update(),
        "a new Host does not accept"
    );

    scoped
        .rpc(
            "daemon.config.set.request",
            json!({"config":{"mcp":{"acceptPeers":true}}}),
        )
        .await;
    assert!(changed(&mut accepting).await);

    scoped
        .rpc(
            "daemon.config.set.request",
            json!({"config":{"mcp":{"enabled":false}}}),
        )
        .await;
    assert!(
        !changed(&mut accepting).await,
        "a disabled MCP endpoint accepts no peers"
    );
    assert!(
        !*scoped.fixture.api.peer_acceptance().borrow(),
        "later observers start from the current value"
    );
    scoped.stop().await;
}
