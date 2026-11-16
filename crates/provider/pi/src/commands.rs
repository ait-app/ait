use std::borrow::Cow;
use std::collections::HashSet;
use std::time::Duration;

use domain::workspace::sandbox::WriteConfinement;
use serde_json::{Value, json};

use super::{PiClient, probe};
use provider_core::ports::agent_session::AgentSessionError;

const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(2);

/// Probe commands in the canonical `cwd`, honoring the supplied write confinement.
/// Returns `Rejected` for invalid directories and native discovery or catalog errors otherwise.
pub(super) async fn discover(
    client: &PiClient,
    cwd: &str,
    confinement: Option<&WriteConfinement>,
) -> Result<Vec<Value>, AgentSessionError> {
    let directory = tokio::fs::canonicalize(cwd)
        .await
        .map_err(|_| AgentSessionError::Rejected)?;
    if !tokio::fs::metadata(&directory)
        .await
        .map_err(|_| AgentSessionError::Rejected)?
        .is_dir()
    {
        return Err(AgentSessionError::Rejected);
    }
    let result = probe::run(
        client,
        directory.to_str().ok_or(AgentSessionError::Rejected)?,
        confinement,
        Some(DISCOVERY_TIMEOUT.min(client.deadline)),
        |transport| Box::pin(transport.request(json!({"type":"get_commands"}))),
    )
    .await;
    finish_discovery(result)
}

fn finish_discovery(
    result: Result<Value, AgentSessionError>,
) -> Result<Vec<Value>, AgentSessionError> {
    // This parameterless capability probe has no user input for Pi to reject.
    // Older peers may accept RPC sessions but not implement get_commands.
    let response = match result {
        Err(AgentSessionError::Rejected) => return Err(AgentSessionError::Unavailable),
        result => result?,
    };
    catalog(&response).inspect_err(|error| {
        tracing::warn!(?error, "Pi command catalog validation failed");
    })
}

/// Map Pi's native command catalog to host menu entries, preserving invocation names.
/// Omit names the composer cannot represent and retain the first entry for each name.
/// Description and argument-hint control characters become spaces. Both are capped at 1024 UTF-8 bytes
/// and omitted if the catalog exceeds 768 KiB.
/// Returns `Failed` for malformed catalog envelopes or names exceeding the payload budget.
pub(super) fn catalog(response: &Value) -> Result<Vec<Value>, AgentSessionError> {
    let mut seen = HashSet::new();
    let mut omitted_names = 0;
    let mut commands = response["commands"]
        .as_array()
        .filter(|commands| commands.len() <= 4096)
        .ok_or(AgentSessionError::Failed)?
        .iter()
        .filter_map(|command| {
            let name = command["name"].as_str().filter(|name| {
                !name.is_empty() && name.len() <= 1024 && !name.chars().any(char::is_control)
            });
            let Some(name) = name else {
                omitted_names += 1;
                return None;
            };
            // Match the composer's slash-query delimiters, including JS's BOM whitespace.
            if name.chars().any(|character| {
                character.is_whitespace() || matches!(character, '/' | '"' | '\'' | '\u{feff}')
            }) {
                omitted_names += 1;
                return None;
            }
            if !seen.insert(name) {
                return None;
            }
            Some(json!({
                "name": name,
                "description": command_label(&command["description"]),
                "argumentHint": command_label(&command["argumentHint"]),
                "kind": if command["source"] == "skill" { "skill" } else { "command" }
            }))
        })
        .collect::<Vec<_>>();
    if omitted_names > 0 {
        tracing::warn!(
            omitted_names,
            "Pi command catalog omitted invalid or unrepresentable names"
        );
    }
    if !catalog_fits(&commands) {
        for command in &mut commands {
            command["description"] = json!("");
            command["argumentHint"] = json!("");
        }
        if !catalog_fits(&commands) {
            return Err(AgentSessionError::Failed);
        }
    }
    Ok(commands)
}

// Optional display labels share a bounded, control-free representation.
fn command_label(value: &Value) -> Cow<'_, str> {
    let text = value.as_str().unwrap_or_default();
    let text = &text[..text.floor_char_boundary(text.len().min(1024))];
    if text.chars().any(char::is_control) {
        Cow::Owned(
            text.chars()
                .map(|character| {
                    if character.is_control() {
                        ' '
                    } else {
                        character
                    }
                })
                .collect(),
        )
    } else {
        Cow::Borrowed(text)
    }
}

// Count escaped JSON bytes without allocating a serialized copy. Stop at the budget.
fn catalog_fits(commands: &[Value]) -> bool {
    let mut counter = CatalogBudget(768 * 1024);
    serde_json::to_writer(&mut counter, commands).is_ok()
}

struct CatalogBudget(usize);

impl std::io::Write for CatalogBudget {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self
            .0
            .checked_sub(bytes.len())
            .ok_or_else(|| std::io::Error::other("command catalog exceeds byte budget"))?;
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
