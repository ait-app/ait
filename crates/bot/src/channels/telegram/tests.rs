use super::*;
use domain::{BotId, ChannelId};
mod lifecycle;
mod polling;

fn channel() -> Channel {
    Channel {
        id: ChannelId::generate(),
        bot_id: BotId::generate(),
        enabled: true,
        cursor: None,
        sync_token: None,
        deleted: false,
        config: ChannelConfig::Telegram {
            token_ref: "env:AIT_SERVER_CREDENTIAL_TELEGRAM".into(),
            allowed_senders: vec!["1".into()],
            allow_groups: true,
        },
    }
}
fn update() -> Value {
    json!({"update_id":10,"message":{"from":{"id":1,"is_bot":false},"chat":{"id":-100,"type":"group"},"text":"@ait_bot hello","message_thread_id":3}})
}

#[test]
fn telegram_routes_allowlisted_senders_and_mentioned_threads() {
    let channel = channel();
    let value = update();
    let message = incoming(&channel, &value, 2, "ait_bot").unwrap().unwrap();
    assert_eq!(message.peer, "-100");
    assert_eq!(message.sender, "1");
    assert_eq!(message.thread.as_deref(), Some("3"));
    assert_eq!(message.message_id, "10");
    let mut ignored = value.clone();
    ignored["message"]["from"]["id"] = json!(9);
    assert!(
        incoming(&channel, &ignored, 2, "ait_bot")
            .unwrap()
            .is_none()
    );
    ignored = value.clone();
    ignored["message"]["text"] = json!("hello");
    assert!(
        incoming(&channel, &ignored, 2, "ait_bot")
            .unwrap()
            .is_none()
    );
    ignored["message"]["reply_to_message"] = json!({"from":{"id":2}});
    assert!(
        incoming(&channel, &ignored, 2, "ait_bot")
            .unwrap()
            .is_some()
    );
    ignored["message"]["from"]["is_bot"] = json!(true);
    assert!(
        incoming(&channel, &ignored, 2, "ait_bot")
            .unwrap()
            .is_none()
    );
}

#[test]
fn connected_status_warns_only_when_group_privacy_hides_accepted_mentions() {
    let groups = channel();
    let mut direct_only = channel();
    direct_only.config = ChannelConfig::Telegram {
        token_ref: "env:AIT_SERVER_CREDENTIAL_TELEGRAM".into(),
        allowed_senders: vec!["1".into()],
        allow_groups: false,
    };
    let private = json!({"can_read_all_group_messages":false});
    let open = json!({"can_read_all_group_messages":true});

    let warned = connected_status(&groups, &private, 2);
    let silent = [
        connected_status(&groups, &open, 2),
        connected_status(&direct_only, &private, 2),
    ];

    assert_eq!(
        warned,
        json!({"state":"connected","accountId":"2","error":GROUP_PRIVACY_WARNING})
    );
    for status in silent {
        assert_eq!(status, json!({"state":"connected","accountId":"2"}));
    }
}
