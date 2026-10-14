use serde::de::DeserializeOwned;
use serde_json::Value;

use super::*;

const CANONICAL_NULL_FIELDS: &[&str] = &[
    "projectCustomName",
    "projectCustomIconRevision",
    "workspaceName",
    "localProxyUrl",
    "publicProxyUrl",
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
fn workspace_wire_shapes_match_pinned_paseo_zod() {
    let fixtures: Value = serde_json::from_str(include_str!(
        "../../../../tests/fixtures/paseo-workspace.json"
    ))
    .unwrap();
    for case in fixtures["cases"].as_array().unwrap() {
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
            omit_added_nulls(&mut output, &case["output"]);
            assert_eq!(
                output, case["output"],
                "{} {}",
                case["schema"], case["name"]
            );
        }
    }
}
