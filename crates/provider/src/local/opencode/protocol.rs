//! Private `OpenCode` wire compatibility; the shared session lifecycle never branches on versions.
mod budget;
pub(super) mod discovery;
mod execution;
pub(super) mod history;
pub(super) mod http;
mod metadata;
pub(super) mod permissions;
mod requests;
mod sessions;
pub(super) mod streaming;

use super::types::Fault;
use super::{OpenCodeExecutionLimits, failure, types::ProtocolError};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
/// Native protocol selected once from the installed executable's version.
pub(in crate::local::opencode) enum Version {
    V1,
    V2,
}

#[cfg(test)]
mod tests;

impl Version {
    pub(in crate::local::opencode) fn parse(output: &str) -> Result<Self, ProtocolError> {
        let version = output
            .trim()
            .strip_prefix("opencode ")
            .unwrap_or(output.trim());
        let version = version.strip_prefix('v').unwrap_or(version);
        let numbers = version
            .split(['.', '-', '+'])
            .take(3)
            .map(str::parse::<u32>)
            .collect::<Result<Vec<_>, _>>();
        match numbers.as_deref() {
            Ok([1, _, _]) => Ok(Self::V1),
            Ok([2, minor, patch]) if *minor > 0 || *patch >= 10 => Ok(Self::V2),
            Ok(_) | Err(_) => Err(failure(
                Fault::AgentCapabilityUnsupported,
                "unsupported OpenCode version; require OpenCode 1.x or 2.0.10+",
            )),
        }
    }

    pub(in crate::local::opencode) fn prefix(self) -> &'static str {
        match self {
            Self::V1 => "",
            Self::V2 => "/api",
        }
    }
}

impl Version {
    pub(super) fn password_environment(self) -> &'static str {
        match self {
            Self::V1 => "OPENCODE_SERVER_PASSWORD",
            Self::V2 => "OPENCODE_PASSWORD",
        }
    }

    pub(super) fn health_path(self) -> &'static str {
        match self {
            Self::V1 => "/global/health",
            Self::V2 => "/api/info",
        }
    }

    pub(super) fn input_id(self) -> String {
        let random = uuid::Uuid::new_v4().simple().to_string();
        match self {
            Self::V1 => {
                let timestamp = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis();
                format!(
                    "msg_{:012x}{}",
                    (timestamp << 12) & 0xffff_ffff_ffff,
                    &random[..14]
                )
            }
            Self::V2 => random,
        }
    }

    fn model(self, model: (&str, &str), variant: Option<&str>) -> Value {
        match self {
            Self::V1 => json!({"providerID":model.0, "modelID":model.1}),
            Self::V2 => {
                let mut selected = json!({"providerID":model.0, "id":model.1});
                if let Some(variant) = variant {
                    selected["variant"] = json!(variant);
                }
                selected
            }
        }
    }
}
