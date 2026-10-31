//! Exercise the public preset service with an independently implemented storage port.

#![expect(
    unused_crate_dependencies,
    reason = "External consumer of the public catalog contract only"
)]

use domain::agent::{AgentConfig, AgentTarget, Driver, Revision};
use provider::ports::agent::{AgentError, DefaultSelection};
use provider::service::agents::Agents;

mod common;

fn config(name: &str) -> AgentConfig {
    AgentConfig::new(
        name.to_owned(),
        Driver::Codex,
        "model".to_owned(),
        None,
        true,
    )
    .unwrap()
}

#[test]
fn an_external_adapter_can_handle_commands_and_construct_successful_receipts() {
    let mut service = Agents::new(Box::new(common::catalog::MemoryCatalog::new()));
    let initial = service.get_default().unwrap();
    assert_eq!(initial.agent_id(), None);
    assert_eq!(initial.version(), 0);
    let first = service
        .configure(AgentTarget::Create, config("first"), "create")
        .unwrap();
    assert_eq!(first.revision(), Revision::new(1).unwrap());
    let original = service.get(first.agent_id(), None).unwrap();
    assert_eq!(original.config(), &config("first"));
    assert!(original.recorded_at() > 0);
    assert_eq!(
        service.list(None, 10).unwrap(),
        std::slice::from_ref(&original)
    );
    let target = AgentTarget::Update {
        id: first.agent_id(),
        expected: first.revision(),
    };
    let updated = service
        .configure(target, config("second"), "update")
        .unwrap();
    assert_ne!(updated.operation_id(), first.operation_id());
    assert_eq!(updated.revision(), Revision::new(2).unwrap());
    assert_eq!(
        service
            .get(first.agent_id(), Some(first.revision()))
            .unwrap(),
        original
    );
    assert_eq!(
        service
            .configure(AgentTarget::Create, config("first"), "create")
            .unwrap(),
        first
    );
    assert_eq!(
        service.configure(AgentTarget::Create, config("changed"), "create"),
        Err(AgentError::IdempotencyConflict)
    );
    assert_eq!(
        service.configure(target, config("stale"), "stale"),
        Err(AgentError::RevisionConflict)
    );
    let selected = service
        .set_default(Some(first.agent_id()), initial.version(), "select")
        .unwrap();
    assert_eq!(selected.selection().agent_id(), Some(first.agent_id()));
    assert_eq!(selected.selection().version(), 1);
    let cleared = service
        .set_default(None, selected.selection().version(), "clear")
        .unwrap();
    assert_ne!(cleared.operation_id(), selected.operation_id());
    assert_eq!(cleared.selection().agent_id(), None);
    assert_eq!(
        service
            .set_default(Some(first.agent_id()), 0, "select")
            .unwrap(),
        selected
    );
    assert_eq!(service.get_default().unwrap(), cleared.selection());
    assert_eq!(
        service.set_default(None, 0, "stale"),
        Err(AgentError::DefaultConflict)
    );
    assert_eq!(service.list(None, 0), Err(AgentError::Invalid));
}

#[test]
fn default_selection_rejects_unrepresentable_versions() {
    assert_eq!(
        DefaultSelection::new(None, i64::MAX as u64)
            .unwrap()
            .version(),
        i64::MAX as u64
    );
    assert_eq!(
        DefaultSelection::new(None, i64::MAX as u64 + 1),
        Err(AgentError::Invalid)
    );
}
