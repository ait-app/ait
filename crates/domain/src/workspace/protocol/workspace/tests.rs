use serde::de::DeserializeOwned;
use serde_json::Value;

use super::*;

const CANONICAL_NULL_FIELDS: &[&str] = &[
    "projectCustomName",
    "projectCustomIconRevision",
    "workspaceName",
    "currentBranch",
    "remoteUrl",
    "isDirty",
    "aheadBehind",
    "aheadOfOrigin",
    "behindOfOrigin",
    "reviewDecision",
    "pullRequest",
    "error",
    "refreshedAt",
    "title",
    "pinnedAt",
    "diffStat",
    "gitRuntime",
    "githubRuntime",
];

const RETIRED_PROXY_FIELDS: &[&str] = &["proxyUrl", "localProxyUrl", "publicProxyUrl"];

fn omit_retired_proxy_fields(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            fields.retain(|name, _| !RETIRED_PROXY_FIELDS.contains(&name.as_str()));
            for value in fields.values_mut() {
                omit_retired_proxy_fields(value);
            }
        }
        Value::Array(values) => {
            for value in values {
                omit_retired_proxy_fields(value);
            }
        }
        _ => {}
    }
}

fn parse<T: DeserializeOwned + Serialize>(input: &Value) -> serde_json::Result<Value> {
    serde_json::from_value::<T>(input.clone()).and_then(serde_json::to_value)
}

fn omit_added_nulls(output: &mut Value, source_output: &Value) {
    match (output, source_output) {
        (Value::Object(output), Value::Object(source_output)) => {
            output.retain(|field, value| {
                if !source_output.contains_key(field) && value.is_null() {
                    assert!(CANONICAL_NULL_FIELDS.contains(&field.as_str()), "{field}");
                    false
                } else {
                    true
                }
            });
            for (field, value) in output {
                if let Some(source_value) = source_output.get(field) {
                    omit_added_nulls(value, source_value);
                }
            }
        }
        (Value::Array(output), Value::Array(source_output)) => {
            for (value, source_value) in output.iter_mut().zip(source_output) {
                omit_added_nulls(value, source_value);
            }
        }
        _ => {}
    }
}

#[test]
fn retained_workspace_wire_shapes_match_pinned_paseo_zod() {
    let fixtures: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/paseo-workspace.json"
    ))
    .unwrap();
    for case in fixtures["cases"].as_array().unwrap() {
        // Keep the pinned upstream evidence unchanged; its retired field validation
        // cases no longer describe Ait's script protocol.
        if case["schema"] == "WorkspaceScriptPayload"
            && case["name"]
                .as_str()
                .expect("fixture case name")
                .rsplit('_')
                .next()
                .is_some_and(|field| RETIRED_PROXY_FIELDS.contains(&field))
        {
            continue;
        }
        let result = match case["schema"].as_str().unwrap() {
            "ProjectCheckoutLitePayload" => parse::<ProjectCheckoutLitePayload>(&case["input"]),
            "ProjectPlacementPayload" => parse::<ProjectPlacementPayload>(&case["input"]),
            "WorkspaceScriptPayload" => parse::<WorkspaceScriptPayload>(&case["input"]),
            "WorkspaceDescriptorPayload" => parse::<WorkspaceDescriptorPayload>(&case["input"]),
            "WorkspaceProjectDescriptorPayload" => {
                parse::<WorkspaceProjectDescriptorPayload>(&case["input"])
            }
            "WorkspaceGitHubRuntimePayload" => {
                parse::<WorkspaceGitHubRuntimePayload>(&case["input"])
            }
            schema => panic!("unexpected fixture schema {schema}"),
        };
        assert_eq!(
            result.is_ok(),
            case["valid"].as_bool().unwrap(),
            "{} {}: {result:?}",
            case["schema"],
            case["name"]
        );
        if let Ok(mut output) = result {
            if case["schema"] == "ProjectPlacementPayload"
                && case["name"] == "missing_workspaceName"
            {
                assert!(output["workspaceName"].is_null());
            }
            let mut expected = case["output"].clone();
            omit_retired_proxy_fields(&mut expected);
            omit_added_nulls(&mut output, &expected);
            assert_eq!(output, expected, "{} {}", case["schema"], case["name"]);
        }
    }
}

#[test]
fn script_payload_does_not_expose_retired_proxy_fields() {
    let output = parse::<WorkspaceScriptPayload>(&serde_json::json!({
        "scriptName": "web",
        "type": "service",
        "hostname": "web",
        "port": 3000,
        "lifecycle": "running",
        "health": null,
        "proxyUrl": "retired",
        "localProxyUrl": "retired",
        "publicProxyUrl": "retired"
    }))
    .expect("current script payload");

    assert_eq!(output["port"], 3000);
    for field in RETIRED_PROXY_FIELDS {
        assert!(output.get(field).is_none(), "retired field {field}");
    }
}
