//! Git observation lifetimes follow admitted directory subscriptions and their active filters.

use std::sync::Arc;

use model::workspace::git::{WorkspaceGitObservation, WorkspaceGitObserver};
use serde_json::json;

use super::*;
use crate::rpc::directory::{execute, listing};

#[derive(Debug, Default)]
struct Log {
    updates: Vec<Vec<String>>,
    released: usize,
}

#[derive(Debug, Default)]
struct Observer(Arc<Mutex<Log>>);

#[derive(Debug)]
struct Interest(Arc<Mutex<Log>>);

impl WorkspaceGitObserver for Observer {
    fn observe(&self) -> Box<dyn WorkspaceGitObservation> {
        Box::new(Interest(self.0.clone()))
    }
}

impl WorkspaceGitObservation for Interest {
    fn set_paths(&mut self, paths: &[String]) {
        self.0.lock().unwrap().updates.push(paths.to_vec());
    }
}

impl Drop for Interest {
    fn drop(&mut self) {
        self.0.lock().unwrap().released += 1;
    }
}

#[test]
fn git_observation_requires_activation_and_tracks_filter_archive_and_release() {
    let observer = Arc::new(Observer::default());
    let mut directory = directory().with_git_observer(observer.clone());
    let mut other = workspace();
    other.workspace_id = "wks_other".to_owned();
    other.cwd = "/tmp/other".to_owned();
    other.title = Some("Review".to_owned());
    directory
        .workspaces
        .upsert(&other, WorkspaceMutationContext::default())
        .unwrap();
    execute(&mut directory, "workspace.list.request", json!({})).unwrap();
    let request = serde_json::from_value(json!({"filter":{"query":"main"}})).unwrap();
    let (_, mut interest) = listing::prepare(&directory, request, "sub".to_owned()).unwrap();
    assert!(observer.0.lock().unwrap().updates.is_empty());
    interest.activate();
    assert_eq!(
        observer.0.lock().unwrap().updates.last().unwrap(),
        &["/tmp/alpha"]
    );
    directory
        .set_workspace_title("wks_a", Some("Review"), "now")
        .unwrap();
    interest.update(&directory).unwrap();
    assert!(
        observer
            .0
            .lock()
            .unwrap()
            .updates
            .last()
            .unwrap()
            .is_empty()
    );
    directory
        .set_workspace_title("wks_a", None, "later")
        .unwrap();
    interest.update(&directory).unwrap();
    assert_eq!(
        observer.0.lock().unwrap().updates.last().unwrap(),
        &["/tmp/alpha"]
    );
    directory.archive_workspace("wks_a", "archived").unwrap();
    interest.update(&directory).unwrap();
    assert!(
        observer
            .0
            .lock()
            .unwrap()
            .updates
            .last()
            .unwrap()
            .is_empty()
    );
    directory.open_workspace("/tmp/alpha", "restored").unwrap();
    interest.update(&directory).unwrap();
    assert_eq!(
        observer.0.lock().unwrap().updates.last().unwrap(),
        &["/tmp/alpha"]
    );
    drop(interest);
    assert_eq!(observer.0.lock().unwrap().released, 1);
}

#[test]
fn git_observation_tracks_all_pages_and_excludes_archived_or_removed_projects() {
    let observer = Arc::new(Observer::default());
    let directory = directory().with_git_observer(observer.clone());
    let mut linked = workspace();
    linked.workspace_id = "wks_linked".to_owned();
    linked.cwd = "/tmp/linked".to_owned();
    directory
        .workspaces
        .upsert(&linked, WorkspaceMutationContext::default())
        .unwrap();
    let request = serde_json::from_value(json!({"page":{"limit":1}})).unwrap();
    let (snapshot, mut interest) = listing::prepare(&directory, request, "sub".to_owned()).unwrap();
    assert_eq!(snapshot["entries"].as_array().unwrap().len(), 1);
    interest.activate();
    assert_eq!(
        observer.0.lock().unwrap().updates.last().unwrap(),
        &["/tmp/alpha", "/tmp/linked"]
    );
    directory.projects.archive("prj_a", "archived").unwrap();
    interest.update(&directory).unwrap();
    assert!(
        observer
            .0
            .lock()
            .unwrap()
            .updates
            .last()
            .unwrap()
            .is_empty()
    );
    directory.projects.upsert(&project()).unwrap();
    interest.update(&directory).unwrap();
    assert_eq!(observer.0.lock().unwrap().updates.last().unwrap().len(), 2);
    directory.remove_project("prj_a", "removed").unwrap();
    interest.update(&directory).unwrap();
    assert!(
        observer
            .0
            .lock()
            .unwrap()
            .updates
            .last()
            .unwrap()
            .is_empty()
    );
    drop(interest);
    assert_eq!(observer.0.lock().unwrap().released, 1);
}
