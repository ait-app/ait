use axum::{
    Json, Router,
    extract::Path,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};

use super::*;

async fn response(Path(mode): Path<String>, Json(body): Json<Value>) -> axum::response::Response {
    match mode.as_str() {
        "quota-json" => (StatusCode::TOO_MANY_REQUESTS, Json(json!({"error":{"type":"insufficient_quota","message":"insufficient balance token=private-key"},"request":{"prompt":"private prompt"}}))).into_response(),
        "missing" => StatusCode::NOT_FOUND.into_response(),
        "unauthorized" => StatusCode::UNAUTHORIZED.into_response(),
        "forbidden" => StatusCode::FORBIDDEN.into_response(),
        "limited" => StatusCode::TOO_MANY_REQUESTS.into_response(),
        "failed" => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
        "invalid" => "invalid json private response".into_response(),
        "large" => "x".repeat(MAX_FRAME+1).into_response(),
        "mismatch" => Json(json!({"type":"server-response","rpcId":"other","result":{"ok":true,"value":{}}})).into_response(),
        "rejected" => Json(json!({"type":"server-response","rpcId":body["rpcId"],"result":{"ok":false,"error":{"message":"private detail"}}})).into_response(),
        "no-value" | "$events/result" => Json(json!({"type":"server-response","rpcId":body["rpcId"],"result":{"ok":true}})).into_response(),
        "no-outcome" => Json(json!({"type":"server-response","rpcId":body["rpcId"],"result":{}})).into_response(),
        _ => Json(json!({"type":"server-response","rpcId":body["rpcId"],"result":{"ok":true,"value":{"fixture":true}}})).into_response(),
    }
}

#[tokio::test]
async fn native_http_distinguishes_absent_capabilities_rejections_and_invalid_envelopes() {
    let app = Router::new()
        .route(
            "/",
            get(|| async {
                (
                    StatusCode::FOUND,
                    [("set-cookie", "fixture=private; HttpOnly")],
                )
            }),
        )
        .route("/api/{*mode}", post(response));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let api = Api::connect(
        &format!("http://{address}/?token=fixture"),
        Duration::from_secs(5),
        crate::local::diagnostics::Diagnostics::new("deepseek-harness"),
    )
    .await
    .unwrap();
    assert!(!format!("{api:?}").contains("private"));
    assert_eq!(
        api.call("valid", json!({})).await.unwrap(),
        json!({"fixture":true})
    );
    assert_eq!(api.optional_call("missing", json!({})).await.unwrap(), None);
    assert_eq!(
        api.optional_call("$events/result", json!({}))
            .await
            .unwrap(),
        Some(Value::Null),
        "a void acknowledgement is a successful response, not a missing capability"
    );
    assert_eq!(
        api.call("missing", json!({})).await,
        Err(AgentSessionError::Failed)
    );
    assert_eq!(
        api.call("rejected", json!({})).await,
        Err(AgentSessionError::Rejected)
    );
    api.diagnostics.clear();
    assert_eq!(
        api.call("quota-json", json!({})).await,
        Err(AgentSessionError::Failed)
    );
    let message = api.diagnostics.message().unwrap();
    assert!(message.contains("HTTP 429"));
    assert!(message.contains("insufficient balance"));
    assert!(!message.contains("private-key"));
    assert!(!message.contains("private prompt"));
    assert_eq!(
        api.diagnostics.failure().unwrap().category,
        crate::ports::diagnostics::Category::Quota
    );
    for mode in [
        "unauthorized",
        "forbidden",
        "limited",
        "failed",
        "invalid",
        "large",
        "mismatch",
        "no-value",
        "no-outcome",
    ] {
        assert_eq!(
            api.call(mode, json!({})).await,
            Err(AgentSessionError::Failed),
            "{mode}"
        );
    }
    task.abort();
}

#[tokio::test]
async fn launch_without_a_redirect_cookie_never_creates_an_authenticated_client() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = Router::new().route("/", get(|| async { StatusCode::OK }));
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    assert!(matches!(
        Api::connect(
            &format!("http://{address}/?token=fixture"),
            Duration::from_secs(5),
            crate::local::diagnostics::Diagnostics::new("deepseek-harness")
        )
        .await,
        Err(AgentSessionError::Failed)
    ));
    task.abort();
}
