//! Runtime credentials and the Bonsai endpoint, validated before any connection is attempted.

use std::ffi::OsString;
use std::fmt;

use secrecy::{ExposeSecret, SecretString};
use url::{Host, Url};

/// Environment variable holding the Bonsai origin printed by the pairing page.
pub const URL_VARIABLE: &str = "BONSAI_RUNTIME_URL";
/// Environment variable holding the paired runtime ID (`rt_` plus 32 hex digits).
pub const ID_VARIABLE: &str = "BONSAI_RUNTIME_ID";
/// Environment variable holding the runtime bearer token.
pub const TOKEN_VARIABLE: &str = "BONSAI_RUNTIME_TOKEN";

const RUNTIME_PATH: &str = "/runtime";

/// Validated connection settings for one Bonsai account.
#[derive(Clone)]
pub struct Config {
    endpoint: Url,
    runtime_id: String,
    token: SecretString,
}

/// Rejected runtime configuration; messages name variables but never echo their values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    /// Only some of the three required variables are set.
    #[error(
        "set all of BONSAI_RUNTIME_URL, BONSAI_RUNTIME_ID and BONSAI_RUNTIME_TOKEN, or none of them"
    )]
    Partial,
    /// A variable is not valid UTF-8.
    #[error("{0} must be UTF-8")]
    NotUtf8(&'static str),
    /// The URL is not a bare `http(s)` origin.
    #[error(
        "BONSAI_RUNTIME_URL must be an http(s) origin without path, query, fragment or credentials"
    )]
    InvalidUrl,
    /// Plain `http://` was requested for a host that is not loopback.
    #[error("BONSAI_RUNTIME_URL may use http:// only for localhost, 127.0.0.0/8 or [::1]")]
    InsecureUrl,
    /// The runtime ID does not have the shape Bonsai issues.
    #[error("BONSAI_RUNTIME_ID must be rt_ followed by 32 lowercase hex digits")]
    InvalidId,
    /// The token does not have the shape Bonsai issues.
    #[error("BONSAI_RUNTIME_TOKEN must be 32 lowercase hex digits")]
    InvalidToken,
}

impl Config {
    /// Read and validate the runtime variables through an injected environment lookup.
    ///
    /// # Arguments
    ///
    /// * `env` - Variable lookup, normally `std::env::var_os`.
    ///
    /// # Returns
    ///
    /// `None` when none of the three required variables is set, so the adapter stays disabled.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] when only some variables are set or any value is malformed.
    pub fn from_env(env: impl Fn(&str) -> Option<OsString>) -> Result<Option<Self>, ConfigError> {
        let url = text(&env, URL_VARIABLE)?;
        let runtime_id = text(&env, ID_VARIABLE)?;
        let token = text(&env, TOKEN_VARIABLE)?;
        let (url, runtime_id, token) = match (url, runtime_id, token) {
            (None, None, None) => return Ok(None),
            (Some(url), Some(runtime_id), Some(token)) => (url, runtime_id, token),
            _ => return Err(ConfigError::Partial),
        };
        Self::new(&url, runtime_id, SecretString::from(token)).map(Some)
    }

    /// Build a configuration from already-read values.
    ///
    /// # Arguments
    ///
    /// * `origin` - Bonsai origin such as `https://example.com` or `http://localhost:8860`.
    /// * `runtime_id` - Paired runtime ID.
    /// * `token` - Runtime bearer token.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] for a malformed origin, ID or token.
    pub fn new(origin: &str, runtime_id: String, token: SecretString) -> Result<Self, ConfigError> {
        let endpoint = endpoint(origin)?;
        if !is_runtime_id(&runtime_id) {
            return Err(ConfigError::InvalidId);
        }
        if !is_lower_hex(token.expose_secret(), 32) {
            return Err(ConfigError::InvalidToken);
        }
        Ok(Self {
            endpoint,
            runtime_id,
            token,
        })
    }

    /// Return the WebSocket endpoint, `ws(s)://<host>/runtime`.
    #[must_use]
    pub fn endpoint(&self) -> &Url {
        &self.endpoint
    }

    /// Return the paired runtime ID sent in `runtime.hello`.
    #[must_use]
    pub fn runtime_id(&self) -> &str {
        &self.runtime_id
    }

    /// Return the bearer token; expose it only while building the upgrade request.
    #[must_use]
    pub fn token(&self) -> &SecretString {
        &self.token
    }
}

impl fmt::Debug for Config {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Config")
            .field("endpoint", &self.endpoint.as_str())
            .field("runtime_id", &self.runtime_id)
            .field("token", &"[redacted]")
            .finish()
    }
}

/// Convert a Bonsai origin into its runtime WebSocket endpoint.
///
/// # Arguments
///
/// * `origin` - `http(s)://host[:port]` with an empty or `/` path.
///
/// # Errors
///
/// Returns [`ConfigError::InvalidUrl`] for other schemes, paths, queries, fragments or
/// credentials, and [`ConfigError::InsecureUrl`] for `http://` on a non-loopback host.
pub fn endpoint(origin: &str) -> Result<Url, ConfigError> {
    let mut url = Url::parse(origin).map_err(|_| ConfigError::InvalidUrl)?;
    let secure = match url.scheme() {
        "https" => true,
        "http" => false,
        _ => return Err(ConfigError::InvalidUrl),
    };
    if url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.host().is_none()
    {
        return Err(ConfigError::InvalidUrl);
    }
    if !secure && !is_loopback(&url) {
        return Err(ConfigError::InsecureUrl);
    }
    url.set_scheme(if secure { "wss" } else { "ws" })
        .map_err(|()| ConfigError::InvalidUrl)?;
    url.set_path(RUNTIME_PATH);
    Ok(url)
}

/// Return whether a URL's parsed host is loopback (`localhost`, `127.0.0.0/8` or `::1`).
#[must_use]
pub fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    }
}

fn text(
    env: &impl Fn(&str) -> Option<OsString>,
    name: &'static str,
) -> Result<Option<String>, ConfigError> {
    env(name)
        .map(|value| value.into_string().map_err(|_| ConfigError::NotUtf8(name)))
        .transpose()
}

fn is_runtime_id(value: &str) -> bool {
    value
        .strip_prefix("rt_")
        .is_some_and(|hex| is_lower_hex(hex, 32))
}

fn is_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests;
