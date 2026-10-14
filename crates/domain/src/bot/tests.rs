use crate::{BotId, BotTurnId, ChannelId, ConversationId};

#[test]
fn bot_identities_roundtrip_and_reject_nil_or_malformed_values() {
    let bot = BotId::generate();
    assert_eq!(
        serde_json::from_str::<BotId>(&serde_json::to_string(&bot).unwrap()).unwrap(),
        bot
    );
    assert_eq!(
        serde_json::from_str::<ChannelId>(&serde_json::to_string(&ChannelId::generate()).unwrap())
            .unwrap()
            .to_string()
            .len(),
        36
    );
    assert_eq!(
        serde_json::from_str::<ConversationId>(
            &serde_json::to_string(&ConversationId::generate()).unwrap()
        )
        .unwrap()
        .to_string()
        .len(),
        36
    );
    assert_eq!(
        serde_json::from_str::<BotTurnId>(&serde_json::to_string(&BotTurnId::generate()).unwrap())
            .unwrap()
            .to_string()
            .len(),
        36
    );
    for invalid in [
        "\"00000000-0000-0000-0000-000000000000\"",
        "\"../escape\"",
        "1",
    ] {
        assert!(serde_json::from_str::<BotId>(invalid).is_err());
    }
}
