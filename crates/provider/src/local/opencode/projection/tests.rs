use super::*;

#[test]
fn shell_details_omit_absent_or_invalid_optional_fields() {
    for cwd in [Value::Null, json!(42), json!({})] {
        let projected = detail("bash", &json!({"command":"pwd","cwd":cwd}));
        assert_eq!(projected, json!({"type":"shell","command":"pwd"}));
    }
    assert_eq!(
        detail("bash", &json!({"command":"pwd"})),
        json!({"type":"shell","command":"pwd"})
    );
    assert_eq!(
        detail("bash", &json!({"command":"pwd","cwd":"/project"})),
        json!({"type":"shell","command":"pwd","cwd":"/project"})
    );
}

#[test]
fn invalid_required_tool_fields_fall_back_to_unknown_details() {
    for (name, field) in [
        ("bash", "command"),
        ("shell", "command"),
        ("read", "filePath"),
        ("edit", "filePath"),
        ("write", "filePath"),
    ] {
        for input in [json!({}), json!({field:null}), json!({field:42})] {
            assert_eq!(detail(name, &input)["type"], "unknown", "{name}: {input}");
        }
    }
}

#[test]
fn native_tool_history_matches_the_shared_frontend_contract() {
    use crate::local::opencode::{history, protocol::Version};
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/opencode-tool-history.json"
    ))
    .unwrap();
    for case in cases {
        let version = if case["version"] == 1 {
            Version::V1
        } else {
            Version::V2
        };
        let normalized =
            history::normalize(version, "ses_one", std::slice::from_ref(&case["message"])).unwrap();
        let entries = records(&normalized, &BTreeMap::new()).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].item, case["expected"], "{}", case["name"]);
    }
}

#[test]
fn legacy_null_tool_fields_are_replaced_on_history_refresh() {
    use crate::{
        local::opencode::{history, protocol::Version},
        storage::timeline::Timeline,
    };
    let cases: Vec<Value> = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/opencode-tool-history.json"
    ))
    .unwrap();
    for case in cases
        .iter()
        .filter(|case| case["expected"]["detail"]["type"] == "shell")
    {
        let version = if case["version"] == 1 {
            Version::V1
        } else {
            Version::V2
        };
        let normalized =
            history::normalize(version, "ses_one", std::slice::from_ref(&case["message"])).unwrap();
        let entries = records(&normalized, &BTreeMap::new()).unwrap();
        let mut legacy = entries[0].clone();
        legacy.key = legacy
            .key
            .replace("native:opencode:projection-v3:", "native:opencode:");
        legacy.item["detail"]["cwd"] = Value::Null;
        legacy.item["detail"]["output"] = Value::Null;
        let timeline = Timeline::memory().unwrap();
        let (before, _) = timeline.append("agent", "opencode", &[legacy]).unwrap();
        let after = timeline.reconcile("agent", "opencode", &entries).unwrap();
        assert_ne!(before, after);
        assert_eq!(
            timeline.read("agent").unwrap().1[0].entry.item,
            case["expected"]
        );
        assert_eq!(
            timeline.reconcile("agent", "opencode", &entries).unwrap(),
            after
        );
    }
}
