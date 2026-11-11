//! Directed unit tests for native model-role discovery, validation and overlay materialization.

use super::*;
use serde_json::json;

fn catalog_json() -> (Value, Value, Value, Value) {
    let roles = json!({"key":"modelRoles","value":{
        "default":"test/model",
        "task":"test/model:high",
        "custom-role":"test/model"
    }});
    let tags = json!({"key":"modelTags","value":{"task":{"name":"Worker"}}});
    let cycle = json!({"key":"cycleOrder","value":["default","task"]});
    let models = json!({"models":[
        {"selector":"test/model","name":"Test","kind":"chat","input":["text"],
         "thinking":["low","high"]},
        {"selector":"test/tiny","name":"Tiny","kind":"tiny","input":["text"],"thinking":null},
        {"selector":"test/image","name":"Image","kind":"image","input":["text"],"thinking":null}
    ]});
    (roles, tags, cycle, models)
}

fn config_with(values: &[(&str, &str)]) -> StoredAgentConfig {
    let mut config = StoredAgentConfig::default();
    let map: BTreeMap<String, Value> = values
        .iter()
        .map(|(key, value)| ((*key).to_owned(), json!(value)))
        .collect();
    config.feature_values = Some(map);
    config
}

#[test]
fn discovery_union_includes_custom_role_beyond_any_fixed_list() {
    let (roles, tags, cycle, models) = catalog_json();
    let catalog = Catalog::parse(&roles, &tags, &cycle, &models).unwrap();
    let ids: Vec<&str> = catalog.roles.iter().map(|role| role.id.as_str()).collect();
    assert!(
        ids.contains(&"custom-role"),
        "discovered custom role must appear"
    );
    assert!(ids.contains(&"default") && ids.contains(&"task"));
    // Label comes from the native tag, not a hardcoded marketing name.
    let task = catalog.roles.iter().find(|role| role.id == "task").unwrap();
    assert_eq!(task.name, "Worker");
    let custom = catalog
        .roles
        .iter()
        .find(|role| role.id == "custom-role")
        .unwrap();
    assert_eq!(custom.name, "custom-role");
}

#[test]
fn discovery_rejects_oversized_role_pool() {
    let mut inherited = serde_json::Map::new();
    for index in 0..=MAX_ROLES {
        inherited.insert(format!("role{index}"), json!("test/model"));
    }
    let roles = json!({"key":"modelRoles","value":Value::Object(inherited)});
    let tags = json!({"key":"modelTags","value":{}});
    let cycle = json!({"key":"cycleOrder","value":[]});
    let models = json!({"models":[]});
    assert!(Catalog::parse(&roles, &tags, &cycle, &models).is_err());
}

#[test]
fn features_render_native_inherit_and_model_effort_options() {
    let (roles, tags, cycle, models) = catalog_json();
    let catalog = Catalog::parse(&roles, &tags, &cycle, &models).unwrap();
    let features = catalog.features(&StoredAgentConfig::default());
    let task = features
        .iter()
        .find(|feature| feature["id"] == "modelRole.task")
        .unwrap();
    assert_eq!(task["type"], "select");
    assert_eq!(task["value"], "native");
    let options = task["options"].as_array().unwrap();
    assert_eq!(options[0]["id"], "native");
    assert_eq!(options[0]["metadata"]["nativeSelector"], "test/model:high");
    assert!(options.iter().any(|option| option["id"] == "test/model"));
    assert!(
        options
            .iter()
            .any(|option| option["id"] == "test/model:high")
    );
    // Image-kind model is not offered to a chat role.
    assert!(!options.iter().any(|option| option["id"] == "test/image"));
}

#[test]
fn validate_accepts_known_assignment_and_rejects_unknown() {
    let (roles, tags, cycle, models) = catalog_json();
    let catalog = Catalog::parse(&roles, &tags, &cycle, &models).unwrap();
    let good = config_with(&[("modelRole.task", "test/model:high")]);
    assert!(catalog.validate(&good).is_ok());
    let unknown_model = config_with(&[("modelRole.task", "test/missing")]);
    assert!(catalog.validate(&unknown_model).is_err());
    let unknown_role = config_with(&[("modelRole.ghost", "test/model")]);
    assert!(catalog.validate(&unknown_role).is_err());
    let inherit = config_with(&[("modelRole.task", "native")]);
    assert!(catalog.validate(&inherit).is_ok());
}

#[test]
fn overrides_exclude_inherit_and_include_unset() {
    let config = config_with(&[
        ("modelRole.default", "native"),
        ("modelRole.task", "test/model:high"),
        ("modelRole.custom-role", "unset"),
        ("autoCompaction", "true"),
    ]);
    let overrides = overrides(&config);
    assert_eq!(overrides.len(), 2);
    assert_eq!(
        overrides.get("task").map(String::as_str),
        Some("test/model:high")
    );
    assert_eq!(
        overrides.get("custom-role").map(String::as_str),
        Some("unset")
    );
    assert!(!overrides.contains_key("default"));
}

#[test]
fn same_overlay_compares_only_role_assignments() {
    let a = config_with(&[("modelRole.task", "test/model:high")]);
    let b = config_with(&[
        ("modelRole.task", "test/model:high"),
        ("autoCompaction", "true"),
    ]);
    let c = config_with(&[("modelRole.task", "test/model:low")]);
    assert!(same_overlay(&a, &b));
    assert!(!same_overlay(&a, &c));
}

#[tokio::test]
async fn overlay_is_private_and_omits_inherited_roles() {
    let inherited = config_with(&[("modelRole.default", "native")]);
    assert!(
        overlay(&inherited).await.unwrap().is_none(),
        "no file when all inherited"
    );

    let config = config_with(&[
        ("modelRole.task", "test/model:high"),
        ("modelRole.custom-role", "unset"),
    ]);
    let file = overlay(&config).await.unwrap().expect("overlay file");
    let bytes = std::fs::read(file.path()).unwrap();
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(value["modelRoles"]["task"], "test/model:high");
    assert!(value["modelRoles"]["custom-role"].is_null());
    assert!(value["modelRoles"].get("default").is_none());
}

#[test]
fn stored_features_render_only_stored_keys_without_fixed_set() {
    let config = config_with(&[("modelRole.task", "test/model:high")]);
    let features = stored_features(&config);
    assert_eq!(features.len(), 1, "no builtin roles invented offline");
    assert_eq!(features[0]["id"], "modelRole.task");
    let options = features[0]["options"].as_array().unwrap();
    let stored = options
        .iter()
        .find(|option| option["id"] == "test/model:high")
        .unwrap();
    assert_eq!(stored["metadata"]["unresolved"], true);
}

#[test]
fn alias_resolution_is_cycle_guarded() {
    let roles = json!({"key":"modelRoles","value":{"a":"@b","b":"@a"}});
    let tags = json!({"key":"modelTags","value":{}});
    let cycle = json!({"key":"cycleOrder","value":["a","b"]});
    let models = json!({"models":[]});
    let catalog = Catalog::parse(&roles, &tags, &cycle, &models).unwrap();
    assert_eq!(
        catalog.resolve_alias("a"),
        None,
        "cyclic alias stays unresolved"
    );
}

#[test]
fn valid_bounds_shape_only_selector_shape_is_catalog_validated() {
    // `valid` is the bounded-shape gate (design §5); provider/id selector shape is enforced
    // by Catalog::validate against the live pool, not here.
    assert!(valid("modelRole.task", &json!("test/model:high")));
    assert!(valid("modelRole.task", &json!("native")));
    assert!(valid("modelRole.task", &json!("unset")));
    assert!(!valid("autoCompaction", &json!("true")));
    assert!(!valid("modelRole.task", &json!(true)));
    assert!(!valid("modelRole.task", &json!("")));
    let long = "a".repeat(MAX_SELECTOR + 1);
    assert!(!valid("modelRole.task", &json!(long)));
    let long_role = "r".repeat(MAX_ROLE_ID + 1);
    assert!(!valid(
        &format!("modelRole.{long_role}"),
        &json!("test/model")
    ));
}

#[test]
fn unresolved_assignments_survive_catalog_refresh_but_cannot_apply() {
    let (roles, tags, cycle, models) = catalog_json();
    let catalog = Catalog::parse(&roles, &tags, &cycle, &models).unwrap();
    let config = config_with(&[
        ("modelRole.task", "test/missing"),
        ("modelRole.ghost", "test/model"),
    ]);
    let features = catalog.features(&config);
    for (id, value) in [
        ("modelRole.task", "test/missing"),
        ("modelRole.ghost", "test/model"),
    ] {
        let feature = features.iter().find(|feature| feature["id"] == id).unwrap();
        assert_eq!(feature["value"], value);
        let option = feature["options"]
            .as_array()
            .unwrap()
            .iter()
            .find(|option| option["id"] == value)
            .unwrap();
        assert_eq!(option["metadata"]["unresolved"], true);
    }
    assert!(
        catalog
            .validate(&config_with(&[("modelRole.ghost", "unset")]))
            .is_err()
    );
    assert!(
        catalog
            .validate(&config_with(&[("modelRole.ghost", "native")]))
            .is_ok()
    );
}

#[test]
fn discovery_bounds_the_combined_role_union() {
    let roles = json!({"key":"modelRoles","value":{}});
    let tags = json!({"key":"modelTags","value":{}});
    let cycle = json!({"key":"cycleOrder","value":
        (0..MAX_ROLES).map(|index| format!("role{index}")).collect::<Vec<_>>()});
    let mut roles = roles;
    roles["value"]["extra"] = json!("test/model");
    assert!(Catalog::parse(&roles, &tags, &cycle, &json!({"models":[]})).is_err());
}

#[tokio::test]
async fn catalog_reader_waits_for_pending_discovery_instead_of_returning_empty() {
    use std::future::{Future, poll_fn};
    use std::sync::Arc;
    use std::task::Poll;

    let directory = tempfile::tempdir().unwrap();
    let cwd = directory.path().to_str().unwrap();
    let client = OmpClient::new(directory.path().join("missing-omp"));
    let gate = Arc::new(tokio::sync::Mutex::new(()));
    let held = gate.lock().await;
    client.role_catalogs.lock().unwrap().insert(
        client.role_catalog_key(cwd),
        super::super::RoleSnapshot {
            catalog: None,
            identity: Arc::new(()),
            fresh: false,
            gate: gate.clone(),
            updated: std::time::Instant::now(),
        },
    );
    let mut read = std::pin::pin!(client.role_catalog(cwd));
    poll_fn(|context| {
        assert!(read.as_mut().poll(context).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(held);
    assert!(read.await.is_err());
}

#[tokio::test]
async fn failed_refresh_preserves_last_good_catalog_for_its_identity() {
    use std::sync::Arc;

    let directory = tempfile::tempdir().unwrap();
    let cwd = directory.path().to_str().unwrap();
    let client = OmpClient::new(directory.path().join("missing-omp"));
    let (roles, tags, cycle, models) = catalog_json();
    let previous = Arc::new(Catalog::parse(&roles, &tags, &cycle, &models).unwrap());
    client.role_catalogs.lock().unwrap().insert(
        client.role_catalog_key(cwd),
        super::super::RoleSnapshot {
            catalog: Some(previous.clone()),
            identity: Arc::new(()),
            fresh: true,
            gate: Arc::new(tokio::sync::Mutex::new(())),
            updated: std::time::Instant::now(),
        },
    );
    client.invalidate_role_catalog(cwd);
    let retained = client.role_catalog(cwd).await.unwrap();
    assert!(
        retained
            .validate(&config_with(&[("modelRole.task", "test/model:high")]))
            .is_ok()
    );
    assert!(client.role_catalog("/different-identity").await.is_err());
}
