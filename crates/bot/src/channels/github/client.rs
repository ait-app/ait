//! GitHub REST calls of a review channel: installation tokens, pull requests and reviews.
//!
//! A call GitHub refused before acting on it (a rate limit, a revoked token, no connection)
//! is repeated, because repeating it cannot post twice; any other failure ends the delivery.

use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use domain::bot::{GithubReviewContext, TurnContext, TurnStatus};
use model::ErrorCode;
use reqwest::{Response, StatusCode};
use ring::rand::SystemRandom;
use ring::signature::{RSA_PKCS1_SHA256, RsaKeyPair};
use secrecy::{ExposeSecret, SecretString};
use serde_json::{Value, json};

use super::{fallback_body, report, review_comment, short_sha};
use crate::channels::json_response;

const API: &str = "https://api.github.com";
const API_VERSION: &str = "2022-11-28";
const USER_AGENT: &str = "ait-github-review";
/// Installation tokens last an hour; one is reused until shortly before that.
const TOKEN_LIFETIME: Duration = Duration::from_mins(55);
/// Attempts of one call GitHub keeps refusing before acting on it.
const ATTEMPTS: u32 = 5;
/// Longest wait between attempts, whatever GitHub asks for.
const MAX_WAIT: Duration = Duration::from_secs(60);
/// Wait after a refusal that names no time, as GitHub advises for secondary rate limits.
const DEFAULT_WAIT: Duration = Duration::from_secs(60);
/// Wait before reconnecting after no connection could be made.
const CONNECT_WAIT: Duration = Duration::from_secs(5);

/// Why a GitHub call failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Failure {
    /// GitHub refused before acting, so the call may be repeated after this wait.
    Later(Duration),
    /// Anything else, including a failure whose effect on GitHub is unknown.
    Final,
}

impl From<ErrorCode> for Failure {
    fn from(_: ErrorCode) -> Self {
        Self::Final
    }
}

impl Failure {
    /// The error a Bot operation reports: a refusal to retry later is a full backlog.
    pub(super) const fn code(self) -> ErrorCode {
        match self {
            Self::Later(_) => ErrorCode::ResourceExhausted,
            Self::Final => ErrorCode::BotIo,
        }
    }
}

/// An installation token and when it was issued.
type CachedToken = (String, SecretString, Instant);

#[derive(Clone)]
pub(super) struct GithubClient {
    client: reqwest::Client,
    api: String,
    token: Arc<Mutex<Option<CachedToken>>>,
}

impl GithubClient {
    pub(super) fn new(client: reqwest::Client) -> Self {
        Self::at(client, API)
    }

    fn at(client: reqwest::Client, api: &str) -> Self {
        Self {
            client,
            api: api.trim_end_matches('/').to_owned(),
            token: Arc::default(),
        }
    }

    /// Run `call` again while GitHub refuses it before acting, up to [`ATTEMPTS`] times.
    /// # Errors
    /// The last failure once it is final or the attempts are used up.
    pub(super) async fn retrying<T, F, Fut>(mut call: F) -> Result<T, Failure>
    where
        F: FnMut() -> Fut,
        Fut: Future<Output = Result<T, Failure>>,
    {
        let mut attempt = 1;
        loop {
            match call().await {
                Err(Failure::Later(wait)) if attempt < ATTEMPTS => {
                    tokio::time::sleep(wait.min(MAX_WAIT)).await;
                    attempt += 1;
                }
                result => return result,
            }
        }
    }

    /// The pull request `number` of `repository`, as GitHub reports it now.
    /// # Errors
    /// [`Failure::Later`] when GitHub refused before answering.
    pub(super) async fn pull(
        &self,
        app_id: &str,
        installation_id: &str,
        private_key: &SecretString,
        repository: &str,
        number: u64,
    ) -> Result<Value, Failure> {
        let token = self
            .installation_token(app_id, installation_id, private_key)
            .await?;
        let url = format!("{}/repos/{repository}/pulls/{number}", self.api);
        let response = self
            .send(self.client.get(url).bearer_auth(token.expose_secret()))
            .await?;
        if response.status().is_success() {
            return Ok(json_response(response).await?);
        }
        Err(self.refused(&response))
    }

    /// Publish the outcome of a review `turn` on its pull request.
    /// # Errors
    /// [`Failure::Later`] when nothing was posted and the delivery may be repeated.
    pub(super) async fn submit(
        &self,
        app_id: &str,
        installation_id: &str,
        private_key: &SecretString,
        turn: &domain::bot::Turn,
    ) -> Result<(), Failure> {
        let Some(TurnContext::GithubReview(context)) = turn.context.as_ref() else {
            return Err(Failure::Final);
        };
        let token = self
            .installation_token(app_id, installation_id, private_key)
            .await?;
        if let Some(body) = outcome_note(turn, context) {
            return self.post_comment(&token, context, &body).await;
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
        // 422 means GitHub refused the review as given; usually an inline position it no
        // longer accepts. The findings then move into the body, and last into a comment.
        let fallback = fallback_body(&report, context);
        if self.post_review(&token, context, &body, &comments).await? {
            return Ok(());
        }
        if !comments.is_empty() && self.post_review(&token, context, &fallback, &[]).await? {
            return Ok(());
        }
        self.post_comment(&token, context, &fallback).await
    }

    /// Post a `COMMENT` review; `Ok(false)` when GitHub refused it as invalid (422).
    async fn post_review(
        &self,
        token: &SecretString,
        context: &GithubReviewContext,
        body: &str,
        comments: &[Value],
    ) -> Result<bool, Failure> {
        let url = format!(
            "{}/repos/{}/pulls/{}/reviews",
            self.api, context.repository, context.pull_number
        );
        let review = json!({
            "commit_id":context.head_sha,
            "body":body,
            "event":"COMMENT",
            "comments":comments
        });
        let response = self
            .send(
                self.client
                    .post(url)
                    .bearer_auth(token.expose_secret())
                    .json(&review),
            )
            .await?;
        if response.status().is_success() {
            let _ = json_response(response).await?;
            return Ok(true);
        }
        if response.status() == StatusCode::UNPROCESSABLE_ENTITY {
            return Ok(false);
        }
        Err(self.refused(&response))
    }

    async fn post_comment(
        &self,
        token: &SecretString,
        context: &GithubReviewContext,
        body: &str,
    ) -> Result<(), Failure> {
        let url = format!(
            "{}/repos/{}/issues/{}/comments",
            self.api, context.repository, context.pull_number
        );
        let response = self
            .send(
                self.client
                    .post(url)
                    .bearer_auth(token.expose_secret())
                    .json(&json!({"body":body})),
            )
            .await?;
        if response.status().is_success() {
            let _ = json_response(response).await?;
            return Ok(());
        }
        Err(self.refused(&response))
    }

    /// A token of `installation_id`, reused while it is fresh.
    async fn installation_token(
        &self,
        app_id: &str,
        installation_id: &str,
        private_key: &SecretString,
    ) -> Result<SecretString, Failure> {
        if let Some(token) = self.cached(installation_id) {
            return Ok(token);
        }
        let jwt = app_jwt(app_id, private_key)?;
        let url = format!(
            "{}/app/installations/{installation_id}/access_tokens",
            self.api
        );
        let response = self.send(self.client.post(url).bearer_auth(jwt)).await?;
        if !response.status().is_success() {
            // A refused App JWT is a configuration error, not a stale token.
            return Err(rate_limited(&response).map_or(Failure::Final, Failure::Later));
        }
        let value = json_response(response).await?;
        let token = value["token"]
            .as_str()
            .filter(|token| !token.is_empty() && token.len() <= 4096)
            .map(|token| SecretString::from(token.to_owned()))
            .ok_or(Failure::Final)?;
        *self.lock() = Some((installation_id.to_owned(), token.clone(), Instant::now()));
        Ok(token)
    }

    fn cached(&self, installation_id: &str) -> Option<SecretString> {
        self.lock()
            .as_ref()
            .filter(|(installation, _, issued)| {
                installation == installation_id && issued.elapsed() < TOKEN_LIFETIME
            })
            .map(|(_, token, _)| token.clone())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<CachedToken>> {
        self.token
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Why GitHub answered `response` with an error; a rejected token is dropped first.
    fn refused(&self, response: &Response) -> Failure {
        if response.status() == StatusCode::UNAUTHORIZED {
            // The installation's token was revoked or the App reinstalled: mint a new one.
            *self.lock() = None;
            return Failure::Later(Duration::ZERO);
        }
        rate_limited(response).map_or(Failure::Final, Failure::Later)
    }

    async fn send(&self, request: reqwest::RequestBuilder) -> Result<Response, Failure> {
        request
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", API_VERSION)
            .header("User-Agent", USER_AGENT)
            .send()
            .await
            .map_err(|error| {
                if error.is_connect() {
                    // The request never reached GitHub.
                    Failure::Later(CONNECT_WAIT)
                } else {
                    Failure::Final
                }
            })
    }
}

/// The comment for a review that produced no report, or `None` when it completed.
fn outcome_note(turn: &domain::bot::Turn, context: &GithubReviewContext) -> Option<String> {
    let at = format!(
        "`{}` at `{}`",
        context.repository,
        short_sha(&context.head_sha)
    );
    match turn.status {
        TurnStatus::Failed => {
            let reason = turn
                .error
                .as_deref()
                .unwrap_or("The review could not be completed");
            Some(format!("GitHub review could not run for {at}: {reason}"))
        }
        // The same head is reviewed once, so only a new commit asks again.
        TurnStatus::Uncertain => Some(format!(
            "GitHub review for {at} did not finish: its outcome could not be confirmed. \
             Push a new commit to request another review."
        )),
        _ => None,
    }
}

/// How long GitHub asks a rate-limited `response` to wait, or `None` when it is not one.
fn rate_limited(response: &Response) -> Option<Duration> {
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.trim().parse::<u64>().ok())
    };
    let status = response.status();
    let exhausted = header("x-ratelimit-remaining") == Some(0);
    let retry_after = header("retry-after");
    if status != StatusCode::TOO_MANY_REQUESTS
        && !(status == StatusCode::FORBIDDEN && (exhausted || retry_after.is_some()))
    {
        return None;
    }
    if let Some(seconds) = retry_after {
        return Some(Duration::from_secs(seconds));
    }
    let reset = header("x-ratelimit-reset")
        .filter(|_| exhausted)
        .and_then(|reset| reset.checked_sub(chrono::Utc::now().timestamp().try_into().ok()?));
    Some(reset.map_or(DEFAULT_WAIT, Duration::from_secs))
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

pub(super) fn pem(value: &str) -> Result<(&'static str, Vec<u8>), ErrorCode> {
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

#[cfg(test)]
mod tests;
