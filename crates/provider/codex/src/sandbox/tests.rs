use super::*;

#[test]
fn switching_modes_keeps_inherited_workspace_settings_and_explicit_overrides() {
    let mut sandbox = Sandbox {
        workspace_write: json!({"writable_roots":["/shared"],"network_access":true,
            "exclude_slash_tmp":true,"exclude_tmpdir_env_var":true}),
        effective: None,
        confined: false,
    };
    sandbox.observe(&json!({"sandbox":{"type":"dangerFullAccess"}}));
    let config = serde_json::from_value(json!({"modeId":"auto","providerOptions":{
        "sandbox_workspace_write":{"network_access":false}}}))
    .unwrap();
    assert_eq!(
        sandbox.policy(&config),
        json!({"type":"workspaceWrite",
        "writableRoots":["/shared"],"networkAccess":false,
        "excludeSlashTmp":true,"excludeTmpdirEnvVar":true})
    );
}

#[test]
fn confined_sessions_use_full_access_inside_the_os_write_sandbox() {
    let mut sandbox = Sandbox {
        workspace_write: json!({"writable_roots":["/shared"]}),
        effective: None,
        confined: true,
    };
    let config = serde_json::from_value(json!({"modeId":"auto"})).unwrap();

    assert_eq!(sandbox.policy(&config), json!({"type":"dangerFullAccess"}));
    sandbox.observe(&json!({"sandbox":{"type":"dangerFullAccess"}}));
    assert_eq!(sandbox.policy(&config), json!({"type":"dangerFullAccess"}));
    let (approval, mode, _) = controls::policy(&config, true);
    assert_eq!(approval, "on-request", "approval prompts are unchanged");
    assert_eq!(mode, "danger-full-access");
}
