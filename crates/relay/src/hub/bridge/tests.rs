use serde_json::json;
use uuid::Uuid;

use super::*;

/// An API that serves connections only, as every implementer did before peer calls.
#[derive(Debug)]
struct ConnectionsOnly;

#[async_trait::async_trait]
impl LocalApi for ConnectionsOnly {
    async fn connect(&self, _admission: Admission<'_>) -> Result<Connection, Refused> {
        Err(Refused)
    }
}

#[tokio::test]
async fn apis_refuse_peer_calls_unless_they_serve_them() {
    let from = Identity {
        host_id: Uuid::new_v4(),
        server_id: Uuid::new_v4(),
        name: "Studio".to_owned(),
    };
    let request = Request {
        tool: "agent_list".to_owned(),
        arguments: json!({}),
        caller: None,
    };
    assert_eq!(
        ConnectionsOnly.peer(&from, request).await,
        Err(PeerRefusal::Disabled)
    );
}
