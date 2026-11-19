//! QQ scan-to-create and manual credentials; see ADR-225 and ADR-290.

use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use domain::bot::ChannelConfig;
use model::ErrorCode;
use reqwest::header::{ACCEPT, USER_AGENT};
use ring::aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
use secrecy::SecretString;
use serde_json::{Value, json};

use super::{TOKEN_URL, fetch_token};
use crate::channels::onboarding::{
    FAILED_ALREADY_CONNECTED, FAILED_EXPIRED, FAILED_UNAVAILABLE, Scan, Sessions, failed,
    http_client, waiting,
};
use crate::channels::{expose, json_response};
use crate::ports::stored_credential_reference;
use crate::protocol::{QqConnect, ScanRequest, ScanStart};
use crate::service::Bots;
use crate::state::StoredChannel;

const PORTAL: &str = "https://q.qq.com";
const CREATE_PATH: &str = "/lite/create_bind_task";
const POLL_PATH: &str = "/lite/poll_bind_result";
const CONNECT_PAGE: &str = "https://q.qq.com/qqbot/openclaw/connect.html";
const CLIENT_NAME: &str = "Ait";
const SOURCE: &str = "ait";
const EXPIRY_SECONDS: u64 = 600;
const POLL_INTERVAL: Duration = Duration::from_secs(2);
const STATUS_COMPLETED: i64 = 2;
const STATUS_EXPIRED: i64 = 3;
const KEY_BYTES: usize = 32;
const MAX_SECRET_BYTES: usize = 4096;
const TAG_BYTES: usize = 16;
#[derive(Debug)]
struct Flow {
    task_id: String,
    /// Base64 key QQ encrypts the new bot's secret with; only this daemon holds it.
    key: SecretString,
}

/// The bot QQ created for the person who scanned.
#[derive(Debug)]
struct Approved {
    app_id: String,
    secret: SecretString,
    scanner: String,
}

enum Poll {
    Waiting,
    Approved(Approved),
    Failed(&'static str),
}

/// One in-memory scan per Workspace.
#[derive(Debug)]
pub(crate) struct Onboarding {
    portal: String,
    token_url: String,
    scans: Sessions<Scan<Flow, Approved>>,
}

impl Default for Onboarding {
    fn default() -> Self {
        Self::at(PORTAL, TOKEN_URL)
    }
}

impl Onboarding {
    fn at(portal: &str, token_url: &str) -> Self {
        Self {
            portal: portal.to_owned(),
            token_url: token_url.to_owned(),
            scans: Sessions::default(),
        }
    }

    /// Begin a scan, replacing any earlier scan for the Workspace.
    pub(crate) async fn start(&self, bots: &Bots, request: ScanStart) -> Result<Value, ErrorCode> {
        bots.check_channel_admission(request.workspace_id.clone())
            .await?;
        let client = http_client()?;
        let key = random_key()?;
        let created = portal_post(
            &client,
            &self.portal,
            CREATE_PATH,
            &json!({"key":expose(&key)}),
        )
        .await?;
        if created["retcode"] != 0 {
            return Err(ErrorCode::BotIo);
        }
        let task_id = created["data"]["task_id"]
            .as_str()
            .filter(|task| !task.is_empty())
            .ok_or(ErrorCode::BotIo)?;
        let url = url::Url::parse_with_params(
            CONNECT_PAGE,
            [("task_id", task_id), ("_wv", "2"), ("source", SOURCE)],
        )
        .map_err(|_| ErrorCode::BotIo)?;
        let flow = Flow {
            task_id: task_id.to_owned(),
            key,
        };
        let scan = Scan::new(
            client,
            flow,
            POLL_INTERVAL,
            EXPIRY_SECONDS,
            request.allow_groups,
        );
        let id = scan.id;
        self.scans.insert(request.workspace_id, scan);
        Ok(json!({"scanId":id,"url":String::from(url),"expiresInSeconds":EXPIRY_SECONDS}))
    }

    /// Report a scan; network failures keep waiting, so an outage cannot lose an approval.
    pub(crate) async fn status(
        &self,
        bots: &Bots,
        request: ScanRequest,
    ) -> Result<Value, ErrorCode> {
        let Some(mut scan) = self.scans.current(&request).await else {
            return Ok(failed(FAILED_EXPIRED));
        };
        if let Some(settled) = scan.before_poll() {
            return Ok(settled);
        }
        if scan.approved.is_none() {
            match poll(&scan, &self.portal).await {
                Poll::Waiting => return Ok(waiting()),
                Poll::Failed(reason) => return Ok(scan.finish(failed(reason))),
                Poll::Approved(approved) => {
                    let identity = format!("qq:{}", approved.app_id);
                    if bots.account_available(identity).await == Ok(false) {
                        return Ok(scan.finish(failed(FAILED_ALREADY_CONNECTED)));
                    }
                    scan.approved = Some(approved);
                }
            }
        }
        let Some(approved) = &scan.approved else {
            return Ok(waiting());
        };
        let config = ChannelConfig::Qq {
            app_id: approved.app_id.clone(),
            secret_ref: stored_credential_reference(scan.channel_id),
            allowed_senders: vec![approved.scanner.clone()],
            allow_groups: scan.allow_groups,
        };
        let secret = approved.secret.clone();
        Ok(scan
            .save_approved(bots, request.workspace_id, config, secret)
            .await)
    }

    /// Stop a scan so a later approval creates no channel; a finished channel is unaffected.
    pub(crate) async fn cancel(&self, request: ScanRequest) -> Result<Value, ErrorCode> {
        Ok(self.scans.cancel(&request).await)
    }

    /// Verify pasted credentials, then store and enable a channel that awaits sender pairing.
    pub(crate) async fn connect(
        &self,
        bots: &Bots,
        request: QqConnect,
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
        fetch_token(&http_client()?, &self.token_url, &request.app_id, &secret)
            .await
            .map_err(|_| ErrorCode::InvalidMessage)?;
        let channel = StoredChannel {
            workspace_id: request.workspace_id,
            channel_id: request.channel_id,
            config: ChannelConfig::Qq {
                app_id: request.app_id,
                secret_ref: stored_credential_reference(request.channel_id),
                allowed_senders: Vec::new(),
                allow_groups: request.allow_groups,
            },
        };
        bots.create_stored_channel(channel, secret).await
    }
}

fn random_bytes<const N: usize>() -> Result<[u8; N], ErrorCode> {
    let mut bytes = [0; N];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| ErrorCode::BotIo)?;
    Ok(bytes)
}

fn random_key() -> Result<SecretString, ErrorCode> {
    Ok(SecretString::from(
        STANDARD.encode(random_bytes::<KEY_BYTES>()?),
    ))
}

/// POST to QQ's portal, which answers anything but an explicit JSON client with a challenge page.
async fn portal_post(
    client: &reqwest::Client,
    portal: &str,
    path: &str,
    body: &Value,
) -> Result<Value, ErrorCode> {
    json_response(
        client
            .post(format!("{portal}{path}"))
            .header(ACCEPT, "application/json")
            .header(USER_AGENT, CLIENT_NAME)
            .json(body)
            .send()
            .await
            .map_err(|_| ErrorCode::BotIo)?,
    )
    .await
}

async fn poll(scan: &Scan<Flow, Approved>, portal: &str) -> Poll {
    let body = json!({"task_id":scan.flow.task_id});
    let Ok(response) = portal_post(&scan.client, portal, POLL_PATH, &body).await else {
        return Poll::Waiting;
    };
    classify(&response, &scan.flow.key)
}

fn classify(response: &Value, key: &SecretString) -> Poll {
    if response["retcode"] != 0 {
        return Poll::Waiting;
    }
    let data = &response["data"];
    match data["status"].as_i64() {
        Some(STATUS_COMPLETED) => completed(data, key),
        Some(STATUS_EXPIRED) => Poll::Failed(FAILED_EXPIRED),
        _ => Poll::Waiting,
    }
}

fn completed(data: &Value, key: &SecretString) -> Poll {
    let field = |name: &str| data[name].as_str().filter(|value| !value.is_empty());
    let approved = match (
        field("bot_appid"),
        field("user_openid"),
        field("bot_encrypt_secret"),
    ) {
        (Some(app_id), Some(scanner), Some(encrypted)) => {
            decrypt_secret(encrypted, key).ok().map(|secret| Approved {
                app_id: app_id.to_owned(),
                secret,
                scanner: scanner.to_owned(),
            })
        }
        _ => None,
    };
    approved.map_or(Poll::Failed(FAILED_UNAVAILABLE), Poll::Approved)
}

/// Decrypt QQ's base64 of `nonce ‖ ciphertext ‖ tag`, sealed with AES-256-GCM.
fn decrypt_secret(encrypted: &str, key: &SecretString) -> Result<SecretString, ErrorCode> {
    let key = STANDARD.decode(expose(key)).map_err(|_| ErrorCode::BotIo)?;
    let mut sealed = STANDARD.decode(encrypted).map_err(|_| ErrorCode::BotIo)?;
    if sealed.len() < NONCE_LEN + TAG_BYTES {
        return Err(ErrorCode::BotIo);
    }
    let mut ciphertext = sealed.split_off(NONCE_LEN);
    let nonce = Nonce::try_assume_unique_for_key(&sealed).map_err(|_| ErrorCode::BotIo)?;
    let key = UnboundKey::new(&AES_256_GCM, &key).map_err(|_| ErrorCode::BotIo)?;
    let plain = LessSafeKey::new(key)
        .open_in_place(nonce, Aad::empty(), &mut ciphertext)
        .map_err(|_| ErrorCode::BotIo)?;
    let secret = std::str::from_utf8(plain).map_err(|_| ErrorCode::BotIo)?;
    if secret.is_empty() || secret.len() > MAX_SECRET_BYTES {
        return Err(ErrorCode::BotIo);
    }
    Ok(SecretString::from(secret.to_owned()))
}

#[cfg(test)]
mod tests;
