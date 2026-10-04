use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::json;
use tokio::sync::mpsc;

use super::{HostEvent, Observation, PortError, Project};

#[tokio::test]
async fn observations_activate_once_and_end_when_the_host_closes() {
    let (sender, receiver) = mpsc::unbounded_channel();
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let mut observation = Observation::new(
        receiver,
        Box::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }),
    );
    assert!(format!("{observation:?}").contains("active: false"));
    observation.activate().expect("activates");
    observation.activate().expect("second call is a no-op");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    sender
        .send(HostEvent {
            method: "agent_stream".to_owned(),
            params: json!({"agentId": "a"}),
        })
        .expect("send");
    drop(sender);
    let event = observation.next().await.expect("one event");
    assert_eq!(event.method, "agent_stream");
    assert!(observation.next().await.is_none());
}

#[test]
fn failed_activation_is_reported() {
    let (_sender, receiver) = mpsc::unbounded_channel();
    let mut observation = Observation::new(receiver, Box::new(|| Err(PortError::Closed)));
    assert_eq!(observation.activate(), Err(PortError::Closed));
}

#[test]
fn project_debug_output_hides_local_paths_and_remotes() {
    let project = Project {
        id: "prj_1".to_owned(),
        name: "repo".to_owned(),
        root: "/Users/someone/secret/repo".to_owned(),
        remote_url: Some("https://user:token@github.com/x/y".to_owned()),
        branch: Some("main".to_owned()),
    };
    let debug = format!("{project:?}");
    assert!(debug.contains("prj_1"));
    assert!(!debug.contains("/Users/someone"));
    assert!(!debug.contains("token"));
}
