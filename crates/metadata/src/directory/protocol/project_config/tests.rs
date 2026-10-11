use serde_json::json;

use super::*;

#[test]
fn service_ports_reject_invalid_ranges_and_normalize_script_only_settings() {
    for invalid in [
        json!({"scripts":[]}),
        json!({"worktree":false}),
        json!({"worktree":{"servicePorts":[]}}),
        json!({"worktree":{"servicePorts":{"portScript":42}}}),
        json!({"worktree":{"servicePorts":{"portScript":"  "}}}),
    ] {
        assert!(PaseoConfigRaw::new(invalid.clone()).is_err(), "{invalid}");
    }
    for range in [
        "3000",
        "-3000",
        "3000-",
        "a-3000",
        "3000-b",
        "100000-100001",
        "1-100000",
        "0-1",
        "65535-65536",
    ] {
        assert!(
            PaseoConfigRaw::new(json!({"worktree":{"servicePorts":{"range":range}}})).is_err(),
            "{range}"
        );
    }
    let config = PaseoConfigRaw::new(json!({"worktree":{"servicePorts":{"portScript":"  echo 3000  "}},"metadataGeneration":false})).unwrap().value().clone();
    assert_eq!(
        config["worktree"]["servicePorts"]["portScript"],
        "echo 3000"
    );
    assert_eq!(config["metadataGeneration"], json!({}));
    let config = PaseoConfigRaw::new(json!({"metadataGeneration":{"title":false,"branchName":{"instructions":"keep"},"future":true}})).unwrap();
    assert_eq!(
        config.value()["metadataGeneration"],
        json!({"title":{},"branchName":{"instructions":"keep"},"future":true})
    );
}

#[test]
fn failed_reads_keep_the_requested_root_and_stable_inline_error() {
    assert_eq!(
        serde_json::to_value(ProjectConfigReadResult::Failure {
            repo_root: "/missing".to_owned(),
            error: ProjectConfigRpcError::ProjectNotFound,
        })
        .unwrap(),
        json!({"ok":false,"repoRoot":"/missing","error":{"code":"project_not_found"}})
    );
}

#[test]
fn raw_config_matches_paseo_known_field_validation_and_passthrough() {
    let parsed = PaseoConfigRaw::new(json!({
        "worktree": {
            "setup": ["npm ci"],
            "servicePorts": {"range":" 3000-4000 ", "portScript":" npm run port "},
            "future": true
        },
        "scripts":{"web":{"command":"npm run dev","future":1}},
        "metadataGeneration":{"title":{"instructions":42},"future":true},
        "future":{"nested":true}
    }))
    .unwrap();
    assert_eq!(
        parsed.value()["worktree"]["servicePorts"]["range"],
        "3000-4000"
    );
    assert_eq!(parsed.value()["metadataGeneration"]["title"], json!({}));
    assert_eq!(parsed.value()["future"], json!({"nested":true}));

    for invalid in [
        json!(null),
        json!({"worktree":{"setup":["ok", 2]}}),
        json!({"worktree":{"servicePorts":{}}}),
        json!({"worktree":{"servicePorts":{"range":"4000-3000"}}}),
        json!({"worktree":{"servicePorts":{"range":"3000-4000","extra":true}}}),
        json!({"scripts":{"web":"npm run dev"}}),
    ] {
        assert!(PaseoConfigRaw::new(invalid).is_err());
    }
}

#[test]
fn canonical_requests_use_camel_case_and_require_expected_revision() {
    let request: ProjectConfigWriteRequest = serde_json::from_value(json!({
        "repoRoot":"/repo",
        "config":{"worktree":{"setup":"npm ci"}},
        "expectedRevision":{"mtimeMs":12.5,"size":42}
    }))
    .unwrap();
    assert_eq!(request.repo_root, "/repo");
    assert!((request.expected_revision.unwrap().size - 42.0).abs() < f64::EPSILON);
    assert!(
        serde_json::from_value::<ProjectConfigWriteRequest>(json!({
            "repoRoot":"/repo", "config":{}
        }))
        .is_err()
    );
}

#[test]
fn result_boolean_discriminator_matches_paseo_payload() {
    assert_eq!(
        serde_json::to_value(ProjectConfigReadResult::Success {
            repo_root: "/repo".to_owned(),
            config: None,
            revision: None,
        })
        .unwrap(),
        json!({"ok":true,"repoRoot":"/repo","config":null,"revision":null})
    );
    assert_eq!(
        serde_json::to_value(ProjectConfigWriteResult::Failure {
            repo_root: "/repo".to_owned(),
            error: ProjectConfigRpcError::StaleProjectConfig {
                current_revision: None
            },
        })
        .unwrap(),
        json!({
            "ok":false,
            "repoRoot":"/repo",
            "error":{"code":"stale_project_config","currentRevision":null}
        })
    );
}
