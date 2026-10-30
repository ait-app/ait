//! Direct Apple APNs and Google FCM delivery. Credentials never cross the native push port.

mod auth;

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use metadata::push::ports::{DeliveryError, Notification, PushTransport};
use reqwest::{Client, Response, StatusCode};
use secrecy::ExposeSecret;
use serde::Deserialize;
use serde_json::{Value, json};

/// Provider target stored inside the existing opaque token registration field.
#[derive(Deserialize)]
#[serde(tag = "provider", rename_all = "lowercase", deny_unknown_fields)]
enum Target {
    Apns {
        token: String,
        #[serde(rename = "bundleId")]
        bundle_id: String,
        environment: String,
    },
    Fcm {
        token: String,
    },
}

/// Direct native-provider adapter with private authentication caches and fixed endpoints.
pub(super) struct NativePush {
    client: Client,
    apple: Option<auth::Apple>,
    google: Option<auth::Google>,
    endpoints: Endpoints,
}

struct Endpoints {
    apns: String,
    sandbox: String,
    fcm: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            apns: "https://api.push.apple.com".to_owned(),
            sandbox: "https://api.sandbox.push.apple.com".to_owned(),
            fcm: "https://fcm.googleapis.com".to_owned(),
        }
    }
}

impl fmt::Debug for NativePush {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NativePush")
            .field("apns_configured", &self.apple.is_some())
            .field("fcm_configured", &self.google.is_some())
            .finish_non_exhaustive()
    }
}

impl NativePush {
    /// Load optional provider credentials using `environment`; return sanitized configuration errors.
    pub(super) fn from_environment(
        environment: impl Fn(&str) -> Option<String>,
    ) -> anyhow::Result<Arc<dyn PushTransport>> {
        let adapter = Self {
            client: Client::builder()
                .https_only(true)
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(10))
                .build()?,
            apple: auth::Apple::from_environment(&environment)?,
            google: auth::Google::from_environment(&environment)?,
            endpoints: Endpoints::default(),
        };
        if adapter.apple.is_none() && adapter.google.is_none() {
            tracing::info!(
                "APNs/FCM credentials are not configured; native local notifications remain available"
            );
        }
        Ok(Arc::new(adapter))
    }

    async fn apns(
        &self,
        notification: &Notification,
        token: &str,
        bundle: &str,
        environment: &str,
    ) -> Result<(), DeliveryError> {
        let apple = self.apple.as_ref().ok_or(DeliveryError::Unconfigured)?;
        if !apple.allows(bundle)
            || token.len() > 512
            || token.is_empty()
            || !token.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(DeliveryError::Rejected);
        }
        let host = match environment {
            "development" => &self.endpoints.sandbox,
            "production" => &self.endpoints.apns,
            _ => return Err(DeliveryError::Rejected),
        };
        let authorization = apple.authorization().await?;
        let response = self
            .client
            .post(format!("{host}/3/device/{token}"))
            .version(reqwest::Version::HTTP_2)
            .bearer_auth(authorization.expose_secret())
            .header("apns-topic", bundle)
            .header("apns-push-type", "alert")
            .header("apns-priority", "10")
            .header(
                "apns-expiration",
                (chrono::Utc::now().timestamp() + 3600).to_string(),
            )
            .json(&apns_payload(notification))
            .send()
            .await
            .map_err(|_| DeliveryError::Retryable(0))?;
        let status = response.status();
        if status == StatusCode::OK {
            return Ok(());
        }
        let retry = retry_after(&response);
        let body = response.json::<Value>().await.unwrap_or(Value::Null);
        let reason = body["reason"].as_str().unwrap_or("");
        if reason == "ExpiredProviderToken" {
            apple.invalidate(&authorization).await;
            return Err(DeliveryError::Retryable(0));
        }
        classify_apns(status, reason, retry)
    }

    async fn fcm(&self, notification: &Notification, token: &str) -> Result<(), DeliveryError> {
        let google = self.google.as_ref().ok_or(DeliveryError::Unconfigured)?;
        if token.is_empty() || token.len() > 4096 || token.chars().any(char::is_control) {
            return Err(DeliveryError::Rejected);
        }
        let authorization = google.authorization(&self.client).await?;
        let response = self
            .client
            .post(format!(
                "{}/v1/projects/{}/messages:send",
                self.endpoints.fcm,
                google.project()
            ))
            .bearer_auth(authorization.expose_secret())
            .json(&fcm_payload(notification, token))
            .send()
            .await
            .map_err(|_| DeliveryError::Retryable(0))?;
        let status = response.status();
        if status.is_success() {
            return Ok(());
        }
        let retry = retry_after(&response);
        if status == StatusCode::UNAUTHORIZED {
            google.invalidate(&authorization).await;
            return Err(DeliveryError::Retryable(0));
        }
        let body = response.json::<Value>().await.unwrap_or(Value::Null);
        classify_fcm(status, &body, retry)
    }
}

#[async_trait]
impl PushTransport for NativePush {
    async fn send(&self, notification: &Notification, token: &str) -> Result<(), DeliveryError> {
        match serde_json::from_str::<Target>(token).map_err(|_| DeliveryError::Unregistered)? {
            Target::Apns {
                token,
                bundle_id,
                environment,
            } => {
                self.apns(notification, &token, &bundle_id, &environment)
                    .await
            }
            Target::Fcm { token } => self.fcm(notification, &token).await,
        }
    }
}

fn apns_payload(notification: &Notification) -> Value {
    // Expo's native receiver expects routing values under `body`; no Expo server is involved.
    json!({"aps":{"alert":{"title":notification.title,"body":notification.body},"sound":"default"},"body":notification.data})
}

fn fcm_payload(notification: &Notification, token: &str) -> Value {
    json!({"message":{"token":token,
        "data":{"title":notification.title,"message":notification.body,"body":notification.data.to_string(),"channelId":"default","sound":"default"},
        "android":{"priority":"HIGH","ttl":"3600s"}}})
}

fn retry_after(response: &Response) -> u64 {
    response
        .headers()
        .get("retry-after")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(60)
        .clamp(1, 3600)
}

fn classify_apns(status: StatusCode, reason: &str, retry: u64) -> Result<(), DeliveryError> {
    if status == StatusCode::GONE && reason == "Unregistered"
        || status == StatusCode::BAD_REQUEST && reason == "BadDeviceToken"
    {
        Err(DeliveryError::Unregistered)
    } else if status == StatusCode::TOO_MANY_REQUESTS {
        Err(DeliveryError::Retryable(retry))
    } else if status.is_server_error() {
        Err(DeliveryError::Retryable(0))
    } else {
        Err(DeliveryError::Rejected)
    }
}

fn classify_fcm(status: StatusCode, body: &Value, retry: u64) -> Result<(), DeliveryError> {
    let unregistered = body["error"]["details"].as_array().is_some_and(|details| {
        details.iter().any(|detail| {
            detail["@type"] == "type.googleapis.com/google.firebase.fcm.v1.FcmError"
                && detail["errorCode"] == "UNREGISTERED"
        })
    });
    if unregistered {
        Err(DeliveryError::Unregistered)
    } else if status == StatusCode::TOO_MANY_REQUESTS {
        Err(DeliveryError::Retryable(retry))
    } else if status.is_server_error() {
        Err(DeliveryError::Retryable(0))
    } else {
        Err(DeliveryError::Rejected)
    }
}

#[cfg(test)]
mod tests;
