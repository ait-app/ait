use model::ErrorCode;
use serde_json::json;

use super::{Offer, announce, discover, execute_retrying, machine_name, normalize_remote};
use crate::ports::Project;
use crate::testing::{MockExecutor, MockHost};

#[test]
fn remotes_normalize_like_bonsai() {
    let cases = [
        ("git@github.com:A/B.git", Some("github.com/a/b")),
        ("ssh://git@github.com/a/b", Some("github.com/a/b")),
        ("https://u:t@github.com/a/b.git", Some("github.com/a/b")),
        ("https://github.com:443/a/b/", Some("github.com/a/b")),
        ("https://github.com/a/b?x=1#frag", Some("github.com/a/b")),
        ("github.com/a/b", Some("github.com/a/b")),
        ("https://github.com/a/y.git.git", Some("github.com/a/y")),
        ("https://github.com/a/y.git/.git/", Some("github.com/a/y")),
        ("git@gitserver:a/b", None),
        ("http://localhost/a/b", None),
        ("C:/Users/me/repo", None),
        ("/Users/me/repo", None),
        ("https://github.com", None),
        ("https://github.com/a b", None),
        ("", None),
    ];
    for (remote, expected) in cases {
        assert_eq!(normalize_remote(remote).as_deref(), expected, "{remote}");
        if let Some(normalized) = expected {
            assert_eq!(
                normalize_remote(normalized).as_deref(),
                Some(normalized),
                "idempotent"
            );
        }
    }
    assert_eq!(
        normalize_remote(&format!("github.com/{}", "a".repeat(3000))),
        None
    );
}

#[test]
fn projects_are_announced_without_paths_and_with_valid_ids() {
    let project = |id: &str, name: &str| Project {
        id: id.to_owned(),
        name: name.to_owned(),
        root: "/Users/me/secret".to_owned(),
        remote_url: Some("https://user:token@github.com/me/repo.git".to_owned()),
        branch: Some(String::new()),
    };
    let announced = announce(vec![
        project("prj_0011223344556677", "repo"),
        project("legacy id with spaces", ""),
        project("prj_0011223344556677", "duplicate"),
    ]);
    assert_eq!(announced.len(), 2);
    assert_eq!(announced[0].id, "prj_0011223344556677");
    assert_eq!(
        announced[0].git_remote.as_deref(),
        Some("github.com/me/repo")
    );
    assert_eq!(announced[0].branch, None);
    assert!(announced[1].id.starts_with("p-"));
    assert_eq!(announced[1].id.len(), 18);
    assert_eq!(announced[1].name, announced[1].id);
    assert_eq!(announced[1].host_id, "legacy id with spaces");
    let hello = Offer {
        name: "mac".to_owned(),
        providers: Vec::new(),
        projects: announced,
    }
    .hello("rt_0123456789abcdef0123456789abcdef");
    let text = hello.to_string();
    assert!(!text.contains("/Users/me"));
    assert!(!text.contains("token"));
    assert!(!text.contains("host_id"));
    assert_eq!(hello["default_provider"], serde_json::Value::Null);
}

#[tokio::test]
async fn discovery_announces_available_providers_with_valid_models() {
    let host = MockHost::new();
    host.executor.always(
        "provider.models.list.request",
        Ok(json!({"models": [{"id": "sonnet", "label": "Sonnet"}, {"id": "bad id"}, {"id": "sonnet"},
                             {"id": "opus", "label": ""}]})),
    );
    let offer = discover(
        host.executor.as_ref(),
        host.projects.as_ref(),
        "mac".to_owned(),
        &|_| false,
    )
    .await
    .expect("discover");
    assert_eq!(offer.providers.len(), 1);
    let claude = &offer.providers[0];
    assert_eq!(claude.id, "claude");
    assert!(claude.approvals);
    assert!(!claude.bonsai_write);
    let ids: Vec<&str> = claude
        .models
        .iter()
        .map(|model| model.id.as_str())
        .collect();
    assert_eq!(ids, ["sonnet", "opus"]);
    assert_eq!(claude.models[1].label, "opus");
    assert_eq!(
        offer.default_provider().map(|p| p.id.as_str()),
        Some("claude")
    );
    let hello = offer.hello("rt_0123456789abcdef0123456789abcdef");
    assert_eq!(hello["type"], "runtime.hello");
    assert_eq!(hello["v"], 1);
    assert_eq!(hello["session"], json!(["bonsai.session/1"]));
    assert_eq!(hello["software"]["name"], "ait");
    assert_eq!(hello["projects"][0]["git_remote"], "github.com/me/repo");
    let keys: Vec<&str> = hello["providers"][0]["settings"]
        .as_array()
        .expect("settings declared")
        .iter()
        .filter_map(|setting| setting["key"].as_str())
        .collect();
    assert!(keys.contains(&"effort"));
    assert!(keys.contains(&"append_system_prompt"));
    assert!(
        !keys.contains(&"bonsai_preapprove"),
        "no injected server, nothing to pre-approve"
    );
    assert!(!keys.contains(&"add_dirs"));
    assert!(hello.to_string().len() < 128 * 1024);
}

#[tokio::test]
async fn discovery_survives_unavailable_provider_catalogs() {
    let host = MockHost::new();
    host.executor
        .always("provider.available.list.request", Err(ErrorCode::AgentIo));
    let offer = discover(
        host.executor.as_ref(),
        host.projects.as_ref(),
        "mac".to_owned(),
        &|_| false,
    )
    .await
    .expect("discover");
    assert!(offer.providers.is_empty());
    assert_eq!(offer.projects.len(), 1);
    assert!(offer.default_provider().is_none());
}

#[tokio::test(start_paused = true)]
async fn only_catalog_busy_is_retried() {
    let executor = MockExecutor::default();
    executor.respond("m", Err(ErrorCode::CatalogBusy));
    executor.respond("m", Err(ErrorCode::CatalogBusy));
    executor.respond("m", Ok(json!({"ok": true})));
    assert_eq!(
        execute_retrying(&executor, "m", json!({})).await,
        Ok(json!({"ok": true}))
    );
    assert_eq!(executor.calls_of("m").len(), 3);
    executor.respond("n", Err(ErrorCode::AgentIo));
    assert_eq!(
        execute_retrying(&executor, "n", json!({})).await,
        Err(ErrorCode::AgentIo)
    );
    assert_eq!(executor.calls_of("n").len(), 1);
    executor.always("busy", Err(ErrorCode::CatalogBusy));
    assert_eq!(
        execute_retrying(&executor, "busy", json!({})).await,
        Err(ErrorCode::CatalogBusy)
    );
    assert_eq!(executor.calls_of("busy").len(), 11);
}

#[test]
fn the_machine_name_is_a_bare_display_string() {
    let name = machine_name();
    assert!(!name.is_empty());
    assert!(!name.contains('/'));
    assert!(name.len() <= 128);
}

#[test]
fn an_oversized_hello_leaves_out_projects_until_it_fits() {
    // Arrange: 200 projects of about 1 KiB each, over the 128 KiB hello limit.
    let projects = (0..200)
        .map(|index| super::AnnouncedProject {
            id: format!("p{index}"),
            name: format!("{index}-{}", "\"n".repeat(250)),
            git_remote: Some(format!("github.com/owner/{}", "r".repeat(200))),
            branch: Some("b".repeat(250)),
            host_id: format!("h{index}"),
        })
        .collect();
    let offer = Offer {
        name: "machine".to_owned(),
        providers: Vec::new(),
        projects,
    };

    // Act
    let (frame, dropped) = offer.hello_frame("rt_0").expect("fits once trimmed");

    // Assert
    assert!(frame.len() <= 128 * 1024);
    assert!(dropped > 0);
    let hello: serde_json::Value = serde_json::from_str(&frame).expect("json");
    let kept = hello["projects"].as_array().expect("projects").len();
    assert_eq!(kept + dropped, 200);
    assert_eq!(
        hello["projects"][0]["id"], "p0",
        "the first projects are kept"
    );
}

#[test]
fn a_hello_that_fits_is_sent_whole() {
    let offer = Offer {
        name: "machine".to_owned(),
        providers: Vec::new(),
        projects: Vec::new(),
    };
    let (frame, dropped) = offer.hello_frame("rt_0").expect("fits");
    assert_eq!(dropped, 0);
    assert_eq!(frame, offer.hello("rt_0").to_string());
}
