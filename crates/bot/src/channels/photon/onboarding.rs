//! Guided iMessage onboarding: the daemon assigns a shared Photon line to the user's own handle,
//! then waits for a one-time code sent from that handle, which proves they own it.
//!
//! Status requests read the project's messages, so no connection outlives them. See ADR-226.

use std::time::Duration;

use chrono::{SecondsFormat, Utc};
use domain::ChannelId;
use domain::bot::ChannelConfig;
use model::ErrorCode;
use reqwest::StatusCode;
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::time::Instant;
use uuid::Uuid;

use super::{API, PLATFORM, STATUS_RECEIVED, valid_project_id};
use crate::channels::onboarding::{
    FAILED_ALREADY_CONNECTED, FAILED_EXPIRED, FAILED_UNAVAILABLE, Sessions, failed, http_client,
    save_channel, waiting,
};
use crate::channels::{expose, json_response};
use crate::ports::stored_credential_reference;
use crate::protocol::{ImessagePairStart, PairRequest};
use crate::service::Bots;
use crate::state::StoredChannel;

const PAIRING_LIFETIME: Duration = Duration::from_secs(600);
const POLL_INTERVAL: Duration = Duration::from_secs(2);
/// Tolerates a daemon clock running ahead of Photon's when asking for recent messages.
const CLOCK_MARGIN_MINUTES: i64 = 5;
const MESSAGES_PER_POLL: &str = "50";
const ASSIGNMENTS_PER_PAGE: &str = "100";
/// Bounds the search for an existing assignment; a project allows far fewer shared lines.
const MAX_ASSIGNMENT_PAGES: usize = 20;
const CODE_PREFIX: &str = "ait-";
/// Length of a code's random part, a UUID's simple hexadecimal form.
const CODE_RANDOM_LENGTH: usize = 32;
const KEY_PREFIX: &str = "pho_sk_";
const MAX_KEY_LENGTH: usize = 512;
const MAX_EMAIL_LENGTH: usize = 254;
const MAX_E164_DIGITS: usize = 15;

const FAILED_INVALID_KEY: &str = "invalidKey";
const FAILED_INVALID_HANDLE: &str = "invalidHandle";
const FAILED_DEDICATED_LINE: &str = "dedicatedLine";

const ASSIGNMENT_EXISTS: &str = "ASSIGNMENT_EXISTS";
const IMESSAGE_MODE_DEDICATED: &str = "IMESSAGE_MODE_DEDICATED";

#[derive(Debug)]
struct Pairing {
    id: Uuid,
    channel_id: ChannelId,
    project_id: String,
    key: SecretString,
    handle: String,
    code: String,
    since: String,
    client: reqwest::Client,
    next_poll: Instant,
    deadline: Instant,
    paired: Option<String>,
    outcome: Option<Value>,
}

impl Pairing {
    fn finish(&mut self, outcome: Value) -> Value {
        self.outcome = Some(outcome.clone());
        outcome
    }
}

/// Authenticated access to one Photon project.
struct Project<'a> {
    client: &'a reqwest::Client,
    api: &'a str,
    id: &'a str,
    key: &'a SecretString,
}

impl Project<'_> {
    fn url(&self, path: &str) -> String {
        format!("{}/v1/projects/{}{path}", self.api, self.id)
    }

    /// Assign a shared line to `handle`, returning its number or the reason it was refused.
    async fn assign(&self, handle: &str, attempt: Uuid) -> Result<String, &'static str> {
        let response = self
            .client
            .post(self.url("/platforms/imessage/assignments"))
            .bearer_auth(expose(self.key))
            .header("idempotency-key", format!("ait-assign:{attempt}"))
            .json(&json!({"handle":handle}))
            .send()
            .await
            .map_err(|_| FAILED_UNAVAILABLE)?;
        let status = response.status();
        let body = json_response(response).await.unwrap_or(Value::Null);
        if status.is_success() {
            return phone_number(&body).ok_or(FAILED_UNAVAILABLE);
        }
        if status == StatusCode::CONFLICT {
            return match body["code"].as_str() {
                Some(ASSIGNMENT_EXISTS) => self.existing(handle).await,
                Some(IMESSAGE_MODE_DEDICATED) => Err(FAILED_DEDICATED_LINE),
                _ => Err(FAILED_UNAVAILABLE),
            };
        }
        Err(match status {
            StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY => FAILED_INVALID_HANDLE,
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::NOT_FOUND => {
                FAILED_INVALID_KEY
            }
            _ => FAILED_UNAVAILABLE,
        })
    }

    /// Find the line already assigned to `handle`; the list cannot be filtered by handle.
    async fn existing(&self, handle: &str) -> Result<String, &'static str> {
        let mut token: Option<String> = None;
        for _ in 0..MAX_ASSIGNMENT_PAGES {
            let mut query = vec![("pageSize", ASSIGNMENTS_PER_PAGE)];
            if let Some(token) = &token {
                query.push(("pageToken", token.as_str()));
            }
            let page = self
                .get("/platforms/imessage/assignments", &query)
                .await
                .map_err(|refusal| refusal.unwrap_or(FAILED_UNAVAILABLE))?;
            let assignments = page["assignments"]
                .as_array()
                .map_or(&[][..], Vec::as_slice);
            if let Some(number) = assignments
                .iter()
                .find(|assignment| same_handle(&assignment["handle"], handle))
                .and_then(phone_number)
            {
                return Ok(number);
            }
            token = page["nextPageToken"].as_str().map(str::to_owned);
            if token.is_none() {
                break;
            }
        }
        Err(FAILED_UNAVAILABLE)
    }

    /// `Err(Some(reason))` is a permanent refusal; `Err(None)` is worth retrying.
    async fn get(&self, path: &str, query: &[(&str, &str)]) -> Result<Value, Option<&'static str>> {
        let response = self
            .client
            .get(self.url(path))
            .bearer_auth(expose(self.key))
            .query(query)
            .send()
            .await
            .map_err(|_| None)?;
        match response.status() {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN | StatusCode::NOT_FOUND => {
                Err(Some(FAILED_INVALID_KEY))
            }
            status if status.is_success() => json_response(response).await.map_err(|_| None),
            _ => Err(None),
        }
    }
}

fn phone_number(assignment: &Value) -> Option<String> {
    assignment["assignedPhoneNumber"]
        .as_str()
        .filter(|number| !number.is_empty())
        .map(str::to_owned)
}

fn same_handle(candidate: &Value, handle: &str) -> bool {
    candidate
        .as_str()
        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(handle))
}

/// Whether `text` is a pairing code, which may arrive again after its channel exists.
pub(super) fn is_pairing_code(text: &str) -> bool {
    text.trim().strip_prefix(CODE_PREFIX).is_some_and(|random| {
        random.len() == CODE_RANDOM_LENGTH && random.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}

/// Photon's project API key shape, which is placed in request headers.
fn valid_key(key: &str) -> bool {
    key.len() > KEY_PREFIX.len()
        && key.len() <= MAX_KEY_LENGTH
        && key.starts_with(KEY_PREFIX)
        && key.bytes().all(|byte| byte.is_ascii_graphic())
}

/// A rough E.164 or email check; Photon validates the rest when assigning the line.
fn valid_handle(handle: &str) -> bool {
    if let Some(digits) = handle.strip_prefix('+') {
        return (2..=MAX_E164_DIGITS).contains(&digits.len())
            && !digits.starts_with('0')
            && digits.bytes().all(|byte| byte.is_ascii_digit());
    }
    handle.len() <= MAX_EMAIL_LENGTH
        && handle.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty() && domain.contains('.') && !domain.contains('@')
        })
        && handle.bytes().all(|byte| byte.is_ascii_graphic())
}

/// The handle that sent `code` from the messages listed, newest first.
fn code_sender(messages: &[Value], code: &str, handle: &str) -> Option<String> {
    messages
        .iter()
        .find(|message| {
            message["status"] == STATUS_RECEIVED
                && message["content"]["type"] == super::CONTENT_TEXT
                && message["content"]["text"]
                    .as_str()
                    .is_some_and(|text| text.trim() == code)
                && same_handle(&message["sentBy"]["handle"], handle)
        })
        .and_then(|message| message["sentBy"]["handle"].as_str())
        .map(str::to_owned)
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

    /// Check the pasted credentials, assign a line, and begin waiting for the code.
    pub(crate) async fn start(
        &self,
        bots: &Bots,
        request: ImessagePairStart,
    ) -> Result<Value, ErrorCode> {
        let project_id = request.project_id.trim();
        let key = request.api_key.trim();
        let handle = request.handle.trim();
        if !valid_project_id(project_id) || !valid_key(key) {
            return Ok(failed(FAILED_INVALID_KEY));
        }
        if !valid_handle(handle) {
            return Ok(failed(FAILED_INVALID_HANDLE));
        }
        bots.check_channel_admission(request.workspace_id.clone())
            .await?;
        if bots.photon_project_claimed(project_id.to_owned()).await? {
            return Ok(failed(FAILED_ALREADY_CONNECTED));
        }
        let key = SecretString::from(key.to_owned());
        let client = http_client()?;
        let id = Uuid::new_v4();
        let project = Project {
            client: &client,
            api: &self.api,
            id: project_id,
            key: &key,
        };
        let number = match project.assign(handle, id).await {
            Ok(number) => number,
            Err(reason) => return Ok(failed(reason)),
        };
        let code = format!("{CODE_PREFIX}{}", Uuid::new_v4().simple());
        let response = json!({
            "state":"waiting",
            "pairingId":id,
            "phoneNumber":number,
            "code":code,
            "url":format!("sms:{number}&body={code}"),
            "expiresInSeconds":PAIRING_LIFETIME.as_secs(),
        });
        let now = Instant::now();
        let since = (Utc::now() - chrono::Duration::minutes(CLOCK_MARGIN_MINUTES))
            .to_rfc3339_opts(SecondsFormat::Secs, true);
        self.pairings.insert(
            request.workspace_id,
            Pairing {
                id,
                channel_id: ChannelId::generate(),
                project_id: project_id.to_owned(),
                key,
                handle: handle.to_owned(),
                code,
                since,
                client,
                next_poll: now,
                deadline: now + PAIRING_LIFETIME,
                paired: None,
                outcome: None,
            },
        );
        Ok(response)
    }

    /// Report a pairing, reading the project's messages at most once per poll interval.
    pub(crate) async fn status(
        &self,
        bots: &Bots,
        request: PairRequest,
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
        // Once the code has arrived, a pending save outlives the code's own lifetime.
        if now >= pairing.deadline && pairing.paired.is_none() {
            return Ok(pairing.finish(failed(FAILED_EXPIRED)));
        }
        if now < pairing.next_poll {
            return Ok(waiting());
        }
        pairing.next_poll = now + POLL_INTERVAL;
        if pairing.paired.is_none() {
            match self.find_code(&pairing).await {
                Ok(sender) => pairing.paired = sender,
                Err(reason) => return Ok(pairing.finish(failed(reason))),
            }
        }
        let Some(sender) = pairing.paired.clone() else {
            return Ok(waiting());
        };
        let channel = StoredChannel {
            workspace_id: request.workspace_id,
            channel_id: pairing.channel_id,
            config: ChannelConfig::PhotonImessage {
                project_id: pairing.project_id.clone(),
                key_ref: stored_credential_reference(pairing.channel_id),
                allowed_senders: vec![sender],
            },
        };
        match save_channel(bots, channel, pairing.key.clone()).await {
            Some(outcome) => Ok(pairing.finish(outcome)),
            None => Ok(waiting()),
        }
    }

    /// Look for the code; `Ok(None)` means not yet, including after a transient failure.
    async fn find_code(&self, pairing: &Pairing) -> Result<Option<String>, &'static str> {
        let project = Project {
            client: &pairing.client,
            api: &self.api,
            id: &pairing.project_id,
            key: &pairing.key,
        };
        let query = [
            ("direction", "received"),
            ("platform", PLATFORM),
            ("createdSince", pairing.since.as_str()),
            ("order", "desc"),
            ("pageSize", MESSAGES_PER_POLL),
        ];
        match project.get("/messages", &query).await {
            Ok(page) => {
                let messages = page["items"].as_array().map_or(&[][..], Vec::as_slice);
                Ok(code_sender(messages, &pairing.code, &pairing.handle))
            }
            Err(Some(reason)) => Err(reason),
            Err(None) => Ok(None),
        }
    }

    /// Stop a pairing so a later code message creates no channel.
    pub(crate) async fn cancel(&self, request: PairRequest) -> Result<Value, ErrorCode> {
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

#[cfg(test)]
mod tests;
