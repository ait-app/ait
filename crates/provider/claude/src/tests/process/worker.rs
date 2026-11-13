//! A Bot ticket worker on Claude runs its report tool without asking (ADR-274).

use std::collections::BTreeMap;

use super::*;
use provider::testing::agent_mcp::{Adapter, open_worker};

#[tokio::test]
async fn a_ticket_worker_runs_its_report_tool_without_asking_and_keeps_the_users_tools() {
    let (root, client, mut spec) = fixture();
    spec.config.provider_options = Some(BTreeMap::from([(
        "allowedTools".to_owned(),
        json!(["Read"]),
    )]));
    let build = || Box::new(client.clone()) as Box<dyn AgentClient>;
    let adapter = Adapter {
        client: &build,
        spec: spec.clone(),
        openings: &Vec::new,
        prompts: true,
    };

    let stored = open_worker(&adapter, &"w".repeat(64)).await;

    let args: Vec<String> =
        serde_json::from_slice(&std::fs::read(root.path().join("claude-args.json")).unwrap())
            .unwrap();
    assert!(
        args.contains(
            &"--allowedTools=Read,mcp__ait__report_progress,mcp__ait__ticket_read".to_owned()
        ),
        "{args:?}"
    );
    assert_eq!(stored.tool_policy, None);
    assert_eq!(stored.provider_options, spec.config.provider_options);
}
