use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use host_link::{
    Binding, CENTER, Center, Credential, Error, Machine, Session, Tokens, VERIFICATION_URI,
};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub(crate) struct HttpCenter {
    client: reqwest::Client,
    base: String,
}

#[derive(Debug)]
pub(crate) struct Authorization {
    pub device_code: SecretString,
    pub user_code: String,
    pub expires_in: u64,
    pub interval: u64,
}

#[derive(Deserialize)]
struct TokenResponse {
    token_type: String,
    access_token: SecretString,
    access_expires_at: DateTime<Utc>,
    refresh_token: SecretString,
    refresh_expires_at: DateTime<Utc>,
    node_id: Uuid,
    host_id: Uuid,
    server_id: Uuid,
    grant_id: Uuid,
}

impl HttpCenter {
    pub fn new() -> Result<Self, Error> {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(10))
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(|_| Error::Unavailable)?;
        Ok(Self {
            client,
            base: CENTER.to_owned(),
        })
    }

    #[cfg(test)]
    pub(super) fn test_center(base: String) -> Self {
        let mut center = Self::new().expect("build test HTTP client");
        center.base = base;
        center
    }

    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        bearer: Option<&SecretString>,
        body: Value,
    ) -> Result<Value, Error> {
        let mut request = self
            .client
            .request(method, format!("{}{path}", self.base))
            .json(&body);
        if let Some(token) = bearer {
            request = request.bearer_auth(token.expose_secret());
        }
        let response = request.send().await.map_err(|_| Error::Unavailable)?;
        let status = response.status();
        if status.is_redirection() {
            return Err(Error::Protocol);
        }
        if status.is_server_error() || status.as_u16() == 429 {
            return Err(Error::Unavailable);
        }
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| Error::Unavailable)?;
            if bytes.len() + chunk.len() > 64 * 1024 {
                return Err(Error::Protocol);
            }
            bytes.extend_from_slice(&chunk);
        }
        let value: Value = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).map_err(|_| Error::Protocol)?
        };
        if status.is_success() {
            return Ok(value);
        }
        match value["error"]["code"].as_str() {
            Some("authorization_pending") => Err(Error::Pending),
            Some("slow_down") => Err(Error::SlowDown),
            Some("access_denied" | "expired_token") => Err(Error::Denied),
            Some("node_session_expired" | "registration_expired") => Err(Error::SessionExpired),
            Some("refresh_reuse") => Err(Error::Unauthorized),
            _ if status.as_u16() == 401 || status.as_u16() == 403 => Err(Error::Unauthorized),
            _ if status.as_u16() == 409 => Err(Error::Conflict),
            _ if status.as_u16() == 404 || status.as_u16() == 410 => Err(Error::SessionExpired),
            _ => Err(Error::Protocol),
        }
    }

    pub async fn authorize(&self, machine: &Machine) -> Result<Authorization, Error> {
        let value = self
            .request(
                reqwest::Method::POST,
                "/v1/auth/device/authorize",
                None,
                json!({"client_id":"ait-linux-daemon", "machine":machine}),
            )
            .await?;
        let code = value["device_code"].as_str().ok_or(Error::Protocol)?;
        let user = value["user_code"].as_str().ok_or(Error::Protocol)?;
        let expires = value["expires_in"].as_u64().ok_or(Error::Protocol)?;
        let interval = value["interval"].as_u64().ok_or(Error::Protocol)?;
        if value["verification_uri"] != VERIFICATION_URI
            || code.len() != 64
            || !code.bytes().all(|b| b.is_ascii_hexdigit())
            || user.len() != 14
            || !user.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
            || !(1..=600).contains(&expires)
            || !(5..=60).contains(&interval)
        {
            return Err(Error::Protocol);
        }
        Ok(Authorization {
            device_code: code.to_owned().into(),
            user_code: user.to_owned(),
            expires_in: expires,
            interval,
        })
    }

    pub async fn poll(&self, code: &SecretString, request_id: Uuid) -> Result<Tokens, Error> {
        decode_tokens(
            self.request(
                reqwest::Method::POST,
                "/v1/auth/device/token",
                None,
                json!({"device_code":code.expose_secret(), "request_id":request_id}),
            )
            .await?,
        )
    }

    pub async fn enroll(
        &self,
        token: &SecretString,
        request_id: Uuid,
        machine: &Machine,
    ) -> Result<Tokens, Error> {
        decode_tokens(
            self.request(
                reqwest::Method::POST,
                "/v1/auth/device/enroll",
                None,
                json!({"token":token.expose_secret(), "request_id":request_id, "machine":machine}),
            )
            .await?,
        )
    }

    pub async fn revoke(&self, token: &SecretString) -> Result<(), Error> {
        self.request(
            reqwest::Method::POST,
            "/v1/auth/device/revoke",
            None,
            json!({"refresh_token":token.expose_secret()}),
        )
        .await?;
        Ok(())
    }
}

fn decode_tokens(value: Value) -> Result<Tokens, Error> {
    let wire: TokenResponse = serde_json::from_value(value).map_err(|_| Error::Protocol)?;
    let refresh = wire.refresh_token.expose_secret();
    let access = wire.access_token.expose_secret();
    if wire.token_type != "Bearer"
        || access.is_empty()
        || access.len() > 8192
        || !access
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
        || !refresh.starts_with("ait_refresh_")
        || refresh.len() != 76
        || !refresh[12..].bytes().all(|b| b.is_ascii_hexdigit())
        || [wire.node_id, wire.host_id, wire.server_id, wire.grant_id]
            .iter()
            .any(Uuid::is_nil)
        || wire.refresh_expires_at <= Utc::now()
        || wire.access_expires_at > Utc::now() + chrono::Duration::minutes(16)
    {
        return Err(Error::Protocol);
    }
    Ok(Tokens {
        access_token: wire.access_token,
        access_expires_at: wire.access_expires_at,
        credential: Credential {
            refresh_token: wire.refresh_token,
            refresh_expires_at: wire.refresh_expires_at,
            binding: Binding {
                node_id: wire.node_id,
                host_id: wire.host_id,
                server_id: wire.server_id,
                grant_id: wire.grant_id,
            },
        },
    })
}

#[async_trait]
impl Center for HttpCenter {
    async fn refresh(&self, secret: &SecretString, request_id: Uuid) -> Result<Tokens, Error> {
        decode_tokens(
            self.request(
                reqwest::Method::POST,
                "/v1/auth/device/refresh",
                None,
                json!({"refresh_token":secret.expose_secret(), "request_id":request_id}),
            )
            .await?,
        )
    }
    async fn register(
        &self,
        tokens: &Tokens,
        machine: &Machine,
        instance: Uuid,
        registration: Uuid,
    ) -> Result<Session, Error> {
        let value = self.request(reqwest::Method::POST, "/v1/nodes/register", Some(&tokens.access_token), json!({
            "registration_id":registration, "installation_id":machine.server_id,
            "display_name":machine.display_name, "platform":machine.platform, "app_version":machine.app_version,
            "runtime":{"server_id":machine.server_id,"instance_id":instance,"relay_modes":["ait-rust-single-v1"]}
        })).await?;
        let binding = &tokens.credential.binding;
        if value["node_id"] != binding.node_id.to_string()
            || value["host_id"] != binding.host_id.to_string()
            || value["server_id"] != machine.server_id.to_string()
            || value["control_required"] != true
        {
            return Err(Error::Protocol);
        }
        let lease = value["lease_duration_seconds"]
            .as_i64()
            .ok_or(Error::Protocol)?;
        let renew = value["renew_after_seconds"]
            .as_u64()
            .ok_or(Error::Protocol)?;
        if !(10..=120).contains(&lease) || renew == 0 || renew >= lease.unsigned_abs() {
            return Err(Error::Protocol);
        }
        let id: Uuid = serde_json::from_value(value["node_session_id"].clone())
            .map_err(|_| Error::Protocol)?;
        if id.is_nil() {
            return Err(Error::Protocol);
        }
        Ok(Session {
            id,
            lease_until: Utc::now() + chrono::Duration::seconds(lease),
            renew_after_seconds: renew,
        })
    }
    async fn renew(&self, tokens: &Tokens, session: Uuid) -> Result<DateTime<Utc>, Error> {
        let value = self
            .request(
                reqwest::Method::POST,
                &format!("/v1/node-sessions/{session}/renew"),
                Some(&tokens.access_token),
                json!({}),
            )
            .await?;
        let until: DateTime<Utc> =
            serde_json::from_value(value["lease_until"].clone()).map_err(|_| Error::Protocol)?;
        if until <= Utc::now() || until > Utc::now() + chrono::Duration::seconds(120) {
            return Err(Error::Protocol);
        }
        Ok(until)
    }
    async fn ticket(&self, tokens: &Tokens, session: Uuid) -> Result<SecretString, Error> {
        let value = self
            .request(
                reqwest::Method::POST,
                &format!("/v1/node-sessions/{session}/control-tickets"),
                Some(&tokens.access_token),
                json!({}),
            )
            .await?;
        let ticket = value["control_ticket"].as_str().ok_or(Error::Protocol)?;
        if ticket.len() != 64 || !ticket.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::Protocol);
        }
        Ok(ticket.to_owned().into())
    }
    async fn close(&self, tokens: &Tokens, session: Uuid) -> Result<(), Error> {
        self.request(
            reqwest::Method::DELETE,
            &format!("/v1/node-sessions/{session}"),
            Some(&tokens.access_token),
            json!({}),
        )
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
