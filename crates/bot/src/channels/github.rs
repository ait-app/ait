//! GitHub App webhook admission and pull-request review delivery.

use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use domain::ChannelId;
use domain::bot::{
    Channel, ChannelConfig, GithubRepositoryBinding, GithubReviewContext, GithubReviewTrigger,
    TurnContext, TurnStatus,
};
use hmac::{Hmac, Mac};
use model::ErrorCode;
use ring::rand::SystemRandom;
use ring::signature::{RSA_PKCS1_SHA256, RsaKeyPair};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

use super::{Incoming, deliver, json_response};
use crate::ports::Credentials;
use crate::service::Bots;

const API: &str = "https://api.github.com";
const API_VERSION: &str = "2022-11-28";
const USER_AGENT: &str = "ait-github-review";
const MAX_MANDATE_BYTES: usize = 4096;

pub(super) async fn webhook(
    channel: &Channel,
    credentials: &Arc<dyn Credentials>,
    client: &reqwest::Client,
    event: &str,
    delivery_id: &str,
    signature: &str,
    body: &[u8],
) -> Result<Option<Incoming>, ErrorCode> {
    let ChannelConfig::Github {
        app_id,
        private_key_ref,
        webhook_secret_ref,
        installation_id,
        mention,
        repositories,
    } = &channel.config
    else {
        return Err(ErrorCode::InvalidMessage);
    };
    let webhook_secret = credentials.resolve(webhook_secret_ref)?;
    verify_signature(&webhook_secret, body, signature)?;
    if !model::valid_id(delivery_id) {
        return Err(ErrorCode::InvalidMessage);
    }
    let payload: Value = serde_json::from_slice(body).map_err(|_| ErrorCode::InvalidMessage)?;
    if event == "ping" {
        return Ok(None);
    }
    if !matches!(event, "issue_comment" | "issues" | "pull_request") {
        return Ok(None);
    }
    if payload["installation"]["id"]
        .as_u64()
        .map(|id| id.to_string())
        != Some(installation_id.clone())
    {
        return Ok(None);
    }
    let repository = required_text(&payload["repository"], "full_name", 201)?;
    let Some(binding) = repositories
        .iter()
        .find(|binding| binding.full_name.eq_ignore_ascii_case(repository))
    else {
        return Ok(None);
    };
    let sender = required_text(&payload["sender"], "login", 100)?;
    if sender.eq_ignore_ascii_case(mention)
        || sender.eq_ignore_ascii_case(&format!("{mention}[bot]"))
        || !allowed(binding, sender)
    {
        return Ok(None);
    }
    let trigger = trigger(event, &payload, mention)?;
    let Some((trigger, mandate, comment_id)) = trigger else {
        return Ok(None);
    };
    let pull_number = payload["issue"]["number"]
        .as_u64()
        .or_else(|| payload["pull_request"]["number"].as_u64())
        .filter(|number| *number > 0)
        .ok_or(ErrorCode::InvalidMessage)?;
    let pull = if event == "pull_request" {
        payload["pull_request"].clone()
    } else {
        let key = credentials.resolve(private_key_ref)?;
        GithubClient::new(client.clone())
            .pull(app_id, installation_id, &key, repository, pull_number)
            .await?
    };
    let context = review_context(
        binding,
        repository,
        pull_number,
        trigger,
        sender,
        comment_id,
        &pull,
    )?;
    let repository_id = payload["repository"]["id"]
        .as_u64()
        .ok_or(ErrorCode::InvalidMessage)?;
    Ok(Some(incoming(channel.id, repository_id, context, &mandate)))
}

pub(super) async fn run(
    bots: Bots,
    channel: Channel,
    credentials: Arc<dyn Credentials>,
    client: reqwest::Client,
    cancel: &CancellationToken,
) -> Result<(), ErrorCode> {
    let ChannelConfig::Github {
        app_id,
        private_key_ref,
        installation_id,
        ..
    } = &channel.config
    else {
        return Err(ErrorCode::InvalidMessage);
    };
    bots.account(channel.id, format!("github:{installation_id}"))
        .await?;
    bots.channel_status(channel.id, json!({"state":"connected"}))
        .await;
    let private_key = credentials.resolve(private_key_ref)?;
    let app_id = app_id.clone();
    let installation_id = installation_id.clone();
    let github = GithubClient::new(client);
    deliver(&bots, channel.id, cancel, |turn, _conversation| {
        let github = github.clone();
        let private_key = private_key.clone();
        let bots = bots.clone();
        let app_id = app_id.clone();
        let installation_id = installation_id.clone();
        async move {
            let workspace = review_workspace(&turn)?;
            github
                .submit(&app_id, &installation_id, &private_key, &turn)
                .await?;
            let _ = bots.archive_review(workspace).await;
            Ok(())
        }
    })
    .await
}

fn trigger(
    event: &str,
    payload: &Value,
    mention: &str,
) -> Result<Option<(GithubReviewTrigger, String, Option<String>)>, ErrorCode> {
    match event {
        "issue_comment"
            if payload["action"] == "created" && payload["issue"]["pull_request"].is_object() =>
        {
            let body = required_text(&payload["comment"], "body", 256 * 1024)?;
            let Some(mandate) = review_command(body, mention) else {
                return Ok(None);
            };
            let comment = payload["comment"]["id"]
                .as_u64()
                .ok_or(ErrorCode::InvalidMessage)?
                .to_string();
            Ok(Some((GithubReviewTrigger::Mention, mandate, Some(comment))))
        }
        "pull_request" if payload["action"] == "review_requested" => {
            let Some(reviewer) = payload["requested_reviewer"]["login"].as_str() else {
                return Ok(None);
            };
            if !bot_login(reviewer, mention) {
                return Ok(None);
            }
            Ok(Some((
                GithubReviewTrigger::ReviewerAssignment,
                String::new(),
                None,
            )))
        }
        "issues"
            if payload["action"] == "assigned" && payload["issue"]["pull_request"].is_object() =>
        {
            let Some(assignee) = payload["assignee"]["login"].as_str() else {
                return Ok(None);
            };
            if !bot_login(assignee, mention) {
                return Ok(None);
            }
            Ok(Some((
                GithubReviewTrigger::AssigneeAssignment,
                String::new(),
                None,
            )))
        }
        _ => Ok(None),
    }
}

fn bot_login(login: &str, mention: &str) -> bool {
    login.eq_ignore_ascii_case(mention) || login.eq_ignore_ascii_case(&format!("{mention}[bot]"))
}

fn review_command(body: &str, mention: &str) -> Option<String> {
    let expected = format!("@{mention}");
    let words = body.split_whitespace().collect::<Vec<_>>();
    let index = words
        .iter()
        .position(|word| word.eq_ignore_ascii_case(&expected))?;
    if !words
        .get(index + 1)
        .is_some_and(|word| word.eq_ignore_ascii_case("review"))
    {
        return None;
    }
    let mandate = words.get(index + 2..).unwrap_or_default().join(" ");
    let end = mandate.floor_char_boundary(mandate.len().min(MAX_MANDATE_BYTES));
    Some(mandate[..end].to_owned())
}

fn review_context(
    binding: &GithubRepositoryBinding,
    repository: &str,
    pull_number: u64,
    trigger: GithubReviewTrigger,
    sender: &str,
    comment_id: Option<String>,
    pull: &Value,
) -> Result<GithubReviewContext, ErrorCode> {
    let head_sha = object_name(required_text(&pull["head"], "sha", 64)?)?;
    let base_sha = object_name(required_text(&pull["base"], "sha", 64)?)?;
    Ok(GithubReviewContext {
        repository: repository.to_ascii_lowercase(),
        project_id: binding.project_id.clone(),
        pull_number,
        title: required_text(pull, "title", 1024)?.to_owned(),
        url: required_text(pull, "html_url", 2048)?.to_owned(),
        base_ref: required_text(&pull["base"], "ref", 255)?.to_owned(),
        head_ref: required_text(&pull["head"], "ref", 255)?.to_owned(),
        base_sha,
        head_sha,
        trigger,
        trigger_user: sender.to_owned(),
        trigger_comment_id: comment_id,
        workspace_id: domain::workspace::registry::generate_workspace_id()
            .map_err(|_| ErrorCode::RegistryIo)?,
    })
}

fn incoming(
    channel_id: ChannelId,
    repository_id: u64,
    context: GithubReviewContext,
    mandate: &str,
) -> Incoming {
    let identity = format!(
        "{}:{}:{}",
        context.repository, context.pull_number, context.head_sha
    );
    let message_id = format!("github-review:{:x}", Sha256::digest(identity.as_bytes()));
    let mandate = if mandate.is_empty() {
        "Apply the Bot's default review mandate.".to_owned()
    } else {
        format!("Additional review mandate from the authorized requester: {mandate}")
    };
    let text = format!(
        "Review GitHub pull request #{number} in {repository}. The admitted comparison is exactly {base}...{head}. Inspect the complete diff and its callers in this checkout. Do not modify files, push, merge, approve, or request changes. Report only actionable defects introduced by this diff. Return JSON only as {{\"summary\":\"...\",\"findings\":[{{\"path\":\"...\",\"line\":1,\"side\":\"RIGHT\",\"severity\":\"high\",\"title\":\"...\",\"body\":\"...\"}}]}}. Severity must be high, medium, or low; use LEFT only for removed lines and RIGHT for added or context lines. {mandate}",
        number = context.pull_number,
        repository = context.repository,
        base = context.base_sha,
        head = context.head_sha,
    );
    Incoming {
        channel_id,
        peer: format!("repo:{repository_id}:pr:{}", context.pull_number),
        sender: "github-review".to_owned(),
        thread: Some(context.head_sha.clone()),
        message_id,
        text,
        reply_to: context.trigger_comment_id.clone(),
        context: Some(TurnContext::GithubReview(context)),
    }
}

fn allowed(binding: &GithubRepositoryBinding, sender: &str) -> bool {
    binding
        .allowed_senders
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(sender))
}

fn required_text<'a>(value: &'a Value, field: &str, limit: usize) -> Result<&'a str, ErrorCode> {
    value[field]
        .as_str()
        .filter(|text| !text.is_empty() && text.len() <= limit && !text.contains('\0'))
        .ok_or(ErrorCode::InvalidMessage)
}

fn object_name(value: &str) -> Result<String, ErrorCode> {
    if matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(value.to_ascii_lowercase())
    } else {
        Err(ErrorCode::InvalidMessage)
    }
}

fn verify_signature(secret: &SecretString, body: &[u8], signature: &str) -> Result<(), ErrorCode> {
    let provided = signature
        .strip_prefix("sha256=")
        .and_then(decode_hex)
        .ok_or(ErrorCode::InvalidMessage)?;
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.expose_secret().as_bytes())
        .map_err(|_| ErrorCode::BotIo)?;
    mac.update(body);
    mac.verify_slice(&provided)
        .map_err(|_| ErrorCode::InvalidMessage)
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if value.len() != 64 {
        return None;
    }
    value
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let high = hex_nibble(pair[0])?;
            let low = hex_nibble(pair[1])?;
            Some((high << 4) | low)
        })
        .collect()
}

fn hex_nibble(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        b'A'..=b'F' => Some(value - b'A' + 10),
        _ => None,
    }
}

#[derive(Clone)]
struct GithubClient {
    client: reqwest::Client,
    api: &'static str,
}

impl GithubClient {
    fn new(client: reqwest::Client) -> Self {
        Self { client, api: API }
    }

    async fn pull(
        &self,
        app_id: &str,
        installation_id: &str,
        private_key: &SecretString,
        repository: &str,
        number: u64,
    ) -> Result<Value, ErrorCode> {
        let token = self
            .installation_token(app_id, installation_id, private_key)
            .await?;
        let response = self
            .request(
                self.client
                    .get(format!("{}/repos/{repository}/pulls/{number}", self.api))
                    .bearer_auth(token.expose_secret()),
            )
            .await?;
        if response.status().is_success() {
            json_response(response).await
        } else {
            Err(ErrorCode::BotIo)
        }
    }

    async fn submit(
        &self,
        app_id: &str,
        installation_id: &str,
        private_key: &SecretString,
        turn: &domain::bot::Turn,
    ) -> Result<(), ErrorCode> {
        let Some(TurnContext::GithubReview(context)) = turn.context.as_ref() else {
            return Err(ErrorCode::BotIo);
        };
        let token = self
            .installation_token(app_id, installation_id, private_key)
            .await?;
        if turn.status == TurnStatus::Failed {
            let reason = turn
                .error
                .as_deref()
                .unwrap_or("The review could not be completed");
            return self
                .post_comment(
                    &token,
                    context,
                    &format!(
                        "GitHub review could not run for `{}` at `{}`: {reason}",
                        context.repository,
                        short_sha(&context.head_sha)
                    ),
                )
                .await;
        }
        let report = report(turn)?;
        let comments = report
            .findings
            .iter()
            .filter_map(review_comment)
            .collect::<Vec<_>>();
        let body = format!(
            "{}\n\nReviewed `{}` at `{}`.",
            report.summary,
            context.repository,
            short_sha(&context.head_sha)
        );
        let response = self
            .request(
                self.client
                    .post(format!(
                        "{}/repos/{}/pulls/{}/reviews",
                        self.api, context.repository, context.pull_number
                    ))
                    .bearer_auth(token.expose_secret())
                    .json(&json!({
                        "commit_id":context.head_sha,
                        "body":body,
                        "event":"COMMENT",
                        "comments":comments
                    })),
            )
            .await?;
        if response.status().is_success() {
            let _ = json_response(response).await?;
            return Ok(());
        }
        if response.status().as_u16() != 422 {
            return Err(ErrorCode::BotIo);
        }
        let fallback = fallback_body(&report, context);
        self.post_comment(&token, context, &fallback).await
    }

    async fn post_comment(
        &self,
        token: &SecretString,
        context: &GithubReviewContext,
        body: &str,
    ) -> Result<(), ErrorCode> {
        let response = self
            .request(
                self.client
                    .post(format!(
                        "{}/repos/{}/issues/{}/comments",
                        self.api, context.repository, context.pull_number
                    ))
                    .bearer_auth(token.expose_secret())
                    .json(&json!({"body":body})),
            )
            .await?;
        if response.status().is_success() {
            let _ = json_response(response).await?;
            Ok(())
        } else {
            Err(ErrorCode::BotIo)
        }
    }

    async fn installation_token(
        &self,
        app_id: &str,
        installation_id: &str,
        private_key: &SecretString,
    ) -> Result<SecretString, ErrorCode> {
        let jwt = app_jwt(app_id, private_key)?;
        let response = self
            .request(
                self.client
                    .post(format!(
                        "{}/app/installations/{installation_id}/access_tokens",
                        self.api
                    ))
                    .bearer_auth(jwt),
            )
            .await?;
        if !response.status().is_success() {
            return Err(ErrorCode::BotIo);
        }
        let value = json_response(response).await?;
        value["token"]
            .as_str()
            .filter(|token| !token.is_empty() && token.len() <= 4096)
            .map(|token| SecretString::from(token.to_owned()))
            .ok_or(ErrorCode::BotIo)
    }

    async fn request(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<reqwest::Response, ErrorCode> {
        request
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", API_VERSION)
            .header("User-Agent", USER_AGENT)
            .send()
            .await
            .map_err(|_| ErrorCode::BotIo)
    }
}

fn app_jwt(app_id: &str, private_key: &SecretString) -> Result<String, ErrorCode> {
    let now = chrono::Utc::now().timestamp();
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","typ":"JWT"}"#);
    let claims = URL_SAFE_NO_PAD.encode(
        serde_json::to_vec(&json!({"iat":now - 60,"exp":now + 540,"iss":app_id}))
            .map_err(|_| ErrorCode::BotIo)?,
    );
    let message = format!("{header}.{claims}");
    let (label, der) = pem(private_key.expose_secret())?;
    let key = if label == "RSA PRIVATE KEY" {
        RsaKeyPair::from_der(&der)
    } else {
        RsaKeyPair::from_pkcs8(&der)
    }
    .map_err(|_| ErrorCode::BotIo)?;
    let mut signature = vec![0; key.public().modulus_len()];
    key.sign(
        &RSA_PKCS1_SHA256,
        &SystemRandom::new(),
        message.as_bytes(),
        &mut signature,
    )
    .map_err(|_| ErrorCode::BotIo)?;
    Ok(format!("{message}.{}", URL_SAFE_NO_PAD.encode(signature)))
}

fn pem(value: &str) -> Result<(&'static str, Vec<u8>), ErrorCode> {
    let normalized = value.replace("\\n", "\n");
    let normalized = normalized.trim();
    let (label, begin, end) = if normalized.contains("-----BEGIN RSA PRIVATE KEY-----") {
        (
            "RSA PRIVATE KEY",
            "-----BEGIN RSA PRIVATE KEY-----",
            "-----END RSA PRIVATE KEY-----",
        )
    } else {
        (
            "PRIVATE KEY",
            "-----BEGIN PRIVATE KEY-----",
            "-----END PRIVATE KEY-----",
        )
    };
    let body = normalized
        .strip_prefix(begin)
        .and_then(|value| value.strip_suffix(end))
        .ok_or(ErrorCode::BotIo)?;
    let encoded = body.lines().map(str::trim).collect::<String>();
    STANDARD
        .decode(encoded)
        .map(|der| (label, der))
        .map_err(|_| ErrorCode::BotIo)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewReport {
    summary: String,
    findings: Vec<ReviewFinding>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewFinding {
    path: String,
    line: u64,
    side: String,
    severity: String,
    title: String,
    body: String,
}

fn report(turn: &domain::bot::Turn) -> Result<ReviewReport, ErrorCode> {
    let reply = turn.reply.as_deref().ok_or(ErrorCode::BotIo)?;
    let report = parse_report(reply).unwrap_or_else(|| ReviewReport {
        summary: reply.to_owned(),
        findings: Vec::new(),
    });
    if report.summary.trim().is_empty()
        || report.summary.len() > 60 * 1024
        || report.findings.len() > 100
    {
        return Err(ErrorCode::BotIo);
    }
    Ok(report)
}

fn parse_report(reply: &str) -> Option<ReviewReport> {
    serde_json::from_str(reply).ok().or_else(|| {
        let fenced = reply
            .trim()
            .strip_prefix("```json")
            .or_else(|| reply.trim().strip_prefix("```"))?
            .strip_suffix("```")?
            .trim();
        serde_json::from_str(fenced).ok()
    })
}

fn review_comment(finding: &ReviewFinding) -> Option<Value> {
    if finding.path.is_empty()
        || finding.path.len() > 500
        || finding.path.starts_with('/')
        || finding.path.split('/').any(|part| part == "..")
        || finding.path.chars().any(char::is_control)
        || finding.line == 0
        || !matches!(finding.side.as_str(), "LEFT" | "RIGHT")
        || !matches!(finding.severity.as_str(), "high" | "medium" | "low")
        || finding.title.trim().is_empty()
        || finding.title.len() > 500
        || finding.body.trim().is_empty()
        || finding.body.len() > 16 * 1024
    {
        return None;
    }
    Some(json!({
        "path":finding.path,
        "line":finding.line,
        "side":finding.side,
        "body":format!("**{}: {}**\n\n{}", finding.severity, finding.title, finding.body)
    }))
}

fn fallback_body(report: &ReviewReport, context: &GithubReviewContext) -> String {
    let mut body = format!(
        "{}\n\nReviewed `{}` at `{}`.",
        report.summary,
        context.repository,
        short_sha(&context.head_sha)
    );
    for finding in report.findings.iter().take(100) {
        let entry = format!(
            "\n\n- **{}: {}** — `{}` line {}: {}",
            finding.severity, finding.title, finding.path, finding.line, finding.body
        );
        if body.len() + entry.len() > 60 * 1024 {
            break;
        }
        body.push_str(&entry);
    }
    body
}

fn short_sha(sha: &str) -> &str {
    sha.get(..12).unwrap_or(sha)
}

fn review_workspace(turn: &domain::bot::Turn) -> Result<String, ErrorCode> {
    match turn.context.as_ref() {
        Some(TurnContext::GithubReview(context)) => Ok(context.workspace_id.clone()),
        None => Err(ErrorCode::BotIo),
    }
}

#[cfg(test)]
mod tests;
