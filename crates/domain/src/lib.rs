//! Pure business identities, records, values and validation shared across server capabilities.

pub mod agent;
pub mod agent_runtime;
pub mod bot;
pub mod creation;
pub mod directory_sync;
pub mod pagination;
pub mod schedule;
pub mod session;
pub mod storage;
pub mod summary;
pub mod workspace;

use std::fmt;
use std::str::FromStr;

use uuid::Uuid;

/// A persisted or supplied domain value violates an invariant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("invalid domain value")]
pub struct InvalidValue;

macro_rules! identity {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        pub struct $name(Uuid);
        impl $name {
            /// Allocate a new UUID identity.
            #[must_use]
            pub fn generate() -> Self {
                Self(Uuid::new_v4())
            }
        }
        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
        impl FromStr for $name {
            type Err = InvalidValue;
            fn from_str(value: &str) -> Result<Self, Self::Err> {
                let id = Uuid::parse_str(value).map_err(|_| InvalidValue)?;
                if id.is_nil() {
                    return Err(InvalidValue);
                }
                Ok(Self(id))
            }
        }
        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_str(self)
            }
        }
        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let value = <String as serde::Deserialize>::deserialize(deserializer)?;
                value.parse().map_err(serde::de::Error::custom)
            }
        }
    };
}

identity!(OperationId, "Identity of a durable operation receipt.");
identity!(AgentId, "Stable identity of a catalog Agent preset.");
identity!(BotId, "Stable identity of a workspace Bot.");
identity!(ChannelId, "Identity of a Bot-owned channel instance.");
identity!(ConversationId, "Identity of an isolated Bot conversation.");
identity!(BotTurnId, "Identity of a durable Bot turn.");

#[cfg(test)]
mod tests;
