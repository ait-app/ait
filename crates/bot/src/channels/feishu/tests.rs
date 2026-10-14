use super::*;
use domain::{BotId, ChannelId};
mod lifecycle;
mod transport;

#[test]
fn feishu_scopes_user_group_and_thread_before_admission() {
    let channel = Channel {
        id: ChannelId::generate(),
        bot_id: BotId::generate(),
        enabled: true,
        cursor: None,
        deleted: false,
        config: ChannelConfig::Feishu {
            app_id: "app-1".into(),
            secret_ref: "env:AIT_SERVER_CREDENTIAL_FEISHU".into(),
            allowed_senders: vec!["ou_alice".into()],
            allow_groups: true,
        },
    };
    let mut event = json!({"header":{"event_type":"im.message.receive_v1","event_id":"event-1"},"event":{"sender":{"sender_type":"user","sender_id":{"open_id":"ou_alice"}},"message":{"chat_type":"group","chat_id":"oc_group","message_type":"text","content":"{\"text\":\"hello\"}","thread_id":"thread-1","mentions":[{"id":{"open_id":"ou_bot"}}]}}});
    event["event"]["message"]["message_id"] = json!("om_original");
    let routed = incoming(&channel, &event, "ou_bot").unwrap().unwrap();
    assert_eq!(routed.sender, "ou_alice");
    assert_eq!(routed.peer, "oc_group");
    assert_eq!(routed.thread.as_deref(), Some("thread-1"));
    assert_eq!(routed.reply_to.as_deref(), Some("om_original"));
    event["event"]["message"]["mentions"] = json!([]);
    assert!(incoming(&channel, &event, "ou_bot").unwrap().is_none());
    event["event"]["message"]["chat_type"] = json!("p2p");
    assert!(incoming(&channel, &event, "ou_bot").unwrap().is_some());
    event["event"]["sender"]["sender_type"] = json!("app");
    assert!(incoming(&channel, &event, "ou_bot").unwrap().is_none());
}
