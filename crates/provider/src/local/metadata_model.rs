//! Shared catalog matching; preference order belongs to each adapter.
use metadata::ports::generation::MetadataSelection;
use serde_json::Value;

pub(super) fn select(
    provider: &str,
    models: &[Value],
    preferences: &[&str],
) -> Option<MetadataSelection> {
    let model = preferences.iter().find_map(|preference| {
        models.iter().find(|model| {
            model["isSelectable"] != false
                && model["id"].as_str().is_some_and(|id| !id.trim().is_empty())
                && ["id", "label"].iter().any(|key| {
                    model[key]
                        .as_str()
                        .is_some_and(|value| value.to_ascii_lowercase().contains(preference))
                })
        })
    })?;
    let options = model["thinkingOptions"].as_array();
    let effort = ["none", "minimal", "low"].into_iter().find(|effort| {
        options.is_some_and(|options| options.iter().any(|option| option["id"] == *effort))
    });
    Some(MetadataSelection {
        provider: provider.to_owned(),
        model: model["id"].as_str().map(str::to_owned),
        thinking_option_id: effort.map(str::to_owned),
    })
}

#[cfg(test)]
mod tests;
