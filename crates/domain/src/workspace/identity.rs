//! Stable project grouping and Git remote identities shared by capability crates.

use std::path::Path;

use super::provisioning::Checkout;

/// Return the final UTF-8 path component, falling back to the supplied path.
#[must_use]
pub fn basename(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or(path)
        .to_owned()
}

/// Derive a grouping key from a checkout's remote or host-local directory identity.
#[must_use]
pub fn derive_project_key(checkout: &Checkout, server_id: &str) -> String {
    let selected_path = checkout
        .worktree_root
        .as_deref()
        .and_then(|root| Path::new(&checkout.cwd).strip_prefix(root).ok())
        .filter(|path| !path.as_os_str().is_empty())
        .and_then(Path::to_str)
        .map(|path| path.replace('\\', "/"));
    if let Some(remote) = checkout.remote_url.as_deref().and_then(parse_remote) {
        let mut path = remote.path;
        if remote.host == "github.com" {
            path.make_ascii_lowercase();
        }
        let host = remote.port.map_or_else(
            || remote.host.clone(),
            |port| format!("{}:{port}", remote.host),
        );
        let key = format!("remote:{host}/{path}");
        return selected_path.map_or(key.clone(), |path| format!("{key}#subdir:{path}"));
    }
    let root = match (&selected_path, checkout.main_repo_root.as_deref()) {
        (Some(selected), Some(main)) => Path::new(main).join(selected),
        _ => Path::new(&checkout.cwd).to_path_buf(),
    };
    format!(
        "host:{server_id}:{}",
        root.to_string_lossy().replace('\\', "/")
    )
}

/// Parsed remote identity used by Project identity and repository provisioning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteLocation {
    /// Normalized host.
    pub host: String,
    /// Nondefault port, if supplied.
    pub port: Option<String>,
    /// Decoded repository path without a trailing `.git`.
    pub path: String,
}

/// Parse a supported Git remote URL into a stable identity, or return none.
#[must_use]
pub fn parse_remote(remote: &str) -> Option<RemoteLocation> {
    let remote = remote.trim();
    if !remote.contains("://")
        && let Some((authority, path)) = remote.split_once(':')
        && let Some((_, host)) = authority.rsplit_once('@')
    {
        return remote_location(host, None, path);
    }
    let (scheme, remainder) = remote.split_once("://")?;
    let default_port = match scheme.to_ascii_lowercase().as_str() {
        "http" => "80",
        "https" => "443",
        "ssh" => "22",
        _ => return None,
    };
    let (authority, path) = remainder.split_once('/')?;
    let host_and_port = authority
        .rsplit_once('@')
        .map_or(authority, |(_, value)| value);
    let (host, port) = match host_and_port.rsplit_once(':') {
        Some((host, port))
            if !port.is_empty() && port.bytes().all(|byte| byte.is_ascii_digit()) =>
        {
            (host, (port != default_port).then(|| port.to_owned()))
        }
        _ => (host_and_port, None),
    };
    let path = path.split(['?', '#']).next()?;
    remote_location(host, port, &percent_decode(path)?)
}

fn remote_location(host: &str, port: Option<String>, path: &str) -> Option<RemoteLocation> {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    let path = path
        .trim()
        .trim_matches('/')
        .strip_suffix(".git")
        .unwrap_or_else(|| path.trim().trim_matches('/'))
        .to_owned();
    let valid_host = host
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        && host
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphanumeric)
        && host
            .as_bytes()
            .last()
            .is_some_and(u8::is_ascii_alphanumeric);
    (valid_host && !path.is_empty()).then_some(RemoteLocation { host, port, path })
}

fn percent_decode(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = hex_digit(*bytes.get(index + 1)?)?;
            let low = hex_digit(*bytes.get(index + 2)?)?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

const fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
