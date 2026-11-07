use super::*;

#[test]
fn native_catalog_keeps_namespaces_hints_and_authoritative_empty_updates() {
    let mut catalog = Catalog::new();
    assert!(catalog.items().is_none());
    let frame = json!({"method":"session/update","params":{"sessionId":"native","update":{
        "sessionUpdate":"available_commands_update","availableCommands":[
            {"name":"mcp:server:prompt","description":"Native MCP prompt","input":{"hint":"<topic>"}},
            {"name":"compact","description":"Native compaction"}]}}});
    assert!(publish(&catalog.sender(), "native", &frame).unwrap());
    catalog.observe();
    assert_eq!(catalog.items().unwrap()[0]["name"], "mcp:server:prompt");
    assert_eq!(catalog.items().unwrap()[0]["argumentHint"], "<topic>");
    assert!(publish(&catalog.sender(), "other-session", &frame).is_err());
    assert!(publish(&catalog.sender(), "native", &json!({"method":"session/update","params":{
        "sessionId":"native","update":{"sessionUpdate":"available_commands_update","availableCommands":[]}}})).unwrap());
    catalog.observe();
    assert_eq!(catalog.items(), Some([].as_slice()));
}

#[test]
fn unrelated_updates_are_preserved_and_malformed_native_catalogs_are_errors() {
    let catalog = Catalog::new();
    assert!(
        !publish(
            &catalog.sender(),
            "native",
            &json!({"method":"session/update",
        "params":{"update":{"sessionUpdate":"usage_update"}}})
        )
        .unwrap()
    );
    assert!(publish(&catalog.sender(), "native", &json!({"method":"session/update","params":{
        "sessionId":"native","update":{"sessionUpdate":"available_commands_update","availableCommands":{}}}})).is_err());
}
