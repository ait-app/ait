//! Feishu channel onboarding: the daemon runs scan-to-create and clients only show the QR code.
//!
//! The registration exchange follows Feishu's SDK; see ADR-224.

use std::io::Write;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use domain::ChannelId;
use domain::bot::ChannelConfig;
use flate2::Compression;
use flate2::write::GzEncoder;
use model::ErrorCode;
use secrecy::SecretString;
use serde_json::{Value, json};
use tokio::time::Instant;
use uuid::Uuid;

use super::API;
use crate::channels::onboarding::{
    FAILED_EXPIRED, FAILED_UNAVAILABLE, Sessions, failed, http_client, save_channel, waiting,
};
use crate::channels::{expose, json_response};
use crate::ports::stored_credential_reference;
use crate::protocol::{FeishuConnect, FeishuScanRequest, FeishuScanStart};
use crate::service::Bots;
use crate::state::StoredChannel;

const ACCOUNTS: &str = "https://accounts.feishu.cn";
const REGISTRATION_PATH: &str = "/oauth/v1/app/registration";
const TOKEN_PATH: &str = "/open-apis/auth/v3/tenant_access_token/internal";
const APP_NAME: &str = "Ait Bot";
const APP_DESCRIPTION: &str = "Ait Workspace Bot";
const SOURCE: &str = "ait";
const DEFAULT_EXPIRY_SECONDS: u64 = 600;
const MAX_EXPIRY_SECONDS: u64 = 3600;
const DEFAULT_POLL_SECONDS: u64 = 5;
const MAX_POLL_SECONDS: u64 = 60;
const SLOW_DOWN_STEP: Duration = Duration::from_secs(5);
const MAX_SECRET_BYTES: usize = 4096;
/// Query keys this client sets on Feishu's verification link.
const LINK_KEYS: [&str; 7] = [
    "from",
    "tp",
    "source",
    "name",
    "desc",
    "createOnly",
    "addons",
];

const FAILED_DENIED: &str = "denied";
const FAILED_LARK: &str = "larkUnsupported";

#[derive(Debug)]
struct Scan {
    id: Uuid,
    channel_id: ChannelId,
    allow_groups: bool,
    client: reqwest::Client,
    device_code: SecretString,
    interval: Duration,
    next_poll: Instant,
    deadline: Instant,
    /// An approved app whose channel has not been saved yet; Feishu reveals it only once.
    registered: Option<Registered>,
    outcome: Option<Value>,
}

impl Scan {
    fn finish(&mut self, outcome: Value) -> Value {
        self.outcome = Some(outcome.clone());
        outcome
    }
}

#[derive(Debug)]
pub(crate) struct Onboarding {
    accounts: String,
    open: String,
    scans: Sessions<Scan>,
}

impl Default for Onboarding {
    fn default() -> Self {
        Self::at(ACCOUNTS, API)
    }
}

/// The app Feishu created for the person who scanned.
#[derive(Debug)]
struct Registered {
    app_id: String,
    secret: SecretString,
    scanner: String,
}

enum Poll {
    Waiting,
    SlowDown,
    Created(Registered),
    Failed(&'static str),
}

impl Onboarding {
    fn at(accounts: &str, open: &str) -> Self {
        Self {
            accounts: accounts.to_owned(),
            open: open.to_owned(),
            scans: Sessions::default(),
        }
    }

    /// Begin a scan, replacing any earlier scan for the Workspace.
    pub(crate) async fn start(
        &self,
        bots: &Bots,
        request: FeishuScanStart,
    ) -> Result<Value, ErrorCode> {
        bots.check_channel_admission(request.workspace_id.clone())
            .await?;
        let client = http_client()?;
        let init = registration(&client, &self.accounts, &[("action", "init")]).await?;
        let supported = init["supported_auth_methods"]
            .as_array()
            .is_some_and(|methods| methods.iter().any(|method| method == "client_secret"));
        if !supported {
            return Err(ErrorCode::BotIo);
        }
        let begin = registration(
            &client,
            &self.accounts,
            &[
                ("action", "begin"),
                ("archetype", "PersonalAgent"),
                ("auth_method", "client_secret"),
                ("request_user_info", "open_id"),
            ],
        )
        .await?;
        let device_code = begin["device_code"].as_str().ok_or(ErrorCode::BotIo)?;
        let url = scan_link(
            begin["verification_uri_complete"]
                .as_str()
                .ok_or(ErrorCode::BotIo)?,
        )?;
        let expires = begin["expires_in"]
            .as_u64()
            .unwrap_or(DEFAULT_EXPIRY_SECONDS)
            .clamp(1, MAX_EXPIRY_SECONDS);
        let interval = begin["interval"]
            .as_u64()
            .unwrap_or(DEFAULT_POLL_SECONDS)
            .clamp(1, MAX_POLL_SECONDS);
        let id = Uuid::new_v4();
        let now = Instant::now();
        let scan = Scan {
            id,
            channel_id: ChannelId::generate(),
            allow_groups: request.allow_groups,
            client,
            device_code: SecretString::from(device_code.to_owned()),
            interval: Duration::from_secs(interval),
            next_poll: now,
            deadline: now + Duration::from_secs(expires),
            registered: None,
            outcome: None,
        };
        self.scans.insert(request.workspace_id, scan);
        Ok(json!({"scanId":id,"url":url,"expiresInSeconds":expires}))
    }

    /// Report a scan, asking Feishu at most once per poll interval.
    /// Network failures keep waiting, so a brief outage cannot lose an approved scan.
    pub(crate) async fn status(
        &self,
        bots: &Bots,
        request: FeishuScanRequest,
    ) -> Result<Value, ErrorCode> {
        let Some(scan) = self.scans.get(&request.workspace_id) else {
            return Ok(failed(FAILED_EXPIRED));
        };
        let mut scan = scan.lock().await;
        if scan.id != request.scan_id {
            return Ok(failed(FAILED_EXPIRED));
        }
        if let Some(outcome) = &scan.outcome {
            return Ok(outcome.clone());
        }
        let now = Instant::now();
        if now >= scan.deadline {
            return Ok(scan.finish(failed(FAILED_EXPIRED)));
        }
        if now < scan.next_poll {
            return Ok(waiting());
        }
        scan.next_poll = now + scan.interval;
        if scan.registered.is_some() {
            return Ok(save(bots, &mut scan, request.workspace_id).await);
        }
        let response = registration(
            &scan.client,
            &self.accounts,
            &[
                ("action", "poll"),
                ("device_code", expose(&scan.device_code)),
            ],
        )
        .await;
        let Ok(response) = response else {
            return Ok(waiting());
        };
        match classify(&response) {
            Poll::Waiting => Ok(waiting()),
            Poll::SlowDown => {
                let interval = scan.interval + SLOW_DOWN_STEP;
                scan.interval = interval.min(Duration::from_secs(MAX_POLL_SECONDS));
                scan.next_poll = now + scan.interval;
                Ok(waiting())
            }
            Poll::Failed(reason) => Ok(scan.finish(failed(reason))),
            Poll::Created(registered) => {
                scan.registered = Some(registered);
                Ok(save(bots, &mut scan, request.workspace_id).await)
            }
        }
    }

    /// Stop a scan so a later approval creates no channel; a finished channel is unaffected.
    pub(crate) async fn cancel(&self, request: FeishuScanRequest) -> Result<Value, ErrorCode> {
        let Some(entry) = self.scans.get(&request.workspace_id) else {
            return Ok(json!({"cancelled":false}));
        };
        {
            let mut scan = entry.lock().await;
            if scan.id != request.scan_id || scan.outcome.is_some() {
                return Ok(json!({"cancelled":false}));
            }
            scan.registered = None;
            scan.finish(failed(FAILED_EXPIRED));
        }
        self.scans.remove(&request.workspace_id, &entry);
        Ok(json!({"cancelled":true}))
    }

    /// Verify manually entered credentials with Feishu, then store and enable the channel.
    pub(crate) async fn connect(
        &self,
        bots: &Bots,
        request: FeishuConnect,
    ) -> Result<Value, ErrorCode> {
        if request.app_secret.is_empty()
            || request.app_secret.len() > MAX_SECRET_BYTES
            || request.app_secret.chars().any(char::is_control)
        {
            return Err(ErrorCode::InvalidMessage);
        }
        bots.check_channel_admission(request.workspace_id.clone())
            .await?;
        let secret = SecretString::from(request.app_secret);
        let token = json_response(
            http_client()?
                .post(format!("{}{TOKEN_PATH}", self.open))
                .json(&json!({"app_id":request.app_id,"app_secret":expose(&secret)}))
                .send()
                .await
                .map_err(|_| ErrorCode::BotIo)?,
        )
        .await?;
        if token["code"] != 0 {
            return Err(ErrorCode::InvalidMessage);
        }
        let channel = StoredChannel {
            workspace_id: request.workspace_id,
            channel_id: request.channel_id,
            config: ChannelConfig::Feishu {
                app_id: request.app_id,
                secret_ref: stored_credential_reference(request.channel_id),
                allowed_senders: request.allowed_senders,
                allow_groups: request.allow_groups,
            },
        };
        bots.create_stored_channel(channel, secret).await
    }
}

/// Save an approved app's channel, keeping it for the next status after a transient failure.
async fn save(bots: &Bots, scan: &mut Scan, workspace: String) -> Value {
    let Some(registered) = &scan.registered else {
        return waiting();
    };
    let channel = StoredChannel {
        workspace_id: workspace,
        channel_id: scan.channel_id,
        config: ChannelConfig::Feishu {
            app_id: registered.app_id.clone(),
            secret_ref: stored_credential_reference(scan.channel_id),
            allowed_senders: vec![registered.scanner.clone()],
            allow_groups: scan.allow_groups,
        },
    };
    let secret = registered.secret.clone();
    match save_channel(bots, channel, secret).await {
        Some(outcome) => {
            scan.registered = None;
            scan.finish(outcome)
        }
        None => waiting(),
    }
}

fn classify(response: &Value) -> Poll {
    let user = &response["user_info"];
    // The channel transport speaks only to Feishu's China region.
    if user["tenant_brand"] == "lark" {
        return Poll::Failed(FAILED_LARK);
    }
    if let (Some(app_id), Some(secret)) = (
        response["client_id"].as_str(),
        response["client_secret"].as_str(),
    ) {
        return user["open_id"]
            .as_str()
            .filter(|scanner| !scanner.is_empty())
            .map_or(Poll::Failed(FAILED_UNAVAILABLE), |scanner| {
                Poll::Created(Registered {
                    app_id: app_id.to_owned(),
                    secret: SecretString::from(secret.to_owned()),
                    scanner: scanner.to_owned(),
                })
            });
    }
    match response["error"].as_str() {
        Some("slow_down") => Poll::SlowDown,
        Some("access_denied") => Poll::Failed(FAILED_DENIED),
        Some("expired_token") => Poll::Failed(FAILED_EXPIRED),
        // Includes transient gateway errors; the scan's deadline bounds the wait.
        _ => Poll::Waiting,
    }
}

/// Post one registration step; Feishu reports a pending scan as a JSON error status.
async fn registration(
    client: &reqwest::Client,
    accounts: &str,
    form: &[(&str, &str)],
) -> Result<Value, ErrorCode> {
    json_response(
        client
            .post(format!("{accounts}{REGISTRATION_PATH}"))
            .form(form)
            .send()
            .await
            .map_err(|_| ErrorCode::BotIo)?,
    )
    .await
}

/// Build the link to show as a QR code, refusing a link that leaves Feishu.
fn scan_link(verification_uri: &str) -> Result<String, ErrorCode> {
    let mut url = url::Url::parse(verification_uri).map_err(|_| ErrorCode::BotIo)?;
    let trusted = url
        .host_str()
        .is_some_and(|host| host == "feishu.cn" || host.ends_with(".feishu.cn"));
    if url.scheme() != "https" || !trusted {
        return Err(ErrorCode::BotIo);
    }
    let kept: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(key, _)| !LINK_KEYS.contains(&key.as_ref()))
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();
    let addons = encoded_addons()?;
    url.query_pairs_mut()
        .clear()
        .extend_pairs(kept)
        .extend_pairs([
            ("from", "sdk"),
            ("tp", "sdk"),
            ("source", SOURCE),
            ("name", APP_NAME),
            ("desc", APP_DESCRIPTION),
            ("createOnly", "true"),
            ("addons", addons.as_str()),
        ]);
    Ok(url.into())
}

/// The permissions and event a new app needs to chat over the long connection.
fn encoded_addons() -> Result<String, ErrorCode> {
    let addons = json!({
        "scopes":{"tenant":[
            "im:message:send_as_bot",
            "im:message.p2p_msg:readonly",
            "im:message.group_at_msg:readonly",
        ]},
        "events":{"items":{"tenant":["im.message.receive_v1"]}},
    });
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(addons.to_string().as_bytes())
        .map_err(|_| ErrorCode::BotIo)?;
    let compressed = encoder.finish().map_err(|_| ErrorCode::BotIo)?;
    Ok(URL_SAFE_NO_PAD.encode(compressed))
}

#[cfg(test)]
mod tests;
