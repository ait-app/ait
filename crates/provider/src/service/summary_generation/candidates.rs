use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
use std::time::Duration;

use crate::summary::{SummaryRequest, SummarySelection};
use serde_json::Value;

use crate::ports::agent_session::AgentClient;

pub(super) async fn resolve(
    clients: &BTreeMap<String, Arc<dyn AgentClient>>,
    config: &Value,
    request: &SummaryRequest,
) -> Vec<SummarySelection> {
    let mut automatic = BTreeMap::new();
    for (provider, client) in clients {
        if config["providers"][provider]["enabled"] == false {
            continue;
        }
        if let Ok(Ok(details)) =
            tokio::time::timeout(Duration::from_secs(5), client.discover(&request.cwd)).await
            && let Some(selection) = client.summary_model(&details.models)
        {
            automatic.insert(provider.clone(), selection);
        }
    }
    ordered(
        clients.keys().map(String::as_str),
        config,
        request.selection.as_ref(),
        &automatic,
    )
}

fn ordered<'a>(
    providers: impl Iterator<Item = &'a str>,
    config: &Value,
    current: Option<&SummarySelection>,
    automatic: &BTreeMap<String, SummarySelection>,
) -> Vec<SummarySelection> {
    let available: BTreeSet<_> = providers
        .filter(|provider| config["providers"][*provider]["enabled"] != false)
        .collect();
    let mut result: Vec<SummarySelection> = config["metadataGeneration"]["providers"]
        .as_array()
        .into_iter()
        .flatten()
        .take(32)
        .filter_map(|value| serde_json::from_value::<SummarySelection>(value.clone()).ok())
        .filter_map(|candidate| {
            let candidate = SummarySelection {
                provider: candidate.provider.trim().to_owned(),
                ..candidate
            };
            if !available.contains(candidate.provider.as_str()) {
                return None;
            }
            if candidate
                .model
                .as_deref()
                .is_some_and(|model| !model.trim().is_empty())
            {
                return Some(candidate);
            }
            let mut selected = automatic.get(&candidate.provider)?.clone();
            if candidate.thinking_option_id.is_some() {
                selected.thinking_option_id = candidate.thinking_option_id;
            }
            Some(selected)
        })
        .collect();
    // Foreground selection affects provider preference only, never the auxiliary model.
    if let Some(current) = current
        && available.contains(current.provider.as_str())
        && let Some(candidate) = automatic.get(&current.provider)
    {
        result.push(candidate.clone());
    }
    result.extend(
        automatic
            .values()
            .filter(|candidate| available.contains(candidate.provider.as_str()))
            .cloned(),
    );
    let mut seen = BTreeSet::new();
    result.retain(|candidate| seen.insert(candidate.clone()));
    result
}

#[cfg(test)]
mod tests;
