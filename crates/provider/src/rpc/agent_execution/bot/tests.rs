use domain::agent_runtime::StoredAgentConfig;
use serde_json::json;

use super::Profile;
use crate::protocol::agent_config::{ConfigPatch, NullableSetting};
use crate::service::bot_execution::{BotSessionSupport, InstructionDelivery};

const CAPABLE: BotSessionSupport = BotSessionSupport {
    instructions: InstructionDelivery::SystemPrompt,
    read_only_mode: Some("read-only"),
    tool_preapproval: true,
    ait_tools: true,
};

const DEGRADED: BotSessionSupport = BotSessionSupport {
    instructions: InstructionDelivery::TurnPrefix,
    read_only_mode: None,
    tool_preapproval: false,
    ait_tools: false,
};

#[test]
fn capable_profile_patches_only_the_differences_until_converged() {
    let current = StoredAgentConfig {
        mode_id: Some("auto".into()),
        system_prompt: Some("rules".into()),
        ..StoredAgentConfig::default()
    };

    let patch = Profile::new(&CAPABLE, "rules".into())
        .patch(&current)
        .unwrap();

    assert_eq!(patch.mode_id, NullableSetting::Set("read-only".into()));
    assert_eq!(patch.system_prompt, NullableSetting::Unchanged);
    let NullableSetting::Set(policy) = &patch.tool_policy else {
        panic!("tool policy must be set: {patch:?}");
    };
    let tools = policy["preapproved"].as_array().unwrap();
    assert!(tools.contains(&json!({"kind":"mcp","server":"ait","tool":"create_agent"})));
    assert!(!tools.contains(&json!({"kind":"mcp","server":"ait","tool":"list_workspaces"})));
    let converged = patch.apply(&current);
    assert_eq!(
        Profile::new(&CAPABLE, "rules".into()).patch(&converged),
        Some(ConfigPatch::default())
    );
    assert_eq!(
        Profile::new(&CAPABLE, "changed".into())
            .patch(&converged)
            .unwrap()
            .system_prompt,
        NullableSetting::Set("changed".into())
    );
}

#[test]
fn degraded_profile_never_patches_the_prompt_and_cannot_clear_a_mode() {
    let profile = Profile::new(&DEGRADED, "rules".into());
    let prompted = StoredAgentConfig {
        system_prompt: Some("old".into()),
        ..StoredAgentConfig::default()
    };
    let granted = StoredAgentConfig {
        tool_policy: Some(json!({"preapproved":[]})),
        ..StoredAgentConfig::default()
    };
    let moded = StoredAgentConfig {
        mode_id: Some("auto".into()),
        ..StoredAgentConfig::default()
    };

    assert_eq!(profile.patch(&prompted), Some(ConfigPatch::default()));
    assert_eq!(
        profile.patch(&granted).unwrap().tool_policy,
        NullableSetting::Clear
    );
    assert_eq!(profile.patch(&moded), None);
}
