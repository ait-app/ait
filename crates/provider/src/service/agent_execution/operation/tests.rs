use super::*;

#[test]
fn every_worker_method_has_a_canonical_operation_and_explicit_ownership() {
    let connection_owned = [
        "agent.context.compact.request",
        "agent.timeline.append.request",
        "agent.timeline.set_subscription.request",
    ];
    for spec in crate::capabilities::implemented_methods() {
        if crate::rpc::agents::METHODS
            .iter()
            .any(|preset| preset.name == spec.name)
            || spec.name.starts_with("agent.skills.")
            || connection_owned.contains(&spec.name)
        {
            continue;
        }
        let operation =
            Operation::parse(spec.name).unwrap_or_else(|_| panic!("unclassified {}", spec.name));
        assert_eq!(operation.as_str(), spec.name);
        if operation.access() == Access::Session || operation.access() == Access::Timeline {
            assert!(operation.target().is_some(), "{} has no target", spec.name);
        }
    }
    for method in connection_owned
        .into_iter()
        .chain(["unknown", "agent.provider_subagents.unknown"])
    {
        assert_eq!(Operation::parse(method), Err(ErrorCode::MethodNotFound));
    }
}

#[test]
fn child_and_plugin_operations_use_their_own_identity_fields() {
    for operation in [Operation::Subagents, Operation::ChildTimeline] {
        assert_eq!(operation.access(), Access::Session);
        assert_eq!(operation.target().unwrap().pointer(), "/parentAgentId");
    }
    assert_eq!(
        Operation::AppendTimeline.target().unwrap().pointer(),
        "/request/agentId"
    );
    assert_eq!(Operation::Rewind.target().unwrap().pointer(), "/agentId");
    assert_eq!(Operation::Snapshot.access(), Access::Catalog);
    assert_eq!(Operation::Archive.access(), Access::Mutation);
}
