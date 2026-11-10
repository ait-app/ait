use super::*;

#[test]
fn builtin_configuration_names_and_aliases_are_never_dispatchable() {
    for name in MANAGED.iter().chain(TUI_ONLY) {
        let native = vec![json!({"name":name,"source":"builtin"})];
        assert_eq!(
            blocked(&format!("/{name} configure anything"), &native, false),
            Some(*name)
        );
        assert!(visible(&native).iter().all(|entry| entry["name"] != *name));
        assert!(!rejection(name).is_empty());
    }
    assert_eq!(
        blocked(
            "/models x",
            &[json!({"name":"model","source":"builtin","aliases":["models"]})],
            false
        ),
        Some("models")
    );
}

#[test]
fn effective_extensions_own_even_special_names_without_builtin_interception() {
    for name in ["record", "model", "models", "btw", "vibe", "goal", "status"] {
        for source in ["extension", "skill", "file", "mcp"] {
            let native =
                vec![json!({"name":name,"source":source,"argumentHint":"[native argument]"})];
            assert!(blocked(&format!("/{name} argument"), &native, false).is_none());
            assert!(!owns_control(&format!("/{name} argument"), &native, name));
            assert!(visible(&native).contains(&native[0]));
            assert!(!read_only(&format!("/{name}"), &native));
        }
    }
    assert!(owns_control("/btw question", &[], "btw"));
    assert!(owns_control("/vibe", &[], "vibe"));
    assert!(!owns_control("/vibes", &[], "vibe"));
}

#[test]
fn mixed_commands_allow_only_query_forms_without_configuration_bypasses() {
    let native = vec![
        json!({"name":"session","source":"builtin","subcommands":[{"name":"info"},{"name":"pin"},{"name":"delete"}]}),
        json!({"name":"memory","source":"builtin"}),
        json!({"name":"usage","source":"builtin"}),
    ];
    assert_eq!(visible(&native)[0]["subcommands"], json!([{"name":"info"}]));
    for text in [
        "/session delete",
        "/session pin account",
        "/memory clear",
        "/memory mm delete x",
        "/usage reset",
    ] {
        assert!(blocked(text, &native, false).is_some(), "{text}");
    }
    for text in [
        "/session info",
        "/memory view",
        "/memory mm show x",
        "/usage",
    ] {
        assert!(blocked(text, &native, false).is_none(), "{text}");
    }
}

#[test]
fn vibe_requires_a_bridge_but_project_commands_and_non_commands_keep_their_meaning() {
    let wired = vec![json!({"name":"vibe","source":"builtin"})];
    assert_eq!(blocked("/vibe task", &wired, false), Some("vibe"));
    assert!(blocked("/vibe task", &wired, true).is_none());
    assert!(rejection("vibe").contains("Ait does not install or modify OMP"));
    for text in ["/etc/hosts explain", "/unknown task", "hello"] {
        assert!(blocked(text, &[], false).is_none());
    }
    assert_eq!(
        split(" /btw What is this? "),
        Some(("btw", "What is this?"))
    );
}
