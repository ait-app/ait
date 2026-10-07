use super::{InboundKind, MethodSpec};

#[test]
fn declarations_preserve_names_and_explicit_message_directions() {
    for (method, name, kind) in [
        (
            MethodSpec::request("connection.ping"),
            "connection.ping",
            InboundKind::Request,
        ),
        (
            MethodSpec::event("terminal.input"),
            "terminal.input",
            InboundKind::Event,
        ),
        (
            MethodSpec::response("browser.automation.execute.response"),
            "browser.automation.execute.response",
            InboundKind::Response,
        ),
    ] {
        assert_eq!(method.name, name);
        assert_eq!(method.kind, kind);
    }
}
