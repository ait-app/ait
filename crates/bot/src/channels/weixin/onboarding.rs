//! `WeChat` scan-to-connect: the daemon runs the `iLink` QR login and clients only show the code.

use std::time::Duration;

use domain::bot::ChannelConfig;
use model::ErrorCode;
use secrecy::SecretString;
use serde_json::{Value, json};

use super::{API, trusted_base, trusted_host, versioned};
use crate::channels::onboarding::{
    FAILED_ALREADY_CONNECTED, FAILED_EXPIRED, FAILED_UNAVAILABLE, Scan, Sessions, failed,
    http_client, waiting,
};
use crate::channels::{expose, json_response};
use crate::ports::stored_credential_reference;
use crate::protocol::{ScanRequest, ScanStart};
use crate::service::Bots;

const QR_PATH: &str = "ilink/bot/get_bot_qrcode";
const STATUS_PATH: &str = "ilink/bot/get_qrcode_status";
const BOT_TYPE: &str = "3";
const EXPIRY_SECONDS: u64 = 480;
const POLL_INTERVAL: Duration = Duration::from_secs(1);
/// iLink holds a status request open; a shorter wait keeps client status requests responsive.
const STATUS_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
struct Flow {
    qrcode: SecretString,
    base: String,
}

/// The bot identity iLink issued to the person who scanned.
#[derive(Debug)]
struct Approved {
    account_id: String,
    token: SecretString,
    base_url: String,
    scanner: String,
}

enum Poll {
    Waiting,
    Redirect(String),
    Approved(Approved),
    Failed(&'static str),
}

/// One in-memory scan per Workspace; a daemon restart simply asks the user to scan again.
#[derive(Debug)]
pub(crate) struct Onboarding {
    base: String,
    scans: Sessions<Scan<Flow, Approved>>,
}

impl Default for Onboarding {
    fn default() -> Self {
        Self::at(API)
    }
}

impl Onboarding {
    fn at(base: &str) -> Self {
        Self {
            base: base.to_owned(),
            scans: Sessions::default(),
        }
    }

    /// Begin a scan, replacing any earlier scan for the Workspace.
    pub(crate) async fn start(&self, bots: &Bots, request: ScanStart) -> Result<Value, ErrorCode> {
        bots.check_channel_admission(request.workspace_id.clone())
            .await?;
        let client = http_client()?;
        let issued = json_response(
            versioned(client.get(format!("{}/{QR_PATH}", self.base)))
                .query(&[("bot_type", BOT_TYPE)])
                .send()
                .await
                .map_err(|_| ErrorCode::BotIo)?,
        )
        .await?;
        let qrcode = issued["qrcode"].as_str().ok_or(ErrorCode::BotIo)?;
        let url = scan_link(
            issued["qrcode_img_content"]
                .as_str()
                .ok_or(ErrorCode::BotIo)?,
        )?;
        let flow = Flow {
            qrcode: SecretString::from(qrcode.to_owned()),
            base: self.base.clone(),
        };
        let scan = Scan::new(client, flow, POLL_INTERVAL, EXPIRY_SECONDS, false);
        let id = scan.id;
        self.scans.insert(request.workspace_id, scan);
        Ok(json!({"scanId":id,"url":url,"expiresInSeconds":EXPIRY_SECONDS}))
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
            match poll(&scan).await {
                Poll::Waiting => return Ok(waiting()),
                Poll::Redirect(base) => {
                    scan.flow.base = base;
                    return Ok(waiting());
                }
                Poll::Failed(reason) => return Ok(scan.finish(failed(reason))),
                Poll::Approved(approved) => {
                    let identity = format!("weixin:{}", approved.account_id);
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
        let config = ChannelConfig::Weixin {
            account_id: approved.account_id.clone(),
            base_url: approved.base_url.clone(),
            token_ref: stored_credential_reference(scan.channel_id),
            allowed_senders: vec![approved.scanner.clone()],
        };
        let token = approved.token.clone();
        Ok(scan
            .save_approved(bots, request.workspace_id, config, token)
            .await)
    }

    /// Stop a scan so a later approval creates no channel; a finished channel is unaffected.
    pub(crate) async fn cancel(&self, request: ScanRequest) -> Result<Value, ErrorCode> {
        Ok(self.scans.cancel(&request).await)
    }
}

async fn poll(scan: &Scan<Flow, Approved>) -> Poll {
    let request = versioned(scan.client.get(format!("{}/{STATUS_PATH}", scan.flow.base)))
        .query(&[("qrcode", expose(&scan.flow.qrcode))])
        .timeout(STATUS_TIMEOUT);
    let Ok(response) = request.send().await else {
        return Poll::Waiting;
    };
    json_response(response)
        .await
        .map_or(Poll::Waiting, |status| classify(&status))
}

fn classify(status: &Value) -> Poll {
    match status["status"].as_str() {
        Some("confirmed") => confirmed(status),
        Some("expired") => Poll::Failed(FAILED_EXPIRED),
        Some("scaned_but_redirect") => status["redirect_host"]
            .as_str()
            .map(|host| format!("https://{host}"))
            .filter(|base| trusted_base(base))
            .map_or(Poll::Waiting, Poll::Redirect),
        _ => Poll::Waiting,
    }
}

fn confirmed(status: &Value) -> Poll {
    let field = |key: &str| status[key].as_str().filter(|value| !value.is_empty());
    let base_url = field("baseurl").map_or(API, |base| base.trim_end_matches('/'));
    match (
        field("ilink_bot_id"),
        field("bot_token"),
        field("ilink_user_id"),
    ) {
        (Some(account_id), Some(token), Some(scanner)) if trusted_base(base_url) => {
            Poll::Approved(Approved {
                account_id: account_id.to_owned(),
                token: SecretString::from(token.to_owned()),
                base_url: base_url.to_owned(),
                scanner: scanner.to_owned(),
            })
        }
        _ => Poll::Failed(FAILED_UNAVAILABLE),
    }
}

/// The link to show as a QR code, refusing one that leaves `WeChat`.
fn scan_link(link: &str) -> Result<String, ErrorCode> {
    let url = url::Url::parse(link).map_err(|_| ErrorCode::BotIo)?;
    if url.scheme() != "https" || !url.host_str().is_some_and(trusted_host) {
        return Err(ErrorCode::BotIo);
    }
    Ok(url.into())
}

#[cfg(test)]
mod tests;
