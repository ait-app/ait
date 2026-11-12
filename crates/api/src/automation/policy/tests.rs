use serde_json::{Value, json};

use super::*;
use crate::test_support::{Workspaces, address};

async fn decide(fixture: &Workspaces, method: &str, params: Value) -> Result<(), ErrorCode> {
    let incoming = Request {
        id: "r1".to_owned(),
        method: method.to_owned(),
        params,
    };
    request(&fixture.api.shared, &incoming).await
}

#[tokio::test]
async fn only_automation_and_turn_methods_pass() {
    let fixture = Workspaces::new(address());
    for method in WEBHOOK_METHODS {
        assert_eq!(
            decide(&fixture, method, json!({})).await,
            Ok(()),
            "{method}"
        );
    }
    assert_eq!(
        decide(&fixture, GITHUB_METHOD, json!({})).await,
        Ok(()),
        "GitHub deliveries use the restricted automation session"
    );
    for method in [
        "agent.create.request",
        "agent.message.send.request",
        "bot.message.send.request",
        "bot.conversation.open.request",
        "bot.history.get.request",
        "bot.channel.update.request",
        "terminal.create.request",
        "automation.settings.set.request",
    ] {
        assert_eq!(
            decide(&fixture, method, json!({})).await,
            Err(ErrorCode::Forbidden),
            "{method}"
        );
    }
    // Without a Bot service no turn is in a Webhook channel.
    assert_eq!(
        decide(
            &fixture,
            "bot.turn.get.request",
            json!({"workspaceId":"wks_0123456789abcdef","turnId":"x"})
        )
        .await,
        Err(ErrorCode::Forbidden)
    );
}
