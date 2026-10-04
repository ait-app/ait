use metadata::ports::registry::{ProjectRegistry, WorkspaceRegistry};
use metadata::service::directory::{Directory, DirectoryDependencies};
use metadata::storage::project_config::LocalProjectConfigStore;
use metadata::storage::project_icon::LocalProjectIconStore;
use metadata::storage::registry::{FileBackedProjectRegistry, FileBackedWorkspaceRegistry};
use model::events::EventHub;
use provider::protocol::timeline::NativeItem;
use provider::storage::timeline::Timeline;
use serde_json::json;

use super::{LocalDirectorySource, PortError, backlog, list_projects, observe, open_workspace};

fn directory(data: &std::path::Path) -> Directory {
    let projects = FileBackedProjectRegistry::new(data.join("projects/projects.json"));
    let workspaces = FileBackedWorkspaceRegistry::new(data.join("projects/workspaces.json"));
    projects.initialize().expect("initialize projects");
    workspaces.initialize().expect("initialize workspaces");
    Directory::new(DirectoryDependencies {
        projects: Box::new(projects),
        workspaces: Box::new(workspaces),
        source: Box::new(LocalDirectorySource),
        config_store: Box::new(LocalProjectConfigStore),
        icon_store: Box::new(LocalProjectIconStore::new(data.join("projects/icons"))),
        server_id: "server-test".to_owned(),
    })
}

#[tokio::test]
async fn observations_forward_buffered_and_live_events_for_one_agent() {
    let hub = EventHub::default();
    let mut observation = observe(&hub, "agent-1");
    hub.publish(
        "agent-1",
        "agent_stream",
        &json!({"agentId": "agent-1", "n": 1}),
    );
    hub.publish("agent-2", "agent_stream", &json!({"agentId": "agent-2"}));
    observation.activate().expect("activates");
    hub.publish(
        "agent-1",
        "agent.timeline.replacement",
        &json!({"agentId": "agent-1", "epoch": "e2"}),
    );
    let first = observation.next().await.expect("buffered event");
    assert_eq!(first.method, "agent_stream");
    assert_eq!(first.params["n"], 1);
    let second = observation.next().await.expect("live event");
    assert_eq!(second.method, "agent.timeline.replacement");
    assert_eq!(second.params["epoch"], "e2");
}

#[tokio::test]
async fn overflowing_while_paused_closes_the_observation() {
    let hub = EventHub::default();
    let mut observation = observe(&hub, "agent-1");
    for index in 0..65 {
        hub.publish("agent-1", "agent_stream", &json!({"n": index}));
    }
    assert_eq!(observation.activate(), Err(PortError::Closed));
    assert!(observation.next().await.is_none());
}

#[test]
fn backlogs_are_unmerged_rows_of_the_current_generation() {
    let timeline = Timeline::memory().expect("memory timeline");
    let item = |key: &str, text: &str| NativeItem {
        key: key.to_owned(),
        turn_id: None,
        timestamp: "2026-10-04T00:00:00.000Z".to_owned(),
        item: json!({"type": "assistant_message", "messageId": key, "text": text}),
    };
    let (epoch, _) = timeline
        .append(
            "agent-1",
            "claude",
            &[
                item("native:claude:m:0", "Hello"),
                item("native:claude:m:1", "World"),
            ],
        )
        .expect("append");
    let read = backlog(&timeline, "agent-1").expect("read");
    assert_eq!(read.epoch, epoch);
    assert_eq!(read.rows.len(), 2);
    assert_eq!(read.rows[0].item["text"], "Hello");
    assert!(read.rows[0].seq < read.rows[1].seq);
    assert_eq!(read.rows[1].provider, "claude");
}

#[test]
fn projects_list_without_paths_leaking_and_open_their_workspace() {
    let root = tempfile::tempdir().expect("temp dir");
    let data = root.path().join("data");
    let repository = root.path().join("repo");
    std::fs::create_dir_all(&repository).expect("create repository");
    let directory = directory(&data);
    let added = directory
        .add_project(
            repository.to_str().expect("utf-8 path"),
            "2026-10-04T00:00:00.000Z",
        )
        .expect("add project");
    let projects = list_projects(&directory).expect("list");
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0].id, added.project_id);
    assert_eq!(projects[0].remote_url, None);
    let workspace = open_workspace(&directory, &added.project_id)
        .expect("open")
        .expect("known project");
    let again = open_workspace(&directory, &added.project_id)
        .expect("open again")
        .expect("known project");
    assert_eq!(workspace, again, "the same workspace is reused");
    assert_eq!(
        open_workspace(&directory, "prj_unknown").expect("lookup"),
        None
    );
    std::fs::remove_dir_all(&repository).expect("remove repository");
    assert_eq!(
        open_workspace(&directory, &added.project_id).expect("lookup"),
        None
    );
}
