use super::*;

#[test]
fn replies_split_on_unicode_boundaries_and_bound_utf16_length() {
    let text = "a日😀".repeat(2000);
    let pieces = chunks(&text, 3500);
    assert_eq!(pieces.concat(), text);
    assert!(
        pieces
            .iter()
            .all(|piece| piece.encode_utf16().count() <= 3500)
    );
    assert!(chunks("", 3500).is_empty());
}

#[tokio::test]
async fn oversized_http_responses_are_rejected_without_echoing_the_body() {
    use axum::{Router, routing::get};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route("/", get(|| async { "sensitive".repeat(150_000) })),
        )
        .await
        .unwrap();
    });
    let response = reqwest::get(format!("http://{address}")).await.unwrap();
    assert_eq!(
        json_response(response).await,
        Err(ErrorCode::ResourceExhausted)
    );
    server.abort();
}
