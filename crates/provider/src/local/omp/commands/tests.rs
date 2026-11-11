use serde_json::{Value, json};

use super::{blocked, split};

/// What the menu offers: the ownership catalog minus entries marked not visible.
fn shown(native: &[Value]) -> Vec<Value> {
    super::catalog(native.to_vec())
        .into_iter()
        .filter(|entry| entry["menuVisible"] != false)
        .collect()
}

fn catalog() -> Vec<Value> {
    vec![
        json!({"name":"review","source":"skill","description":"Review"}),
        json!({"name":"model","source":"builtin","description":"Select model"}),
        json!({"name":"quit","source":"builtin","aliases":["q"],"description":"Exit"}),
        json!({"name":"goal","source":"builtin","description":"Native goal"}),
    ]
}

#[test]
fn split_recognizes_commands_but_not_paths_or_prose() {
    assert_eq!(split(" /review args "), Some(("review", "args")));
    assert_eq!(split("/force: now"), Some(("force", "now")));
    assert_eq!(split("/model"), Some(("model", "")));
    assert_eq!(split("/folder/goal pause"), None);
    assert_eq!(split("discuss /goal pause"), None);
    assert_eq!(split("plain text"), None);
    assert_eq!(split("/"), None);
}

#[test]
fn same_name_plugin_owner_survives_builtin_filtering() {
    // The native catalog reports the effective owner: a plugin named like a builtin
    // arrives with its own source and must not be intercepted by the builtin list.
    let native = vec![
        json!({"name":"review","source":"skill"}),
        json!({"name":"model","source":"extension","description":"Project model tool"}),
        json!({"name":"quit","source":"plugin"}),
    ];
    assert_eq!(blocked("/model gpt", &native), None);
    assert_eq!(blocked("/quit", &native), None);
    let menu = shown(&native);
    assert_eq!(menu.len(), 3);
    assert!(
        menu.iter()
            .any(|entry| entry["name"] == "model" && entry["source"] == "extension")
    );
}

#[test]
fn plugin_owners_keep_blocked_and_terminal_names() {
    let native = vec![
        json!({"name":"record","source":"extension"}),
        json!({"name":"goal","source":"project"}),
        json!({"name":"btw","source":"template"}),
        json!({"name":"vibe","source":"skill"}),
    ];
    for name in ["record", "goal", "btw", "vibe"] {
        assert_eq!(blocked(&format!("/{name} argument"), &native), None);
    }
    assert!(
        super::catalog(native)
            .iter()
            .all(|entry| entry["menuVisible"] != false)
    );
}

#[test]
fn explicit_builtin_owners_and_aliases_are_blocked() {
    let native = catalog();
    assert_eq!(blocked("/model gpt", &native), Some("model"));
    assert_eq!(blocked("/quit", &native), Some("quit"));
    assert_eq!(blocked("/q", &native), Some("q"));
    assert_eq!(blocked("/goal set", &native), Some("goal"));
    // Real non-builtin commands pass through untouched.
    assert_eq!(blocked("/review now", &native), None);
    assert_eq!(blocked("/unknown-plugin x", &native), None);
    assert_eq!(blocked("plain text", &native), None);
}

#[test]
fn missing_or_unknown_provenance_downgrades_to_native_dispatch() {
    // A name absent from the catalog is never assumed builtin from its name alone.
    assert_eq!(blocked("/settings", &[]), None);
    assert_eq!(blocked("/hotkeys", &[]), None);
    assert_eq!(blocked("/model gpt", &[]), None);
    // Entries without a source, or with an unrecognized source, keep native ownership.
    let legacy = vec![
        json!({"name":"model","description":"No provenance"}),
        json!({"name":"quit","source":"unknown-kind"}),
    ];
    assert_eq!(blocked("/model gpt", &legacy), None);
    assert_eq!(blocked("/quit", &legacy), None);
    assert!(
        super::catalog(legacy)
            .iter()
            .all(|entry| entry["menuVisible"] != false)
    );
}

#[test]
fn visible_hides_blocked_builtins_but_keeps_their_ownership() {
    let mut native = catalog();
    native.push(
        json!({"name":"deploy","source":"project","argumentHint":"[env]",
        "aliases":["deploy:"],"input":{"hint":"[env]"}}),
    );
    let menu = shown(&native);
    let names: Vec<_> = menu.iter().map(|entry| entry["name"].clone()).collect();
    assert_eq!(names, vec![json!("review"), json!("deploy")]);
    assert_eq!(menu[1]["argumentHint"], "[env]");
    assert_eq!(menu[1]["source"], "project");
    // The refused builtins stay in the catalog so their names remain owned by a native entry;
    // hiding a builtin must never free it for a client shortcut.
    let catalog = super::catalog(native.clone());
    assert_eq!(catalog.len(), native.len());
    for name in ["model", "quit", "goal"] {
        let entry = catalog
            .iter()
            .find(|entry| entry["name"] == name)
            .expect("ownership retained");
        assert_eq!(
            entry["menuVisible"], false,
            "/{name} must not be advertised"
        );
    }
    // The blocked alias is withheld from menu matching, raw aliases stay authoritative.
    let quit = catalog
        .iter()
        .find(|entry| entry["name"] == "quit")
        .unwrap();
    assert_eq!(quit["aliases"], json!(["q"]));
    assert_eq!(quit["menuAliases"], json!([] as [&str; 0]));
}

#[test]
fn alias_owners_resolve_to_their_entry_source() {
    let native = vec![
        json!({"name":"quit","source":"builtin","aliases":["q"]}),
        json!({"name":"exit","source":"plugin","aliases":["q"]}),
    ];
    // A plugin claimant of the same alias keeps native dispatch regardless of catalog order.
    assert_eq!(blocked("/q", &native[..1]), Some("q"));
    assert_eq!(blocked("/q", &native[1..]), None);
    assert_eq!(blocked("/q", &native), None);
    // /quit stays refused by name; the plugin owner survives in the menu.
    let menu = shown(&native);
    assert_eq!(menu.len(), 1);
    assert_eq!(menu[0]["name"], "exit");
}

#[test]
fn duplicate_claims_resolve_by_source_not_catalog_order() {
    // A builtin and a provenance-less entry may both claim one name; a single unknown
    // claimant downgrades the name to native dispatch whichever entry arrives first.
    let builtin_first = vec![
        json!({"name":"model","source":"builtin"}),
        json!({"name":"model","description":"No provenance"}),
    ];
    let unknown_first = vec![
        json!({"name":"model","description":"No provenance"}),
        json!({"name":"model","source":"builtin"}),
    ];
    for native in [&builtin_first, &unknown_first] {
        assert_eq!(blocked("/model gpt", native), None);
        let menu = shown(native);
        // The surviving menu entry is the real owner, never the refused builtin.
        assert_eq!(menu.len(), 1);
        assert!(menu[0]["source"].is_null());
    }
    // Duplicate builtin claims stay refused; ownership is retained but nothing is advertised.
    let builtins = vec![
        json!({"name":"quit","source":"builtin"}),
        json!({"name":"quit","source":"builtin"}),
    ];
    assert_eq!(blocked("/quit", &builtins), Some("quit"));
    assert_eq!(shown(&builtins), [] as [Value; 0]);
    assert_eq!(super::catalog(builtins).len(), 2);
}

#[test]
fn blocked_alias_never_advertises_its_builtin_owner() {
    let native = vec![
        json!({"name":"quit","source":"builtin","aliases":["q"]}),
        json!({"name":"model","source":"builtin"}),
    ];
    assert_eq!(blocked("/q", &native), Some("q"));
    // Ownership keeps both refused builtins; the menu advertises neither, and the refused
    // alias is not offered for matching while the raw alias still answers precedence.
    assert_eq!(shown(&native), [] as [Value; 0]);
    let catalog = super::catalog(native);
    assert_eq!(catalog.len(), 2);
    assert_eq!(catalog[0]["aliases"], json!(["q"]));
    assert_eq!(catalog[0]["menuAliases"], json!([] as [&str; 0]));
}

#[test]
fn working_native_handlers_stay_dispatchable() {
    let native: Vec<_> = ["compact", "usage", "session", "memory", "tools", "context"]
        .iter()
        .map(|name| json!({"name":name,"source":"builtin"}))
        .collect();
    for entry in &native {
        let name = entry["name"].as_str().unwrap();
        assert_eq!(
            blocked(&format!("/{name}"), &native),
            None,
            "/{name} must keep native execution"
        );
    }
    assert!(
        super::catalog(native)
            .iter()
            .all(|entry| entry["menuVisible"] != false)
    );
    // Identity/history-mutating builtins stay unavailable until task25 reconciliation.
    for name in [
        "handoff", "new", "clear", "resume", "branch", "rewind", "fork", "tree",
    ] {
        let catalog = vec![json!({"name":name,"source":"builtin"})];
        assert_eq!(blocked(&format!("/{name}"), &catalog), Some(name));
    }
}
