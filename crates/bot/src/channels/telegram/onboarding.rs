//! Guided Telegram onboarding: the user pastes a bot token, then taps Start in a link carrying
//! a one-time code, which identifies them as the channel's allowed sender.
//!
//! Until the channel exists, status requests read the bot's updates themselves. See ADR-224.

use std::time::Duration;

use domain::ChannelId;
use domain::bot::ChannelConfig;
use model::ErrorCode;
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::time::Instant;
use uuid::Uuid;

use super::API;
use crate::channels::onboarding::{
    FAILED_ALREADY_CONNECTED, FAILED_EXPIRED, FAILED_UNAVAILABLE, Sessions, failed, http_client,
    save_channel, waiting,
};
use crate::channels::{expose, json_response};
use crate::ports::stored_credential_reference;
use crate::protocol::{TelegramPairRequest, TelegramPairStart};
use crate::service::Bots;
use crate::state::StoredChannel;

const PAIRING_LIFETIME: Duration = Duration::from_secs(600);
const POLL_INTERVAL: Duration = Duration::from_secs(2);
/// Length of a code rendered as a UUID's simple hexadecimal form.
const PAIRING_CODE_LENGTH: usize = 32;
const UPDATES_PER_POLL: u8 = 50;

const FAILED_INVALID_TOKEN: &str = "invalidToken";
const FAILED_POLLING_CONFLICT: &str = "pollingConflict";
const FAILED_WEBHOOK_CONFLICT: &str = "webhookConflict";

#[derive(Debug)]
struct Pairing {
    id: Uuid,
    channel_id: ChannelId,
    allow_groups: bool,
    client: reqwest::Client,
    token: SecretString,
    code: String,
    offset: i64,
    next_poll: Instant,
    deadline: Instant,
    paired: Option<Paired>,
    outcome: Option<Value>,
}

impl Pairing {
    fn finish(&mut self, outcome: Value) -> Value {
        self.outcome = Some(outcome.clone());
        outcome
    }
}

/// The person who sent the one-time code, and the offset that acknowledges their message.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Paired {
    sender: String,
    next_offset: i64,
}

#[derive(Debug)]
pub(crate) struct Onboarding {
    api: String,
    pairings: Sessions<Pairing>,
}

impl Default for Onboarding {
    fn default() -> Self {
        Self::at(API)
    }
}

impl Onboarding {
    fn at(api: &str) -> Self {
        Self {
            api: api.to_owned(),
            pairings: Sessions::default(),
        }
    }

    /// Verify a pasted token and begin waiting for its owner to tap Start.
    pub(crate) async fn start(
        &self,
        bots: &Bots,
        request: TelegramPairStart,
    ) -> Result<Value, ErrorCode> {
        let token = request.token.trim();
        if !valid_token(token) {
            return Ok(failed(FAILED_INVALID_TOKEN));
        }
        bots.check_channel_admission(request.workspace_id.clone())
            .await?;
        let token = SecretString::from(token.to_owned());
        let client = http_client()?;
        let Ok(me) = bot_api(&client, &self.api, &token, "getMe", json!({})).await else {
            return Ok(failed(FAILED_UNAVAILABLE));
        };
        if let Some(reason) = refusal(&me) {
            return Ok(failed(reason));
        }
        // Pairing reads this bot's updates, which would take them from a channel already using it.
        let identity = format!("telegram:{}", me["result"]["id"]);
        if !bots.account_available(identity).await? {
            return Ok(failed(FAILED_ALREADY_CONNECTED));
        }
        let Some(username) = me["result"]["username"]
            .as_str()
            .filter(|name| valid_username(name))
        else {
            return Ok(failed(FAILED_UNAVAILABLE));
        };
        let Ok(webhook) = bot_api(&client, &self.api, &token, "getWebhookInfo", json!({})).await
        else {
            return Ok(failed(FAILED_UNAVAILABLE));
        };
        if webhook["ok"] != true {
            return Ok(failed(refusal(&webhook).unwrap_or(FAILED_UNAVAILABLE)));
        }
        if webhook["result"]["url"].as_str() != Some("") {
            return Ok(failed(FAILED_WEBHOOK_CONFLICT));
        }
        let id = Uuid::new_v4();
        let code = Uuid::new_v4().simple().to_string();
        let url = format!("https://t.me/{username}?start={code}");
        let now = Instant::now();
        let response = json!({
            "state":"waiting",
            "pairingId":id,
            "botUsername":username,
            "url":url,
            "expiresInSeconds":PAIRING_LIFETIME.as_secs(),
        });
        self.pairings.insert(
            request.workspace_id,
            Pairing {
                id,
                channel_id: ChannelId::generate(),
                allow_groups: request.allow_groups,
                client,
                token,
                code,
                offset: 0,
                next_poll: now,
                deadline: now + PAIRING_LIFETIME,
                paired: None,
                outcome: None,
            },
        );
        Ok(response)
    }

    /// Report a pairing, reading the bot's updates at most once per poll interval.
    pub(crate) async fn status(
        &self,
        bots: &Bots,
        request: TelegramPairRequest,
    ) -> Result<Value, ErrorCode> {
        let Some(pairing) = self.pairings.get(&request.workspace_id) else {
            return Ok(failed(FAILED_EXPIRED));
        };
        let mut pairing = pairing.lock().await;
        if pairing.id != request.pairing_id {
            return Ok(failed(FAILED_EXPIRED));
        }
        if let Some(outcome) = &pairing.outcome {
            return Ok(outcome.clone());
        }
        let now = Instant::now();
        if now >= pairing.deadline {
            return Ok(pairing.finish(failed(FAILED_EXPIRED)));
        }
        if now < pairing.next_poll {
            return Ok(waiting());
        }
        pairing.next_poll = now + POLL_INTERVAL;
        if pairing.paired.is_none() {
            let outcome = self.read_updates(&mut pairing).await;
            if pairing.paired.is_none() {
                return Ok(outcome);
            }
        }
        Ok(self
            .acknowledge_and_save(bots, &mut pairing, request.workspace_id)
            .await)
    }

    /// Look for the one-time code; every update read before it is discarded.
    async fn read_updates(&self, pairing: &mut Pairing) -> Value {
        let offset = pairing.offset;
        let Some(envelope) = self.get_updates(pairing, offset, UPDATES_PER_POLL).await else {
            return waiting();
        };
        if let Some(reason) = refusal(&envelope) {
            return pairing.finish(failed(reason));
        }
        let updates = envelope["result"].as_array().map_or(&[][..], Vec::as_slice);
        let code = pairing.code.clone();
        pairing.paired = find_code(updates, &code, &mut pairing.offset);
        waiting()
    }

    /// Acknowledge the code message so the channel does not treat it as the first prompt.
    async fn acknowledge_and_save(
        &self,
        bots: &Bots,
        pairing: &mut Pairing,
        workspace: String,
    ) -> Value {
        let Some(paired) = pairing.paired.clone() else {
            return waiting();
        };
        let Some(envelope) = self.get_updates(pairing, paired.next_offset, 1).await else {
            return waiting();
        };
        if let Some(reason) = refusal(&envelope) {
            return pairing.finish(failed(reason));
        }
        if envelope["ok"] != true {
            return waiting();
        }
        let channel = StoredChannel {
            workspace_id: workspace,
            channel_id: pairing.channel_id,
            config: ChannelConfig::Telegram {
                token_ref: stored_credential_reference(pairing.channel_id),
                allowed_senders: vec![paired.sender],
                allow_groups: pairing.allow_groups,
            },
        };
        match save_channel(bots, channel, pairing.token.clone()).await {
            Some(outcome) => pairing.finish(outcome),
            None => waiting(),
        }
    }

    /// Read updates without waiting; `None` reports a network failure worth retrying.
    async fn get_updates(&self, pairing: &Pairing, offset: i64, limit: u8) -> Option<Value> {
        let body = json!({"offset":offset,"timeout":0,"limit":limit,"allowed_updates":["message"]});
        bot_api(
            &pairing.client,
            &self.api,
            &pairing.token,
            "getUpdates",
            body,
        )
        .await
        .ok()
    }

    /// Stop a pairing so a later tap on Start creates no channel.
    pub(crate) async fn cancel(&self, request: TelegramPairRequest) -> Result<Value, ErrorCode> {
        let Some(entry) = self.pairings.get(&request.workspace_id) else {
            return Ok(json!({"cancelled":false}));
        };
        {
            let mut pairing = entry.lock().await;
            if pairing.id != request.pairing_id || pairing.outcome.is_some() {
                return Ok(json!({"cancelled":false}));
            }
            pairing.finish(failed(FAILED_EXPIRED));
        }
        self.pairings.remove(&request.workspace_id, &entry);
        Ok(json!({"cancelled":true}))
    }
}

/// Telegram's reason for refusing this token permanently, when the envelope carries one.
fn refusal(envelope: &Value) -> Option<&'static str> {
    match envelope["error_code"].as_i64() {
        Some(401 | 404) => Some(FAILED_INVALID_TOKEN),
        Some(409) => Some(FAILED_POLLING_CONFLICT),
        _ => None,
    }
}

/// Find the code in a private chat, advancing `offset` past every update read before it.
fn find_code(updates: &[Value], code: &str, offset: &mut i64) -> Option<Paired> {
    let expected = format!("/start {code}");
    for update in updates {
        let next_offset = update["update_id"].as_i64()?.checked_add(1)?;
        let message = &update["message"];
        let human_in_private =
            message["chat"]["type"] == "private" && message["from"]["is_bot"] != true;
        if human_in_private
            && message["text"] == expected.as_str()
            && let Some(sender) = message["from"]["id"].as_i64()
        {
            return Some(Paired {
                sender: sender.to_string(),
                next_offset,
            });
        }
        *offset = next_offset;
    }
    None
}

/// Telegram's `<numeric id>:<secret>` token shape, checked before any network use.
fn valid_token(token: &str) -> bool {
    token.split_once(':').is_some_and(|(id, secret)| {
        !id.is_empty()
            && id.bytes().all(|byte| byte.is_ascii_digit())
            && secret.len() >= 30
            && token.len() <= 256
            && secret
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
    })
}

/// Whether `text` is a pairing link's Start message, which may be sent again after pairing.
pub(super) fn is_pairing_start(text: &str) -> bool {
    text.strip_prefix("/start ").is_some_and(|code| {
        code.len() == PAIRING_CODE_LENGTH && code.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

fn valid_username(username: &str) -> bool {
    !username.is_empty()
        && username
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// Call the Bot API and return its envelope, whether or not Telegram accepted the call.
/// Transport errors are discarded because their URLs contain the token.
async fn bot_api(
    client: &reqwest::Client,
    api: &str,
    token: &SecretString,
    method: &str,
    body: Value,
) -> Result<Value, ErrorCode> {
    json_response(
        client
            .post(format!("{api}/bot{}/{method}", expose(token)))
            .json(&body)
            .send()
            .await
            .map_err(|_| ErrorCode::BotIo)?,
    )
    .await
}

#[cfg(test)]
mod tests;
