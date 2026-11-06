use super::*;

#[test]
fn catalog_keeps_exported_commands_and_real_extensions_without_builtin_collisions() {
    let native = vec![
        json!({"name":"usage","source":"builtin"}),
        json!({"name":"model","aliases":["models"],"source":"builtin"}),
        json!({"name":"future-host-control","source":"builtin"}),
        json!({"name":"status","source":"extension"}),
        json!({"name":"my:command","source":"file"}),
    ];
    let projected = visible(&native);
    assert_eq!(
        projected[0], native[0],
        "filtering must not insert null optional fields rejected by the shared wire schema"
    );
    let names: Vec<_> = projected
        .iter()
        .map(|item| item["name"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(names, ["usage", "status", "my:command", "btw"]);
    assert_eq!(blocked("/models other/model", &native), Some("models"));
    assert_eq!(blocked("/model:other/model", &native), Some("model"));
    assert_eq!(
        blocked("/future-host-control", &native),
        Some("future-host-control")
    );
    assert!(blocked("/status", &native).is_none());
    assert!(blocked("/my:command", &native).is_none());
}

#[test]
fn manual_unexported_commands_never_become_inference_but_paths_and_unknown_text_survive() {
    for name in TUI_ONLY.iter().chain(MANAGED) {
        assert_eq!(blocked(&format!("/{name}"), &[]), Some(*name));
        assert!(!rejection(name).is_empty());
    }
    for text in ["/etc/hosts explain this file", "/unknown question", "hello"] {
        assert!(blocked(text, &[]).is_none());
        assert!(!is_btw(text));
    }
    assert_eq!(
        split(" /btw What is this? "),
        Some(("btw", "What is this?"))
    );
    assert!(is_btw("/btw"));
    assert_eq!(visible(&[]).len(), 1);
}

#[test]
fn terminal_only_subcommands_are_hidden_without_removing_native_data_arguments() {
    let native = vec![
        json!({"name":"advisor","source":"builtin","subcommands":[{"name":"status"},{"name":"configure"}]}),
        json!({"name":"todo","source":"builtin","subcommands":[{"name":"edit"},{"name":"export"}]}),
        json!({"name":"mcp","source":"builtin","subcommands":[{"name":"list"},{"name":"reconnect"}]}),
    ];
    assert_eq!(
        visible(&native)[0]["subcommands"],
        json!([{"name":"status"}])
    );
    assert_eq!(
        visible(&native)[1]["subcommands"],
        json!([{"name":"export"}])
    );
    assert_eq!(blocked("/advisor configure", &native), Some("advisor"));
    assert_eq!(blocked("/mcp reconnect", &native), Some("mcp"));
    assert!(blocked("/todo export configure", &native).is_none());
    assert!(read_only("/advisor status", &native));
    assert!(!read_only("/todo import file.md", &native));
}
