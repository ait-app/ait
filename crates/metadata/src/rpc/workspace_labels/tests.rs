use std::sync::{Arc, Mutex};

use model::workspace::label_store::{
    WorkspaceLabelStore, WorkspaceLabelStoreError, WorkspaceLabelStoreMutation,
    WorkspaceLabelStoreSnapshot,
};
use model::workspace::labels::{WorkspaceLabelColor, WorkspaceLabelDefinition};
use serde_json::{Value, json};

use super::{Delivery, live_update, map_error};
use crate::rpc::ErrorCode;
use crate::service::workspace_labels::WorkspaceLabels;
use crate::service::workspace_labels::{
    SequencedWorkspaceLabelChange, WorkspaceLabelChange, WorkspaceLabelError,
};

#[test]
fn all_palette_colors_roundtrip_and_incremental_sync_reports_deletion() {
    let labels = labels();
    let sink: super::EventSink = Arc::new(|_| Ok(()));
    for color in [
        "violet", "sky", "emerald", "orange", "pink", "indigo", "teal", "red", "amber", "blue",
    ] {
        let result = super::execute(
            &labels,
            "workspace.label.update.request",
            json!({"name":"Urgent","color":color}),
            sink.clone(),
        )
        .unwrap();
        assert_eq!(result.value["label"]["color"], color);
        assert_eq!(result.value["label"]["name"], "Urgent");
    }
    let snapshot = super::execute(
        &labels,
        "workspace.label.list.request",
        json!({}),
        sink.clone(),
    )
    .unwrap()
    .value;
    super::execute(
        &labels,
        "workspace.label.delete.request",
        json!({"name":"Urgent"}),
        sink.clone(),
    )
    .unwrap();
    let delta = super::execute(&labels, "workspace.label.list.request", json!({"sync":{"generation":snapshot["sync"]["generation"],"afterSeq":snapshot["sync"]["headSeq"]}}), sink.clone()).unwrap().value;
    assert_eq!(delta["sync"]["mode"], "changes");
    assert_eq!(delta["labels"], json!([]));
    assert_eq!(delta["sync"]["removals"][0]["name"], "Urgent");
    assert!(matches!(
        super::execute(&labels, "unknown", json!({}), sink.clone()),
        Err(ErrorCode::MethodNotFound)
    ));
    assert!(matches!(
        super::execute(
            &labels,
            "workspace.label.assignment.set.request",
            json!({"workspaceId":"missing","label":{"name":"Urgent","color":"red"},"assigned":true}),
            sink
        ),
        Err(ErrorCode::WorkspaceNotFound)
    ));
}

#[test]
fn deleting_a_label_before_subscription_activation_replaces_its_buffered_upsert() {
    let labels = labels();
    let events = Arc::new(Mutex::new(Vec::new()));
    let received = events.clone();
    let sink: super::EventSink = Arc::new(move |event| {
        received.lock().unwrap().push(event);
        Ok(())
    });
    let subscription = super::execute(
        &labels,
        "workspace.label.list.request",
        json!({"subscribe":{}}),
        sink,
    )
    .unwrap()
    .subscription
    .unwrap();
    labels
        .update("Urgent", None, Some(WorkspaceLabelColor::Blue), "updated")
        .unwrap();
    labels.delete("Urgent", "deleted").unwrap();
    let (_, _listener) = subscription.activate().unwrap();
    let events = events.lock().unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["kind"], "remove");
    assert!(events[0].get("label").is_none());
    assert_eq!(events[0]["name"], "Urgent");
}

fn change(name: &str, seq: u64) -> SequencedWorkspaceLabelChange {
    SequencedWorkspaceLabelChange {
        generation: "generation-one".to_owned(),
        seq,
        change: WorkspaceLabelChange::Upsert {
            label: WorkspaceLabelDefinition {
                name: name.to_owned(),
                color: WorkspaceLabelColor::Red,
            },
            previous_name: None,
        },
    }
}

#[test]
fn label_business_errors_keep_paseo_machine_codes() {
    assert_eq!(
        map_error(WorkspaceLabelError::NameEmpty),
        ErrorCode::LabelNameEmpty
    );
    assert_eq!(
        map_error(WorkspaceLabelError::LabelNotFound),
        ErrorCode::LabelNotFound
    );
    assert_eq!(
        map_error(WorkspaceLabelError::NameTaken),
        ErrorCode::LabelNameTaken
    );
    assert_eq!(
        map_error(WorkspaceLabelError::StorageUncertain),
        ErrorCode::WorkspaceLabelStorageUncertain
    );
}

#[test]
fn live_update_uses_the_connection_subscription_and_sequence() {
    let update = live_update(
        "subscription-one",
        &SequencedWorkspaceLabelChange {
            generation: "generation-one".to_owned(),
            seq: 3,
            change: WorkspaceLabelChange::Upsert {
                label: WorkspaceLabelDefinition {
                    name: "Urgent".to_owned(),
                    color: WorkspaceLabelColor::Red,
                },
                previous_name: Some("Blocked".to_owned()),
            },
        },
    );
    let json = serde_json::to_value(update).unwrap();
    assert_eq!(json["subscriptionId"], "subscription-one");
    assert_eq!(json["generation"], "generation-one");
    assert_eq!(json["previousName"], "Blocked");
    assert_eq!(json["seq"], 3);

    let outbound: super::EventSink = Arc::new(|_| Ok(()));
    let mut delivery = Delivery {
        ready: false,
        pending: Vec::new(),
        outbound,
        subscription_id: "subscription-one".to_owned(),
    };
    delivery.accept(change("A", 1));
    delivery.accept(change("B", 2));
    delivery.accept(change("A", 3));
    let buffered = delivery
        .pending
        .iter()
        .map(|(key, change)| (key.as_str(), change.seq))
        .collect::<Vec<_>>();
    assert_eq!(buffered, vec![("a", 3), ("b", 2)]);
}

#[derive(Debug)]
struct MemoryStore(Mutex<WorkspaceLabelStoreSnapshot>);

impl WorkspaceLabelStore for MemoryStore {
    fn initialize(&self) -> Result<(), WorkspaceLabelStoreError> {
        Ok(())
    }

    fn snapshot(&self) -> Result<WorkspaceLabelStoreSnapshot, WorkspaceLabelStoreError> {
        Ok(self.0.lock().unwrap().clone())
    }

    fn commit(
        &self,
        mutation: &WorkspaceLabelStoreMutation,
    ) -> Result<(), WorkspaceLabelStoreError> {
        let mut state = self.0.lock().unwrap();
        state.labels.clone_from(&mutation.labels);
        Ok(())
    }
}

fn labels() -> WorkspaceLabels {
    WorkspaceLabels::new(Box::new(MemoryStore(Mutex::new(
        WorkspaceLabelStoreSnapshot {
            labels: vec![WorkspaceLabelDefinition {
                name: "Urgent".to_owned(),
                color: WorkspaceLabelColor::Red,
            }],
            workspaces: Vec::new(),
        },
    ))))
    .unwrap()
}

#[test]
fn subscription_activates_after_snapshot_and_drops_its_listener() {
    let labels = labels();
    let events = Arc::new(Mutex::new(Vec::<Value>::new()));
    let received = events.clone();
    let sink: super::EventSink = Arc::new(move |event| {
        received.lock().unwrap().push(event);
        Ok(())
    });
    let dispatch = super::execute(
        &labels,
        "workspace.label.list.request",
        json!({"subscribe":{}}),
        sink,
    )
    .unwrap();
    assert_eq!(dispatch.value["labels"][0]["name"], "Urgent");
    labels
        .update("Urgent", Some("Blocked"), None, "now")
        .unwrap();
    assert!(events.lock().unwrap().is_empty());
    let (id, subscription) = dispatch.subscription.unwrap().activate().unwrap();
    assert_eq!(events.lock().unwrap()[0]["subscriptionId"], id);
    assert_eq!(events.lock().unwrap()[0]["label"]["name"], "Blocked");
    labels
        .update("Blocked", Some("Later"), None, "later")
        .unwrap();
    assert_eq!(events.lock().unwrap().len(), 2);
    drop(subscription);
    labels
        .update("Later", Some("Silent"), None, "after-drop")
        .unwrap();
    assert_eq!(events.lock().unwrap().len(), 2);
}

#[test]
fn activation_reports_sink_failure_and_releases_the_listener() {
    let labels = labels();
    let attempts = Arc::new(Mutex::new(0));
    let delivered = attempts.clone();
    let sink: super::EventSink = Arc::new(move |_| {
        *delivered.lock().unwrap() += 1;
        Err(super::DeliveryError::Closed)
    });
    let dispatch = super::execute(
        &labels,
        "workspace.label.list.request",
        json!({"subscribe":{}}),
        sink,
    )
    .unwrap();
    labels
        .update("Urgent", Some("Blocked"), None, "now")
        .unwrap();
    assert!(matches!(
        dispatch.subscription.unwrap().activate(),
        Err(super::DeliveryError::Closed)
    ));
    labels
        .update("Blocked", Some("Silent"), None, "after-failure")
        .unwrap();
    assert_eq!(*attempts.lock().unwrap(), 1);
}
