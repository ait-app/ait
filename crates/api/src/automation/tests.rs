use std::sync::Arc;

use super::*;

#[test]
fn only_an_active_bot_is_reachable() {
    let bot = |state| WebhookBot {
        workspace_id: "wks_0123456789abcdef".to_owned(),
        channel_id: domain::ChannelId::generate(),
        title: "app".to_owned(),
        state,
        kind: "webhook",
    };
    assert!(!reachable(&[]));
    assert!(!reachable(&[bot("paused")]));
    assert!(reachable(&[bot("paused"), bot("active")]));
}

#[tokio::test]
async fn sessions_need_the_bot_service() {
    let automation = Arc::new(Automation::default());
    assert_eq!(automation.admit(None).err(), Some(ErrorCode::Forbidden));
}
