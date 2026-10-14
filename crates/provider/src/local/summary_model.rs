//! Shared catalog matching; preference order belongs to each adapter.
use domain::summary::SummarySelection;
use serde_json::Value;

pub(super) fn select(
    provider: &str,
    models: &[Value],
    preferences: &[&str],
) -> Option<SummarySelection> {
    let model = preferences.iter().find_map(|preference| {
        models.iter().find(|model| {
            model["isSelectable"] != false
                && model["id"].as_str().is_some_and(|id| !id.trim().is_empty())
                && ["id", "label"].iter().any(|key| {
                    model[key]
                        .as_str()
                        .is_some_and(|value| matches_preference(value, preference))
                })
        })
    })?;
    let options = model["thinkingOptions"].as_array();
    let effort = ["none", "minimal", "low"].into_iter().find(|effort| {
        options.is_some_and(|options| options.iter().any(|option| option["id"] == *effort))
    });
    Some(SummarySelection {
        provider: provider.to_owned(),
        model: model["id"].as_str().map(str::to_owned),
        thinking_option_id: effort.map(str::to_owned),
    })
}

fn matches_preference(value: &str, preference: &str) -> bool {
    let value = value.to_ascii_lowercase();
    value.match_indices(preference).any(|(start, matched)| {
        let end = start + matched.len();
        (start == 0 || !value.as_bytes()[start - 1].is_ascii_alphanumeric())
            && (end == value.len() || !value.as_bytes()[end].is_ascii_alphanumeric())
    })
}

#[cfg(test)]
mod tests;
