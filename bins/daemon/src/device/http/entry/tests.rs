use std::sync::atomic::{AtomicUsize, Ordering};

use axum::{Json, Router, extract::State, routing::get};
use serde_json::{Value, json};
use tokio::{net::TcpListener, task::JoinHandle};

use super::*;

struct Gateway {
    url: String,
    calls: Arc<AtomicUsize>,
    task: JoinHandle<()>,
}

impl Gateway {
    async fn start(status: StatusCode, region: Value) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let calls = Arc::new(AtomicUsize::new(0));
        let router =
            Router::new()
                .route(
                    "/v1/client/bootstrap",
                    get(
                        move |State(calls): State<Arc<AtomicUsize>>,
                              headers: axum::http::HeaderMap| async move {
                            assert!(!headers.contains_key("authorization"));
                            calls.fetch_add(1, Ordering::Relaxed);
                            (status, Json(json!({"recommended_region": region})))
                        },
                    ),
                )
                .with_state(calls.clone());
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self { url, calls, task }
    }
}

impl Drop for Gateway {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[tokio::test]
async fn recommendation_uses_built_in_destination_and_is_shared_across_clones() {
    let mainland = Gateway::start(StatusCode::OK, json!("global")).await;
    let global = Gateway::start(StatusCode::OK, json!(null)).await;
    let entry = Entry::new(None, [mainland.url.clone(), global.url.clone()]);
    let client = Client::new();

    assert_eq!(entry.selected(), None);
    assert_eq!(entry.resolve(&client).await.unwrap(), global.url);
    assert_eq!(entry.clone().resolve(&client).await.unwrap(), global.url);
    assert_eq!(mainland.calls.load(Ordering::Relaxed), 1);
    assert_eq!(global.calls.load(Ordering::Relaxed), 1);
}

#[tokio::test]
async fn unreachable_recommended_entry_falls_back_and_legacy_gateways_are_supported() {
    let mainland = Gateway::start(StatusCode::OK, json!("global")).await;
    let offline = Gateway::start(StatusCode::SERVICE_UNAVAILABLE, json!(null)).await;
    let legacy = Gateway::start(StatusCode::NOT_FOUND, json!(null)).await;
    let client = Client::new();

    assert_eq!(
        detect(&client, [&mainland.url, &offline.url])
            .await
            .unwrap(),
        mainland.url
    );
    assert_eq!(
        detect(&client, [&legacy.url, &offline.url]).await.unwrap(),
        legacy.url
    );
    assert_eq!(
        detect(&client, [&offline.url, &legacy.url]).await.unwrap(),
        legacy.url
    );
    assert_eq!(
        detect(&client, [&legacy.url, &legacy.url]).await.unwrap(),
        legacy.url
    );
}

#[tokio::test]
async fn overrides_skip_discovery_and_outages_are_not_cached() {
    let offline = Gateway::start(StatusCode::SERVICE_UNAVAILABLE, json!(null)).await;
    let client = Client::new();
    let official = [offline.url.clone(), offline.url.clone()];
    let pinned = Entry::new(
        Some("http://localhost:1234/api".to_owned()),
        official.clone(),
    );
    let automatic = Entry::new(None, official);

    assert_eq!(
        pinned.resolve(&client).await.unwrap(),
        "http://localhost:1234/api"
    );
    assert_eq!(offline.calls.load(Ordering::Relaxed), 0);
    for _ in 0..2 {
        assert!(matches!(
            automatic.resolve(&client).await,
            Err(Error::Unavailable)
        ));
        assert_eq!(automatic.selected(), None);
    }
    assert_eq!(offline.calls.load(Ordering::Relaxed), 4);
}
