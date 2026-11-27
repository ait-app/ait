use std::sync::Arc;
use std::time::Duration;

use futures_util::stream::{FuturesUnordered, StreamExt};
use host_link::{CENTER, Error};
use reqwest::{Client, StatusCode};
use serde::Deserialize;
use tokio::sync::OnceCell;

pub(super) const GLOBAL_CENTER: &str = "https://hub.ait-app.com/api";
pub(super) const GLOBAL_VERIFICATION_URI: &str = "https://hub.ait-app.com/auth/device";
const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);

/// One gateway selection for all machine authorization and publication within a runtime.
#[derive(Debug, Clone)]
pub(super) struct Entry(Arc<Selection>);

#[derive(Debug)]
struct Selection {
    selected: OnceCell<String>,
    official: [String; 2],
}

impl Entry {
    /// Create a lazy selection with a pinned override or these two official API bases.
    pub(super) fn new(override_url: Option<String>, official: [String; 2]) -> Self {
        Self(Arc::new(Selection {
            selected: OnceCell::new_with(override_url),
            official,
        }))
    }

    /// Use the official mainland and overseas gateways unless the operator supplied an override.
    pub(super) fn official(override_url: Option<String>) -> Self {
        Self::new(override_url, [CENTER.to_owned(), GLOBAL_CENTER.to_owned()])
    }

    /// Return the selected URL without starting HTTP discovery.
    pub(super) fn selected(&self) -> Option<&str> {
        self.0.selected.get().map(String::as_str)
    }

    /// Return the runtime gateway, or `Unavailable` when neither official entry is reachable.
    pub(super) async fn resolve(&self, client: &Client) -> Result<&str, Error> {
        self.0
            .selected
            .get_or_try_init(|| async {
                detect(client, self.0.official.each_ref().map(String::as_str))
                    .await
                    .map(str::to_owned)
            })
            .await
            .map(String::as_str)
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Region {
    Cn,
    Global,
    #[serde(other)]
    Unknown,
}

impl Region {
    fn index(self) -> Option<usize> {
        match self {
            Self::Cn => Some(0),
            Self::Global => Some(1),
            Self::Unknown => None,
        }
    }
}

#[derive(Deserialize)]
struct Recommendation {
    recommended_region: Option<Region>,
}

/// Discover only against the built-in gateways. Legacy gateways can still return 404.
async fn probe(client: &Client, base: &str) -> Option<Recommendation> {
    let response = client
        .get(format!("{base}/v1/client/bootstrap"))
        .timeout(PROBE_TIMEOUT)
        .send()
        .await
        .ok()?;
    if response.status() == StatusCode::NOT_FOUND {
        return Some(Recommendation {
            recommended_region: None,
        });
    }
    if !response.status().is_success() {
        return None;
    }
    response.json().await.ok()
}

/// Choose the first recommendation with a reachable destination. Without a usable
/// recommendation, preserve the mainland default when reachable.
async fn detect<'a>(client: &Client, official: [&'a str; 2]) -> Result<&'a str, Error> {
    let mut probes: FuturesUnordered<_> = official
        .into_iter()
        .enumerate()
        .map(|(index, base)| async move { (index, probe(client, base).await) })
        .collect();
    let mut reachable = [false; 2];
    let mut first_recommendation = None;
    while let Some((index, result)) = probes.next().await {
        let Some(result) = result else {
            continue;
        };
        reachable[index] = true;
        let recommendation = result.recommended_region.and_then(Region::index);
        if let Some(index) = first_recommendation
            .into_iter()
            .chain(recommendation)
            .find(|&index| reachable[index])
        {
            return Ok(official[index]);
        }
        first_recommendation = first_recommendation.or(recommendation);
    }
    reachable
        .into_iter()
        .position(|available| available)
        .map(|index| official[index])
        .ok_or(Error::Unavailable)
}

#[cfg(test)]
mod tests;
