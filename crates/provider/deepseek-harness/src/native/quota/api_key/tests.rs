use super::*;
use axum::{
    Router,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::get,
};

fn response() -> Value {
    json!({"is_available":true,"balance_infos":[{"currency":"CNY","total_balance":"12.01","granted_balance":"2.67","topped_up_balance":"9.33"}]})
}

#[test]
fn official_totals_are_not_recomputed_from_rounded_components() {
    let report = project(&response()).unwrap();
    assert_eq!(report["balances"][0]["remaining"], 12.01);
    assert_eq!(report["balances"][1]["remaining"], 9.33);
    assert_eq!(report["balances"][2]["remaining"], 2.67);
    assert_eq!(report["balances"][0]["unit"], "cny");
    assert_eq!(report["windows"], json!([]));
    assert!(report["balances"][0].get("used").is_none());
}

#[test]
fn zero_or_negative_balances_do_not_become_login_errors() {
    let mut value = response();
    value["is_available"] = json!(false);
    value["balance_infos"][0]["total_balance"] = json!("-0.01");
    value["balance_infos"][0]["granted_balance"] = json!("0");
    value["balance_infos"][0]["currency"] = json!("USD");
    let report = project(&value).unwrap();
    assert_eq!(report["status"], "available");
    assert_eq!(report["balances"][0]["remaining"], -0.01);
    assert_eq!(report["details"][0]["tone"], "warning");
    value["balance_infos"] = json!([]);
    assert_eq!(project(&value).unwrap()["status"], "unavailable");
}

#[test]
fn malformed_balance_responses_fail_without_forwarding_extra_data() {
    for field in ["total_balance", "granted_balance", "topped_up_balance"] {
        let mut value = response();
        value["balance_infos"][0][field] = json!("fixture-secret");
        assert!(project(&value).is_err());
    }
    for value in [
        json!({}),
        json!({"is_available":false,"balance_infos":{}}),
        json!({"is_available":true,"balance_infos":[{}, {}, {}]}),
    ] {
        assert!(project(&value).is_err());
    }
    let mut value = response();
    value["token"] = json!("fixture-secret");
    assert!(
        !project(&value)
            .unwrap()
            .to_string()
            .contains("fixture-secret")
    );
}

#[tokio::test]
async fn balance_http_is_authenticated_bounded_and_never_follows_redirects_or_leaks_errors() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new()
        .route(
            "/ok",
            get(|headers: HeaderMap| async move {
                assert_eq!(headers["authorization"], "Bearer fixture-secret");
                axum::Json(response())
            }),
        )
        .route(
            "/auth",
            get(|| async { (StatusCode::UNAUTHORIZED, "fixture-secret") }),
        )
        .route(
            "/forbidden",
            get(|| async { (StatusCode::FORBIDDEN, "fixture-secret") }),
        )
        .route(
            "/failed",
            get(|| async { (StatusCode::INTERNAL_SERVER_ERROR, "fixture-secret") }),
        )
        .route(
            "/redirect",
            get(|| async {
                (StatusCode::FOUND, [("location", "/ok")], "fixture-secret").into_response()
            }),
        )
        .route("/large", get(|| async { "x".repeat(65537) }))
        .route("/invalid", get(|| async { "fixture-secret" }))
        .route(
            "/slow",
            get(|| async {
                tokio::time::sleep(Duration::from_secs(2)).await;
                "{}"
            }),
        );
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let key = SecretString::from("fixture-secret");
    assert_eq!(
        fetch(&key, &format!("{base}/ok"), Duration::from_secs(1))
            .await
            .unwrap()["status"],
        "available"
    );
    for (path, status) in [("auth", 401), ("forbidden", 403)] {
        let report = fetch(&key, &format!("{base}/{path}"), Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(report["problem"]["status"], status);
        assert!(!report.to_string().contains("fixture-secret"));
    }
    for path in ["failed", "redirect", "large", "invalid", "slow"] {
        assert!(
            fetch(&key, &format!("{base}/{path}"), Duration::from_millis(100))
                .await
                .is_err(),
            "{path}"
        );
    }
    server.abort();
}
