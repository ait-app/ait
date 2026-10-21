//! Cached ES256 Apple provider tokens and RS256 Google service-account OAuth exchanges.

use anyhow::{Context, ensure};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use metadata::push::ports::DeliveryError;
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use serde_json::json;
use tokio::sync::Mutex;

const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";

struct Cached {
    value: SecretString,
    expires_at: i64,
}

/// Apple signing material and a reusable provider-token cache; never debug-print credentials.
pub(super) struct Apple {
    key: EncodingKey,
    key_id: String,
    team_id: String,
    bundles: Vec<String>,
    cached: Mutex<Option<Cached>>,
}

impl Apple {
    /// Load APNs settings through `environment`; missing configuration disables this provider.
    /// Returns sanitized errors for incomplete settings or unreadable/invalid signing material.
    pub(super) fn from_environment(
        environment: &dyn Fn(&str) -> Option<String>,
    ) -> anyhow::Result<Option<Self>> {
        let key_file = environment("AIT_APNS_KEY_FILE");
        let key_id = environment("AIT_APNS_KEY_ID");
        let team_id = environment("AIT_APNS_TEAM_ID");
        if key_file.is_none() && key_id.is_none() && team_id.is_none() {
            return Ok(None);
        }
        let key_file = key_file.context("AIT_APNS_KEY_FILE is required for APNs")?;
        let key_id = key_id.context("AIT_APNS_KEY_ID is required for APNs")?;
        let team_id = team_id.context("AIT_APNS_TEAM_ID is required for APNs")?;
        ensure!(
            valid_identifier(&key_id) && valid_identifier(&team_id),
            "invalid APNs key or team identifier"
        );
        let pem = SecretString::from(
            std::fs::read_to_string(key_file)
                .map_err(|_| anyhow::anyhow!("cannot read APNs signing key"))?,
        );
        let key = EncodingKey::from_ec_pem(pem.expose_secret().as_bytes())
            .map_err(|_| anyhow::anyhow!("invalid APNs P-256 signing key"))?;
        let bundles = environment("AIT_APNS_BUNDLE_IDS")
            .unwrap_or_else(|| "dev.ait.mobile,dev.ait.mobile.debug".to_owned())
            .split(',')
            .map(str::trim)
            .filter(|bundle| !bundle.is_empty())
            .map(str::to_owned)
            .collect::<Vec<_>>();
        ensure!(
            !bundles.is_empty() && bundles.iter().all(|bundle| valid_identifier(bundle)),
            "invalid APNs bundle allowlist"
        );
        Ok(Some(Self {
            key,
            key_id,
            team_id,
            bundles,
            cached: Mutex::new(None),
        }))
    }

    /// Whether `bundle` belongs to the configured application allowlist.
    pub(super) fn allows(&self, bundle: &str) -> bool {
        self.bundles.iter().any(|allowed| allowed == bundle)
    }

    /// Reuse a current credential or sign a new one; signing failures are safely classified.
    pub(super) async fn authorization(&self) -> Result<SecretString, DeliveryError> {
        let now = chrono::Utc::now().timestamp();
        let mut cached = self.cached.lock().await;
        if let Some(token) = cached.as_ref().filter(|token| token.expires_at > now) {
            return Ok(token.value.clone());
        }
        let mut header = Header::new(Algorithm::ES256);
        header.kid = Some(self.key_id.clone());
        let value = encode(&header, &json!({"iss":self.team_id,"iat":now}), &self.key)
            .map_err(|_| DeliveryError::Rejected)?;
        let value = SecretString::from(value);
        *cached = Some(Cached {
            value: value.clone(),
            expires_at: now + 50 * 60,
        });
        Ok(value)
    }

    /// Clear only `rejected`, preserving credentials refreshed by concurrent requests.
    pub(super) async fn invalidate(&self, rejected: &SecretString) {
        let mut cached = self.cached.lock().await;
        if cached
            .as_ref()
            .is_some_and(|current| current.value.expose_secret() == rejected.expose_secret())
        {
            *cached = None;
        }
    }
}

#[derive(Deserialize)]
struct ServiceAccount {
    project_id: String,
    client_email: String,
    private_key: SecretString,
}

#[derive(Deserialize)]
struct OAuthResponse {
    access_token: SecretString,
    expires_in: u64,
}

/// Google service identity, signing material and cached OAuth access token.
pub(super) struct Google {
    project_id: String,
    email: String,
    key: EncodingKey,
    cached: Mutex<Option<Cached>>,
    token_url: String,
}

impl Google {
    /// Load an optional service account through `environment`; return sanitized credential errors.
    pub(super) fn from_environment(
        environment: &dyn Fn(&str) -> Option<String>,
    ) -> anyhow::Result<Option<Self>> {
        let path = environment("AIT_FCM_SERVICE_ACCOUNT_FILE")
            .or_else(|| environment("GOOGLE_APPLICATION_CREDENTIALS"));
        let Some(path) = path else { return Ok(None) };
        let contents = SecretString::from(
            std::fs::read_to_string(path)
                .map_err(|_| anyhow::anyhow!("cannot read FCM service account"))?,
        );
        let account: ServiceAccount = serde_json::from_str(contents.expose_secret())
            .map_err(|_| anyhow::anyhow!("invalid FCM service account"))?;
        ensure!(
            valid_identifier(&account.project_id) && !account.client_email.is_empty(),
            "invalid FCM service account identity"
        );
        let key = EncodingKey::from_rsa_pem(account.private_key.expose_secret().as_bytes())
            .map_err(|_| anyhow::anyhow!("invalid FCM RSA signing key"))?;
        Ok(Some(Self {
            project_id: account.project_id,
            email: account.client_email,
            key,
            cached: Mutex::new(None),
            token_url: GOOGLE_TOKEN_URL.to_owned(),
        }))
    }

    /// Return the validated Firebase project identifier used in the provider URL.
    pub(super) fn project(&self) -> &str {
        &self.project_id
    }

    /// Reuse a current credential or exchange a signed assertion through `client`.
    /// Returns classified signing, network or OAuth failures without response contents.
    pub(super) async fn authorization(
        &self,
        client: &reqwest::Client,
    ) -> Result<SecretString, DeliveryError> {
        let now = chrono::Utc::now().timestamp();
        let mut cached = self.cached.lock().await;
        if let Some(token) = cached.as_ref().filter(|token| token.expires_at > now) {
            return Ok(token.value.clone());
        }
        let assertion = encode(
            &Header::new(Algorithm::RS256),
            &json!({
                "iss":self.email,"scope":"https://www.googleapis.com/auth/firebase.messaging",
                "aud":self.token_url,"iat":now,"exp":now+3600,
            }),
            &self.key,
        )
        .map_err(|_| DeliveryError::Rejected)?;
        let result = self.exchange(client, &assertion).await?;
        let value = result.value.clone();
        *cached = Some(result);
        Ok(value)
    }

    async fn exchange(
        &self,
        client: &reqwest::Client,
        assertion: &str,
    ) -> Result<Cached, DeliveryError> {
        let response = client
            .post(&self.token_url)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", assertion),
            ])
            .send()
            .await
            .map_err(|_| DeliveryError::Retryable(0))?;
        let status = response.status();
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
            return Err(DeliveryError::Retryable(super::retry_after(&response)));
        }
        if !status.is_success() {
            return Err(DeliveryError::Rejected);
        }
        let result: OAuthResponse = response.json().await.map_err(|_| DeliveryError::Rejected)?;
        if result.access_token.expose_secret().is_empty() || result.expires_in <= 60 {
            return Err(DeliveryError::Rejected);
        }
        Ok(Cached {
            value: result.access_token,
            expires_at: chrono::Utc::now().timestamp()
                + i64::try_from(result.expires_in.min(3600) - 60)
                    .map_err(|_| DeliveryError::Rejected)?,
        })
    }

    /// Clear only `rejected`, preserving credentials refreshed by concurrent requests.
    pub(super) async fn invalidate(&self, rejected: &SecretString) {
        let mut cached = self.cached.lock().await;
        if cached
            .as_ref()
            .is_some_and(|current| current.value.expose_secret() == rejected.expose_secret())
        {
            *cached = None;
        }
    }
}

fn valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
}

#[cfg(test)]
pub(super) mod tests;
