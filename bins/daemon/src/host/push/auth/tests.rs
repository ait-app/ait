use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

pub(in crate::host::push) fn apple() -> Apple {
    let random = ring::rand::SystemRandom::new();
    let key = ring::signature::EcdsaKeyPair::generate_pkcs8(
        &ring::signature::ECDSA_P256_SHA256_FIXED_SIGNING,
        &random,
    )
    .unwrap();
    Apple {
        key: EncodingKey::from_ec_der(key.as_ref()),
        key_id: "test-key".to_owned(),
        team_id: "test-team".to_owned(),
        bundles: vec!["dev.ait.mobile".to_owned()],
        cached: Mutex::new(None),
    }
}

pub(in crate::host::push) fn google(token_url: String) -> Google {
    Google {
        project_id: "test-project".to_owned(),
        email: "test@example.invalid".to_owned(),
        key: EncodingKey::from_secret(b"not-a-signing-key"),
        cached: Mutex::new(Some(Cached {
            value: SecretString::from("mock-access-token"),
            expires_at: i64::MAX,
        })),
        token_url,
    }
}

#[tokio::test]
async fn apple_generates_and_caches_es256_credentials_with_expected_claims() {
    let apple = apple();
    let first = apple.authorization().await.unwrap();
    let header = jsonwebtoken::decode_header(first.expose_secret()).unwrap();
    assert_eq!(header.alg, Algorithm::ES256);
    assert_eq!(header.kid.as_deref(), Some("test-key"));
    let claims = first.expose_secret().split('.').nth(1).unwrap();
    let claims: serde_json::Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(claims).unwrap()).unwrap();
    assert_eq!(claims["iss"], "test-team");
    assert!(claims["iat"].as_i64().unwrap() <= chrono::Utc::now().timestamp());
    assert_eq!(
        first.expose_secret(),
        apple.authorization().await.unwrap().expose_secret()
    );
}

#[tokio::test]
async fn google_exchanges_the_assertion_at_the_oauth_endpoint_without_exposing_credentials() {
    let (url, mut requests, _server) = super::super::tests::mock_server(
        reqwest::StatusCode::OK,
        json!({"access_token":"mock-oauth-token","expires_in":3600}),
    )
    .await;
    let google = google(format!("{url}/token"));
    let result = google
        .exchange(&reqwest::Client::new(), "mock-signed-assertion")
        .await
        .unwrap();
    assert_eq!(result.value.expose_secret(), "mock-oauth-token");
    let request = requests.recv().await.unwrap();
    assert_eq!(request.path, "/token");
    let form = String::from_utf8(request.body).unwrap();
    assert!(form.contains("assertion=mock-signed-assertion"));
    assert!(form.contains("grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Ajwt-bearer"));
}

#[tokio::test]
async fn authorization_reuses_unexpired_credentials_and_invalidates_them() {
    let apple = Apple {
        key: EncodingKey::from_secret(b"not-a-signing-key"),
        key_id: "key".to_owned(),
        team_id: "team".to_owned(),
        bundles: vec!["dev.ait.mobile".to_owned()],
        cached: Mutex::new(Some(Cached {
            value: SecretString::from("cached-test-credential"),
            expires_at: i64::MAX,
        })),
    };
    assert!(apple.allows("dev.ait.mobile"));
    assert!(!apple.allows("other.application"));
    assert_eq!(
        apple.authorization().await.unwrap().expose_secret(),
        "cached-test-credential"
    );
    apple
        .invalidate(&SecretString::from("older-test-credential"))
        .await;
    assert!(apple.authorization().await.is_ok());
    apple
        .invalidate(&SecretString::from("cached-test-credential"))
        .await;
    assert_eq!(
        apple.authorization().await.unwrap_err(),
        DeliveryError::Rejected
    );
    let google = Google {
        project_id: "test-project".to_owned(),
        email: "test@example.invalid".to_owned(),
        key: EncodingKey::from_secret(b"not-a-signing-key"),
        cached: Mutex::new(Some(Cached {
            value: SecretString::from("cached-test-credential"),
            expires_at: i64::MAX,
        })),
        token_url: GOOGLE_TOKEN_URL.to_owned(),
    };
    let client = reqwest::Client::new();
    assert_eq!(google.project(), "test-project");
    assert_eq!(
        google.authorization(&client).await.unwrap().expose_secret(),
        "cached-test-credential"
    );
    google
        .invalidate(&SecretString::from("older-test-credential"))
        .await;
    assert!(google.authorization(&client).await.is_ok());
    google
        .invalidate(&SecretString::from("cached-test-credential"))
        .await;
    assert_eq!(
        google.authorization(&client).await.unwrap_err(),
        DeliveryError::Rejected
    );
}

#[test]
fn provider_identifiers_cannot_escape_the_fixed_http_endpoints() {
    for id in ["dev.ait.mobile", "test-project", "TEAM12345"] {
        assert!(valid_identifier(id));
    }
    for id in [
        "",
        "../other",
        "project?key=value",
        "host/path",
        "line\nbreak",
    ] {
        assert!(!valid_identifier(id));
    }
}

#[test]
fn credential_configuration_errors_are_safe_and_optional_providers_do_not_read_files() {
    assert!(Apple::from_environment(&|_| None).unwrap().is_none());
    assert!(Google::from_environment(&|_| None).unwrap().is_none());
    let missing = tempfile::tempdir().unwrap();
    let path = missing
        .path()
        .join("missing-provider-key")
        .to_string_lossy()
        .into_owned();
    let apple = Apple::from_environment(&|name| match name {
        "AIT_APNS_KEY_FILE" => Some(path.clone()),
        "AIT_APNS_KEY_ID" => Some("test-key".to_owned()),
        "AIT_APNS_TEAM_ID" => Some("test-team".to_owned()),
        _ => None,
    });
    assert!(
        apple
            .err()
            .unwrap()
            .to_string()
            .contains("cannot read APNs signing key")
    );
    let google = Google::from_environment(&|name| {
        (name == "GOOGLE_APPLICATION_CREDENTIALS").then(|| path.clone())
    });
    assert!(
        google
            .err()
            .unwrap()
            .to_string()
            .contains("cannot read FCM service account")
    );
}
