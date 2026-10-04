use std::collections::BTreeSet;

use serde_json::{Map, Value, json};

use super::{
    Applied, BONSAI_SERVER, BONSAI_TOOLS, Mode, ModelTraits, apply, bonsai_tool_policy,
    declarations, default_mode, mode_asks, provider_options,
};

const PROVIDERS: [&str; 2] = ["claude", "codex"];
/// Protocol §3: at most 32 settings per provider.
const SETTINGS_PER_PROVIDER: usize = 32;
/// Protocol §3: at most 32 options per choice.
const OPTIONS_PER_CHOICE: usize = 32;
/// Protocol §3: all providers' settings together, serialized, after cleaning.
const SETTINGS_TOTAL_BYTES: usize = 64 * 1024;
/// Protocol §3: cleaning can grow the settings up to three times their size.
const CLEANING_GROWTH: usize = 3;
const CODEX_MODES: [Option<&str>; 4] =
    [None, Some("auto"), Some("auto-review"), Some("full-access")];
const APPROVAL_POLICIES: [Option<&str>; 3] = [None, Some("on-request"), Some("never")];
const SANDBOX_MODES: [Option<&str>; 4] = [
    None,
    Some("read-only"),
    Some("workspace-write"),
    Some("danger-full-access"),
];
const CLAUDE_MODES: [Option<&str>; 6] = [
    None,
    Some("default"),
    Some("plan"),
    Some("acceptEdits"),
    Some("auto"),
    Some("bypassPermissions"),
];

fn mode(id: &str, label: &str) -> Mode {
    Mode {
        id: id.to_owned(),
        label: label.to_owned(),
    }
}

/// The modes `provider.modes.list` reports today (AIT `claude/config.rs` `modes()`; codex
/// `controls.rs` `modes()` plus `auto-review` on codex 0.115 or later).
fn modes_of(provider: &str) -> Vec<Mode> {
    if provider == "codex" {
        vec![
            mode("auto", "Default Permissions"),
            mode("auto-review", "Auto-review"),
            mode("full-access", "Full Access"),
        ]
    } else {
        vec![
            mode("default", "Always Ask"),
            mode("plan", "Plan Mode"),
            mode("acceptEdits", "Accept File Edits"),
            mode("auto", "Auto mode"),
            mode("bypassPermissions", "Bypass"),
        ]
    }
}

/// The richest declaration: every mode and the Bonsai MCP server injected.
fn full(provider: &str) -> Vec<Value> {
    declarations(provider, &modes_of(provider), true)
}

/// Every declaration a hello can carry, labelled for assertion messages.
fn every_declaration() -> Vec<(String, Vec<Value>)> {
    let mut all = Vec::new();
    for provider in PROVIDERS {
        for modes in [modes_of(provider), Vec::new()] {
            for bonsai_write in [false, true] {
                let label = format!(
                    "{provider} modes={} bonsai_write={bonsai_write}",
                    modes.len()
                );
                all.push((label, declarations(provider, &modes, bonsai_write)));
            }
        }
    }
    all
}

fn settings(pairs: &[(&str, Value)]) -> Map<String, Value> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), value.clone()))
        .collect()
}

fn key_of(setting: &Value) -> &str {
    setting["key"].as_str().unwrap_or_default()
}

fn keys(declared: &[Value]) -> Vec<&str> {
    declared.iter().map(key_of).collect()
}

fn find<'a>(declared: &'a [Value], key: &str) -> Option<&'a Value> {
    declared.iter().find(|setting| setting["key"] == key)
}

fn options(setting: &Value) -> &[Value] {
    setting["options"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default()
}

fn option_values(setting: &Value) -> Vec<&str> {
    options(setting)
        .iter()
        .filter_map(|option| option["value"].as_str())
        .collect()
}

fn keys_of_type<'a>(declared: &'a [Value], kind: &str) -> Vec<&'a str> {
    declared
        .iter()
        .filter(|setting| setting["type"] == kind)
        .map(key_of)
        .collect()
}

/// `^[a-z][a-z0-9_.-]{0,63}$` (protocol §3).
fn is_key_shape(key: &str) -> bool {
    let mut bytes = key.bytes();
    key.len() <= 64
        && bytes.next().is_some_and(|first| first.is_ascii_lowercase())
        && bytes.all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'.' | b'-')
        })
}

/// `^[A-Za-z0-9._:-]{1,64}$` (protocol §3, the shape of a provider id).
fn is_option_shape(value: &str) -> bool {
    (1..=64).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b':' | b'-'))
}

/// Already what the Hub would store: no control characters, no surrounding whitespace.
fn is_clean(text: &str) -> bool {
    text == text.trim() && !text.chars().any(char::is_control)
}

/// Every `key=value` whose option is marked `unattended`.
fn unattended_marks(declared: &[Value]) -> BTreeSet<String> {
    declared
        .iter()
        .flat_map(|setting| {
            options(setting)
                .iter()
                .filter(|option| option["unattended"] == true)
                .map(move |option| {
                    format!(
                        "{}={}",
                        key_of(setting),
                        option["value"].as_str().unwrap_or_default()
                    )
                })
        })
        .collect()
}

/// The `key=value` pairs Bonsai's `unattendedOf` sees in use (protocol §4.2): each choice's
/// filled value, or its declared default when unfilled, whose option is marked `unattended`.
fn unattended_in_use(declared: &[Value], chosen: &Map<String, Value>) -> Vec<String> {
    declared
        .iter()
        .filter(|setting| setting["type"] == "choice")
        .filter_map(|setting| {
            let key = key_of(setting);
            let used = chosen
                .get(key)
                .and_then(Value::as_str)
                .or_else(|| setting["default"].as_str())?;
            options(setting)
                .iter()
                .any(|option| option["value"] == used && option["unattended"] == true)
                .then(|| format!("{key}={used}"))
        })
        .collect()
}

/// Whether the effective codex policy stops for a human (adapter §4.5 rule 1): `auto-review`
/// hands approvals to codex's reviewer; `never` never asks; `untrusted` always asks; otherwise
/// only a sandbox asks for escalation.
fn codex_effective_asks(mode: Option<&str>, approval: Option<&str>, sandbox: Option<&str>) -> bool {
    let mode = mode.unwrap_or("auto");
    if mode == "auto-review" {
        return false;
    }
    let (base_approval, base_sandbox) = if mode == "full-access" {
        ("never", "danger-full-access")
    } else {
        ("on-request", "workspace-write")
    };
    let sandbox = sandbox.unwrap_or(base_sandbox);
    match approval.unwrap_or(base_approval) {
        "never" => false,
        "untrusted" => true,
        _ => sandbox != "danger-full-access",
    }
}

fn codex_cell(
    mode: Option<&str>,
    approval: Option<&str>,
    sandbox: Option<&str>,
) -> Map<String, Value> {
    let mut chosen = Map::new();
    for (key, value) in [
        ("permission_mode", mode),
        ("approval_policy", approval),
        ("sandbox_mode", sandbox),
    ] {
        if let Some(value) = value {
            chosen.insert(key.to_owned(), Value::from(value));
        }
    }
    chosen
}

fn cell_label(mode: Option<&str>, approval: Option<&str>, sandbox: Option<&str>) -> String {
    format!(
        "{}|{}|{}",
        mode.unwrap_or("unset"),
        approval.unwrap_or("unset"),
        sandbox.unwrap_or("unset")
    )
}

/// Assert one combination: rejected exactly when it still asks while an unattended option is in
/// use; otherwise the reported approvals agree with Bonsai's `unattendedOf` and the real policy.
/// Returns whether the cell was rejected.
fn assert_cell(
    provider: &str,
    declared: &[Value],
    chosen: &Map<String, Value>,
    asks: bool,
    cell: &str,
) -> bool {
    let in_use = unattended_in_use(declared, chosen);
    let result = apply(provider, declared, chosen, None);
    if asks && !in_use.is_empty() {
        let reason = result.expect_err(cell);
        for pair in &in_use {
            assert!(
                reason.contains(pair.as_str()),
                "{cell}: {reason} should name {pair}"
            );
        }
        true
    } else {
        let applied = result.expect(cell);
        assert_eq!(
            applied.approvals,
            in_use.is_empty(),
            "{cell}: approvals vs unattendedOf"
        );
        assert_eq!(
            applied.approvals, asks,
            "{cell}: approvals vs effective policy"
        );
        false
    }
}

fn rejection(provider: &str, pairs: &[(&str, Value)]) -> String {
    match apply(provider, &full(provider), &settings(pairs), None) {
        Ok(applied) => panic!("{provider} {pairs:?} should be rejected, got {applied:?}"),
        Err(reason) => reason,
    }
}

fn accepted(provider: &str, pairs: &[(&str, Value)]) -> Applied {
    match apply(provider, &full(provider), &settings(pairs), None) {
        Ok(applied) => applied,
        Err(reason) => panic!("{provider} {pairs:?} should be accepted: {reason}"),
    }
}

fn traits(thinking: &[&str], fast: bool) -> ModelTraits {
    ModelTraits {
        thinking: thinking.iter().map(|id| (*id).to_owned()).collect(),
        fast,
    }
}

#[test]
fn every_key_matches_the_protocol_shape_and_is_unique() {
    for (label, declared) in every_declaration() {
        // Arrange
        let declared_keys = keys(&declared);

        // Act
        let unique: BTreeSet<&str> = declared_keys.iter().copied().collect();

        // Assert
        assert_eq!(unique.len(), declared_keys.len(), "{label}: duplicate keys");
        for key in declared_keys {
            assert!(is_key_shape(key), "{label}: key {key:?}");
        }
    }
}

#[test]
fn each_provider_declares_at_most_32_settings() {
    for (label, declared) in every_declaration() {
        // Arrange + Act
        let count = declared.len();

        // Assert
        assert!(count <= SETTINGS_PER_PROVIDER, "{label}: {count} settings");
    }
}

#[test]
fn claude_declares_the_documented_keys_and_types_in_order() {
    // Arrange
    let expected = [
        ("permission_mode", "choice"),
        ("effort", "choice"),
        ("fast_mode", "flag"),
        ("append_system_prompt", "text"),
        ("allowed_tools", "list"),
        ("disallowed_tools", "list"),
        ("permissions_allow", "list"),
        ("permissions_ask", "list"),
        ("permissions_deny", "list"),
        ("sandbox", "flag"),
        ("sandbox_auto_bash", "flag"),
        ("sandbox_allowed_domains", "list"),
        ("sandbox_allow_write", "list"),
        ("bonsai_preapprove", "flag"),
    ];

    // Act
    let declared = full("claude");
    let actual: Vec<(&str, &str)> = declared
        .iter()
        .map(|setting| {
            (
                key_of(setting),
                setting["type"].as_str().unwrap_or_default(),
            )
        })
        .collect();

    // Assert
    assert_eq!(actual, expected);
}

#[test]
fn codex_declares_the_documented_keys_and_types_in_order() {
    // Arrange
    let expected = [
        ("permission_mode", "choice"),
        ("effort", "choice"),
        ("fast_mode", "flag"),
        ("append_system_prompt", "text"),
        ("approval_policy", "choice"),
        ("sandbox_mode", "choice"),
        ("network_access", "flag"),
        ("web_search", "choice"),
        ("multi_agent", "flag"),
        ("bonsai_preapprove", "flag"),
    ];

    // Act
    let declared = full("codex");
    let actual: Vec<(&str, &str)> = declared
        .iter()
        .map(|setting| {
            (
                key_of(setting),
                setting["type"].as_str().unwrap_or_default(),
            )
        })
        .collect();

    // Assert
    assert_eq!(actual, expected);
}

#[test]
fn fixed_choices_offer_the_options_the_ait_whitelist_accepts() {
    // Arrange
    let expected = [
        (
            "claude",
            "effort",
            vec!["off", "low", "medium", "high", "xhigh", "max", "ultracode"],
        ),
        (
            "codex",
            "effort",
            vec![
                "none", "minimal", "low", "medium", "high", "xhigh", "max", "ultra",
            ],
        ),
        ("codex", "approval_policy", vec!["on-request", "never"]),
        (
            "codex",
            "sandbox_mode",
            vec!["read-only", "workspace-write", "danger-full-access"],
        ),
        (
            "codex",
            "web_search",
            vec!["disabled", "cached", "indexed", "live"],
        ),
    ];

    for (provider, key, values) in expected {
        // Act
        let declared = full(provider);
        let setting = find(&declared, key).expect("the choice is declared");

        // Assert
        assert_eq!(option_values(setting), values, "{provider} {key}");
        assert!(
            setting.get("default").is_none(),
            "{provider} {key} has no default"
        );
    }
}

#[test]
fn every_setting_has_a_known_type_and_well_formed_fields() {
    for (label, declared) in every_declaration() {
        for setting in &declared {
            // Arrange
            let key = key_of(setting);

            // Act
            let kind = setting["type"].as_str().unwrap_or_default();

            // Assert
            match kind {
                "choice" => {
                    let values = option_values(setting);
                    assert!(!values.is_empty(), "{label} {key}: no options");
                    assert!(
                        values.len() <= OPTIONS_PER_CHOICE,
                        "{label} {key}: too many options"
                    );
                    assert_eq!(
                        values.len(),
                        options(setting).len(),
                        "{label} {key}: option without value"
                    );
                    for value in &values {
                        assert!(is_option_shape(value), "{label} {key}: option {value:?}");
                    }
                    let unique: BTreeSet<&&str> = values.iter().collect();
                    assert_eq!(
                        unique.len(),
                        values.len(),
                        "{label} {key}: duplicate options"
                    );
                    if let Some(default) = setting.get("default") {
                        let default = default.as_str().expect("a choice default is a string");
                        assert!(
                            values.contains(&default),
                            "{label} {key}: default {default:?}"
                        );
                    }
                }
                "text" => {
                    let max = setting["max_bytes"]
                        .as_u64()
                        .expect("text declares max_bytes");
                    assert!((1..=2048).contains(&max), "{label} {key}: max_bytes {max}");
                    assert!(
                        setting["multiline"].is_boolean(),
                        "{label} {key}: multiline"
                    );
                }
                "flag" | "list" => {}
                other => panic!("{label} {key}: unknown type {other:?}"),
            }
        }
    }
}

#[test]
fn labels_and_help_fit_the_hub_limits_and_need_no_cleaning() {
    for (label, declared) in every_declaration() {
        for setting in &declared {
            // Arrange
            let key = key_of(setting);

            // Act
            let name = setting["label"]
                .as_str()
                .expect("every setting has a label");
            let help = setting["help"].as_str().unwrap_or_default();

            // Assert
            assert!(
                !name.is_empty() && name.len() <= 128,
                "{label} {key}: label {name:?}"
            );
            assert!(
                help.len() <= 512,
                "{label} {key}: help is {} bytes",
                help.len()
            );
            assert!(
                is_clean(name) && is_clean(help),
                "{label} {key}: label or help needs cleaning"
            );
            for option in options(setting) {
                let option_label = option["label"].as_str().expect("every option has a label");
                assert!(
                    !option_label.is_empty() && option_label.len() <= 128 && is_clean(option_label),
                    "{label} {key}: option label {option_label:?}"
                );
            }
        }
    }
}

#[test]
fn all_providers_settings_fit_the_64_kib_total_even_after_cleaning() {
    // Arrange
    let declared: Vec<Value> = PROVIDERS.into_iter().flat_map(full).collect();

    // Act
    let total: usize = declared
        .iter()
        .map(|setting| {
            serde_json::to_vec(setting)
                .expect("a setting serializes")
                .len()
        })
        .sum();

    // Assert
    assert!(
        total * CLEANING_GROWTH <= SETTINGS_TOTAL_BYTES,
        "{total} bytes"
    );
}

#[test]
fn directories_that_widen_what_needs_approval_are_never_declared() {
    for (label, declared) in every_declaration() {
        // Arrange + Act
        let declared_keys = keys(&declared);

        // Assert
        for key in declared_keys {
            assert!(
                !matches!(key, "add_dirs" | "writable_roots"),
                "{label}: {key}"
            );
            assert!(
                !key.contains("dir") && !key.contains("writable"),
                "{label}: {key}"
            );
        }
    }
}

#[test]
fn add_dirs_and_writable_roots_are_rejected_when_sent_anyway() {
    for (provider, key) in [
        ("claude", "add_dirs"),
        ("claude", "writable_roots"),
        ("codex", "add_dirs"),
        ("codex", "writable_roots"),
    ] {
        // Arrange + Act
        let reason = rejection(provider, &[(key, json!(["/"]))]);

        // Assert
        assert!(reason.contains(key), "{provider}: {reason}");
    }
}

#[test]
fn bonsai_preapprove_is_declared_only_when_bonsai_writes() {
    for provider in PROVIDERS {
        for modes in [modes_of(provider), Vec::new()] {
            // Arrange + Act
            let with = declarations(provider, &modes, true);
            let without = declarations(provider, &modes, false);

            // Assert
            let setting = find(&with, "bonsai_preapprove").expect("declared with bonsai_write");
            assert_eq!(setting["type"], "flag");
            assert!(find(&without, "bonsai_preapprove").is_none(), "{provider}");
            assert_eq!(
                with.len(),
                without.len() + 1,
                "{provider}: only that one setting differs"
            );
        }
    }
}

#[test]
fn bonsai_preapprove_is_an_unknown_key_without_bonsai_writes() {
    for provider in PROVIDERS {
        // Arrange
        let declared = declarations(provider, &modes_of(provider), false);
        let chosen = settings(&[("bonsai_preapprove", json!(true))]);

        // Act
        let reason = apply(provider, &declared, &chosen, None).expect_err("not declared");

        // Assert
        assert!(reason.contains("bonsai_preapprove"), "{provider}: {reason}");
    }
}

#[test]
fn permission_mode_options_mirror_the_given_modes_in_order() {
    for provider in PROVIDERS {
        // Arrange
        let modes = modes_of(provider);

        // Act
        let declared = declarations(provider, &modes, false);

        // Assert
        let setting = find(&declared, "permission_mode").expect("declared when modes exist");
        assert_eq!(setting["type"], "choice");
        let actual: Vec<(&str, &str)> = options(setting)
            .iter()
            .map(|option| {
                (
                    option["value"].as_str().unwrap_or_default(),
                    option["label"].as_str().unwrap_or_default(),
                )
            })
            .collect();
        let expected: Vec<(&str, &str)> = modes
            .iter()
            .map(|mode| (mode.id.as_str(), mode.label.as_str()))
            .collect();
        assert_eq!(actual, expected, "{provider}");
    }
}

#[test]
fn permission_mode_offers_only_the_modes_reported_on_this_machine() {
    // Arrange: an older codex without auto-review.
    let modes = vec![
        mode("auto", "Default Permissions"),
        mode("full-access", "Full Access"),
    ];
    let declared = declarations("codex", &modes, false);

    // Act
    let reason = apply(
        "codex",
        &declared,
        &settings(&[("permission_mode", json!("auto-review"))]),
        None,
    )
    .expect_err("auto-review is not offered");

    // Assert
    let setting = find(&declared, "permission_mode").expect("declared");
    assert_eq!(option_values(setting), ["auto", "full-access"]);
    assert!(reason.contains("permission_mode"), "{reason}");
}

#[test]
fn permission_mode_is_omitted_when_no_modes_are_reported() {
    for provider in PROVIDERS {
        // Arrange + Act
        let declared = declarations(provider, &[], true);

        // Assert
        assert!(find(&declared, "permission_mode").is_none(), "{provider}");
        let reason = apply(
            provider,
            &declared,
            &settings(&[("permission_mode", json!(default_mode(provider)))]),
            None,
        )
        .expect_err("permission_mode is not declared");
        assert!(reason.contains("permission_mode"), "{provider}: {reason}");
    }
}

#[test]
fn permission_mode_default_is_the_provider_default_only_when_offered() {
    for provider in PROVIDERS {
        // Arrange
        let offered = modes_of(provider);
        let missing: Vec<Mode> = offered
            .iter()
            .filter(|mode| mode.id != default_mode(provider))
            .cloned()
            .collect();

        // Act
        let with = declarations(provider, &offered, false);
        let without = declarations(provider, &missing, false);

        // Assert
        let setting = find(&with, "permission_mode").expect("declared");
        assert_eq!(setting["default"], default_mode(provider), "{provider}");
        let setting = find(&without, "permission_mode").expect("declared");
        assert!(
            setting.get("default").is_none(),
            "{provider}: no default when not offered"
        );
    }
}

#[test]
fn mode_ids_that_are_not_option_shaped_are_left_out() {
    // Arrange
    let long = "a".repeat(65);
    let modes = vec![
        mode("default", "Always Ask"),
        mode("has space", "Spaced"),
        mode("", "Empty"),
        mode(&long, "Long"),
        mode("auto", "Auto mode"),
    ];

    // Act
    let declared = declarations("claude", &modes, false);

    // Assert
    let setting = find(&declared, "permission_mode").expect("declared");
    assert_eq!(option_values(setting), ["default", "auto"]);
    assert_eq!(setting["default"], "default");
}

#[test]
fn unattended_marks_exactly_the_options_that_run_without_approval() {
    // Arrange
    let expected_claude: BTreeSet<String> =
        ["permission_mode=bypassPermissions", "permission_mode=auto"]
            .into_iter()
            .map(str::to_owned)
            .collect();
    let expected_codex: BTreeSet<String> = [
        "permission_mode=full-access",
        "permission_mode=auto-review",
        "approval_policy=never",
        "sandbox_mode=danger-full-access",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();

    // Act
    let claude = unattended_marks(&full("claude"));
    let codex = unattended_marks(&full("codex"));

    // Assert
    assert_eq!(claude, expected_claude);
    assert_eq!(codex, expected_codex);
}

#[test]
fn unattended_is_only_ever_written_as_true() {
    for (label, declared) in every_declaration() {
        for setting in &declared {
            for option in options(setting) {
                // Arrange + Act
                let mark = option.get("unattended");

                // Assert
                assert!(
                    mark.is_none() || mark == Some(&Value::Bool(true)),
                    "{label}: {option}"
                );
            }
        }
    }
}

#[test]
fn without_modes_only_the_codex_overrides_are_marked_unattended() {
    // Arrange
    let expected: BTreeSet<String> = ["approval_policy=never", "sandbox_mode=danger-full-access"]
        .into_iter()
        .map(str::to_owned)
        .collect();

    // Act
    let claude = unattended_marks(&declarations("claude", &[], true));
    let codex = unattended_marks(&declarations("codex", &[], true));

    // Assert
    assert!(claude.is_empty(), "{claude:?}");
    assert_eq!(codex, expected);
}

#[test]
fn preapproval_help_says_the_listed_items_skip_approval() {
    for (provider, key) in [
        ("claude", "allowed_tools"),
        ("claude", "permissions_allow"),
        ("claude", "sandbox_auto_bash"),
        ("claude", "bonsai_preapprove"),
        ("codex", "bonsai_preapprove"),
    ] {
        // Arrange
        let declared = full(provider);

        // Act
        let help = find(&declared, key)
            .and_then(|setting| setting["help"].as_str())
            .unwrap_or_default();

        // Assert
        assert!(help.contains("不经审批"), "{provider} {key}: {help:?}");
    }
}

#[test]
fn append_system_prompt_is_a_multiline_text_of_2048_bytes() {
    for provider in PROVIDERS {
        // Arrange + Act
        let declared = full(provider);
        let setting = find(&declared, "append_system_prompt").expect("declared");

        // Assert
        assert_eq!(setting["type"], "text", "{provider}");
        assert_eq!(setting["multiline"], true, "{provider}");
        assert_eq!(setting["max_bytes"], 2048, "{provider}");
    }
}

#[test]
fn codex_every_combination_agrees_with_bonsai_unattended_of() {
    // Arrange
    let declared = full("codex");
    let mut rejected = 0;

    // Act + Assert
    for mode in CODEX_MODES {
        for approval in APPROVAL_POLICIES {
            for sandbox in SANDBOX_MODES {
                let chosen = codex_cell(mode, approval, sandbox);
                let asks = codex_effective_asks(mode, approval, sandbox);
                let cell = cell_label(mode, approval, sandbox);
                if assert_cell("codex", &declared, &chosen, asks, &cell) {
                    rejected += 1;
                }
            }
        }
    }
    assert_eq!(rejected, 2);
}

#[test]
fn codex_rejects_exactly_the_documented_contradictions() {
    // Arrange
    let declared = full("codex");
    let expected: BTreeSet<String> = [
        "full-access|on-request|read-only",
        "full-access|on-request|workspace-write",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect();

    // Act
    let mut rejected = BTreeSet::new();
    for mode in CODEX_MODES {
        for approval in APPROVAL_POLICIES {
            for sandbox in SANDBOX_MODES {
                let chosen = codex_cell(mode, approval, sandbox);
                if apply("codex", &declared, &chosen, None).is_err() {
                    rejected.insert(cell_label(mode, approval, sandbox));
                }
            }
        }
    }

    // Assert
    assert_eq!(rejected, expected);
}

#[test]
fn codex_without_declared_modes_agrees_with_bonsai_unattended_of() {
    // Arrange
    let declared = declarations("codex", &[], false);
    let mut rejected = Vec::new();

    // Act + Assert
    for approval in APPROVAL_POLICIES {
        for sandbox in SANDBOX_MODES {
            let chosen = codex_cell(None, approval, sandbox);
            let asks = codex_effective_asks(None, approval, sandbox);
            let cell = cell_label(None, approval, sandbox);
            if assert_cell("codex", &declared, &chosen, asks, &cell) {
                rejected.push(cell);
            }
        }
    }
    assert!(rejected.is_empty(), "{rejected:?}");
}

#[test]
fn untrusted_is_not_offered_because_codex_refuses_to_load_it() {
    // codex 0.156: "approval_policy = \"untrusted\" is no longer supported; remove this setting".
    let declared = full("codex");
    let settings: Map<String, Value> = json!({"approval_policy": "untrusted"})
        .as_object()
        .cloned()
        .expect("object");
    let error = apply("codex", &declared, &settings, None).expect_err("undeclared value");
    assert!(error.contains("approval_policy"), "{error}");
}

#[test]
fn codex_applied_mode_is_always_explicit() {
    for mode in CODEX_MODES {
        // Arrange
        let chosen = codex_cell(mode, None, None);

        // Act
        let applied =
            apply("codex", &full("codex"), &chosen, None).expect("a mode alone is accepted");

        // Assert
        assert_eq!(applied.mode, mode.unwrap_or("auto"));
    }
}

#[test]
fn claude_every_permission_mode_agrees_with_bonsai_unattended_of() {
    // Arrange
    let declared = full("claude");

    for mode in CLAUDE_MODES {
        let chosen = codex_cell(mode, None, None);
        let asks = !matches!(mode.unwrap_or("default"), "auto" | "bypassPermissions");
        let cell = format!("claude {}", mode.unwrap_or("unset"));

        // Act + Assert
        let rejected = assert_cell("claude", &declared, &chosen, asks, &cell);
        assert!(!rejected, "{cell}: no claude mode contradicts itself");
        let applied = apply("claude", &declared, &chosen, None).expect("accepted");
        assert_eq!(applied.mode, mode.unwrap_or("default"), "{cell}");
    }
}

#[test]
fn preapproval_lists_still_report_approvals() {
    // Arrange
    let pairs = [
        ("allowed_tools", json!(["Bash(git status)", "Read"])),
        ("permissions_allow", json!(["Edit"])),
        ("sandbox", json!(true)),
        ("sandbox_auto_bash", json!(true)),
        ("bonsai_preapprove", json!(true)),
    ];

    // Act
    let claude = accepted("claude", &pairs);
    let codex = accepted("codex", &[("bonsai_preapprove", json!(true))]);

    // Assert
    assert!(claude.approvals);
    assert!(claude.preapprove_bonsai);
    assert!(codex.approvals);
    assert!(codex.preapprove_bonsai);
}

#[test]
fn approvals_with_nothing_filled_match_the_hello() {
    for provider in PROVIDERS {
        for modes in [modes_of(provider), Vec::new()] {
            // Arrange
            let declared = declarations(provider, &modes, true);

            // Act
            let applied = apply(provider, &declared, &Map::new(), None).expect("empty settings");

            // Assert
            assert_eq!(
                applied.approvals,
                mode_asks(provider, default_mode(provider)),
                "{provider}"
            );
            assert!(
                applied.approvals,
                "{provider}: the default stops for approval"
            );
            assert_eq!(applied.mode, default_mode(provider), "{provider}");
        }
    }
}

#[test]
fn empty_settings_apply_to_the_provider_defaults() {
    for provider in PROVIDERS {
        // Arrange
        let expected = Applied {
            mode: default_mode(provider).to_owned(),
            approvals: true,
            ..Applied::default()
        };

        // Act
        let applied = accepted(provider, &[]);

        // Assert
        assert_eq!(applied, expected, "{provider}");
    }
}

#[test]
fn a_missing_default_mode_falls_back_to_the_provider_default() {
    // Arrange: a claude that does not offer `default`.
    let modes = vec![
        mode("plan", "Plan Mode"),
        mode("bypassPermissions", "Bypass"),
    ];
    let declared = declarations("claude", &modes, false);

    // Act
    let applied = apply("claude", &declared, &Map::new(), None).expect("empty settings");

    // Assert
    assert_eq!(applied.mode, "default");
    assert!(applied.approvals);
}

#[test]
fn every_declared_option_alone_is_accepted() {
    for provider in PROVIDERS {
        // Arrange
        let declared = full(provider);

        for setting in declared
            .iter()
            .filter(|setting| setting["type"] == "choice")
        {
            for value in option_values(setting) {
                let chosen = settings(&[(key_of(setting), json!(value))]);

                // Act
                let result = apply(provider, &declared, &chosen, None);

                // Assert
                assert!(
                    result.is_ok(),
                    "{provider} {}={value}: {result:?}",
                    key_of(setting)
                );
            }
        }
    }
}

#[test]
fn unknown_keys_are_rejected_by_name() {
    for (provider, key, value) in [
        ("claude", "approval_policy", json!("never")),
        ("claude", "thinking", json!("high")),
        ("claude", "system_prompt", json!("replace")),
        ("codex", "allowed_tools", json!(["Bash"])),
        ("codex", "plan_mode", json!(true)),
        ("codex", "Permission_mode", json!("auto")),
    ] {
        // Arrange + Act
        let reason = rejection(provider, &[(key, value)]);

        // Assert
        assert!(reason.contains(key), "{provider} {key}: {reason}");
    }
}

#[test]
fn choice_values_outside_the_options_are_rejected_by_name() {
    for provider in PROVIDERS {
        // Arrange
        let declared = full(provider);

        for key in keys_of_type(&declared, "choice") {
            for value in [
                json!("not-an-option"),
                json!(""),
                json!(1),
                json!(true),
                json!(["auto"]),
                Value::Null,
            ] {
                let chosen = settings(&[(key, value.clone())]);

                // Act
                let reason = apply(provider, &declared, &chosen, None).expect_err("not an option");

                // Assert
                assert!(reason.contains(key), "{provider} {key}={value}: {reason}");
            }
        }
    }
}

#[test]
fn choice_values_are_case_sensitive() {
    for (provider, key, value) in [
        ("claude", "permission_mode", "Default"),
        ("claude", "permission_mode", "bypasspermissions"),
        ("codex", "permission_mode", "Full-Access"),
        ("codex", "approval_policy", "NEVER"),
    ] {
        // Arrange + Act
        let reason = rejection(provider, &[(key, json!(value))]);

        // Assert
        assert!(reason.contains(key), "{provider} {key}={value}: {reason}");
    }
}

#[test]
fn flags_that_are_not_booleans_are_rejected_by_name() {
    for provider in PROVIDERS {
        // Arrange
        let declared = full(provider);

        for key in keys_of_type(&declared, "flag") {
            for value in [
                json!("true"),
                json!(1),
                json!(0),
                Value::Null,
                json!([]),
                json!({}),
            ] {
                let chosen = settings(&[(key, value.clone())]);

                // Act
                let reason = apply(provider, &declared, &chosen, None).expect_err("not a boolean");

                // Assert
                assert!(reason.contains(key), "{provider} {key}={value}: {reason}");
            }
            for value in [true, false] {
                let chosen = settings(&[(key, json!(value))]);
                assert!(
                    apply(provider, &declared, &chosen, None).is_ok(),
                    "{provider} {key}={value}"
                );
            }
        }
    }
}

#[test]
fn text_over_max_bytes_is_rejected_by_name() {
    for provider in PROVIDERS {
        // Arrange
        let at_limit = "a".repeat(2048);
        let over = "a".repeat(2049);

        // Act
        let applied = accepted(provider, &[("append_system_prompt", json!(at_limit))]);
        let reason = rejection(provider, &[("append_system_prompt", json!(over))]);

        // Assert
        assert_eq!(
            applied.append_system_prompt.as_deref(),
            Some(at_limit.as_str()),
            "{provider}"
        );
        assert!(
            reason.contains("append_system_prompt"),
            "{provider}: {reason}"
        );
    }
}

#[test]
fn text_limit_counts_utf8_bytes_not_characters() {
    // Arrange: 683 three-byte characters are 2049 bytes; 682 are 2046.
    let over = "设".repeat(683);
    let under = "设".repeat(682);

    // Act
    let reason = rejection("claude", &[("append_system_prompt", json!(over))]);
    let applied = accepted("claude", &[("append_system_prompt", json!(under))]);

    // Assert
    assert!(reason.contains("append_system_prompt"), "{reason}");
    assert_eq!(
        applied.append_system_prompt.as_deref(),
        Some(under.as_str())
    );
}

#[test]
fn text_that_is_not_a_string_is_rejected_by_name() {
    for value in [json!(1), json!(true), json!(["a"]), Value::Null] {
        // Arrange + Act
        let reason = rejection("codex", &[("append_system_prompt", value.clone())]);

        // Assert
        assert!(reason.contains("append_system_prompt"), "{value}: {reason}");
    }
}

#[test]
fn multiline_text_keeps_its_newlines() {
    // Arrange
    let text = "first line\nsecond line";

    // Act
    let applied = accepted("claude", &[("append_system_prompt", json!(text))]);

    // Assert
    assert_eq!(applied.append_system_prompt.as_deref(), Some(text));
}

#[test]
fn a_newline_in_single_line_text_is_rejected_by_name() {
    // Arrange
    let declared = [json!({"key": "note", "label": "Note", "type": "text", "max_bytes": 64})];

    // Act
    let reason = apply(
        "claude",
        &declared,
        &settings(&[("note", json!("a\nb"))]),
        None,
    )
    .expect_err("single-line text with a newline");
    let single = apply(
        "claude",
        &declared,
        &settings(&[("note", json!("ab"))]),
        None,
    );

    // Assert
    assert!(reason.contains("note"), "{reason}");
    assert!(single.is_ok(), "{single:?}");
}

#[test]
fn a_declared_max_bytes_below_the_default_is_enforced() {
    // Arrange
    let declared = [
        json!({"key": "note", "label": "Note", "type": "text", "multiline": true, "max_bytes": 4}),
    ];

    // Act
    let reason = apply(
        "claude",
        &declared,
        &settings(&[("note", json!("abcde"))]),
        None,
    )
    .expect_err("over the declared limit");
    let fits = apply(
        "claude",
        &declared,
        &settings(&[("note", json!("abcd"))]),
        None,
    );

    // Assert
    assert!(reason.contains("note"), "{reason}");
    assert!(fits.is_ok(), "{fits:?}");
}

#[test]
fn a_declaration_of_unknown_type_rejects_its_value() {
    // Arrange
    let declared = [json!({"key": "count", "label": "Count", "type": "number"})];

    // Act
    let reason = apply("claude", &declared, &settings(&[("count", json!(3))]), None)
        .expect_err("unknown declaration type");

    // Assert
    assert!(reason.contains("count"), "{reason}");
}

#[test]
fn an_empty_append_system_prompt_is_not_passed_on() {
    // Arrange + Act
    let applied = accepted("codex", &[("append_system_prompt", json!(""))]);

    // Assert
    assert_eq!(applied.append_system_prompt, None);
}

#[test]
fn lists_over_32_items_are_rejected_by_name() {
    // Arrange
    let declared = full("claude");
    let fits: Vec<String> = (0..32).map(|index| format!("Tool{index}")).collect();
    let over: Vec<String> = (0..33).map(|index| format!("Tool{index}")).collect();

    for key in keys_of_type(&declared, "list") {
        // Act
        let reason = apply("claude", &declared, &settings(&[(key, json!(over))]), None)
            .expect_err("33 items");
        let at_limit = apply("claude", &declared, &settings(&[(key, json!(fits))]), None);

        // Assert
        assert!(reason.contains(key), "{key}: {reason}");
        assert!(at_limit.is_ok(), "{key}: {at_limit:?}");
    }
}

#[test]
fn list_items_over_256_bytes_are_rejected_by_name() {
    // Arrange
    let declared = full("claude");
    let fits = "a".repeat(256);
    let over = "a".repeat(257);

    for key in keys_of_type(&declared, "list") {
        // Act
        let reason = apply(
            "claude",
            &declared,
            &settings(&[(key, json!([over]))]),
            None,
        )
        .expect_err("257-byte item");
        let at_limit = apply(
            "claude",
            &declared,
            &settings(&[(key, json!([fits]))]),
            None,
        );

        // Assert
        assert!(reason.contains(key), "{key}: {reason}");
        assert!(at_limit.is_ok(), "{key}: {at_limit:?}");
    }
}

#[test]
fn empty_and_non_string_list_items_are_rejected_by_name() {
    // Arrange
    let declared = full("claude");

    for key in keys_of_type(&declared, "list") {
        for value in [
            json!([""]),
            json!(["Read", ""]),
            json!([1]),
            json!([null]),
            json!("Read"),
            json!({}),
        ] {
            // Act
            let reason = apply(
                "claude",
                &declared,
                &settings(&[(key, value.clone())]),
                None,
            )
            .expect_err("bad list");

            // Assert
            assert!(reason.contains(key), "{key}={value}: {reason}");
        }
    }
}

#[test]
fn commas_are_rejected_only_in_the_tool_lists_joined_by_ait() {
    // Arrange
    let rule = json!(["Bash(git log:*),Read"]);

    for key in ["allowed_tools", "disallowed_tools"] {
        // Act
        let reason = rejection("claude", &[(key, rule.clone())]);

        // Assert
        assert!(reason.contains(key), "{key}: {reason}");
    }
    for key in [
        "permissions_allow",
        "permissions_ask",
        "permissions_deny",
        "sandbox_allowed_domains",
        "sandbox_allow_write",
    ] {
        // Act
        let applied = accepted("claude", &[(key, rule.clone())]);

        // Assert
        assert!(!applied.provider_options.is_empty(), "{key}");
    }
}

#[test]
fn effort_outside_the_models_thinking_options_is_rejected_by_name() {
    // Arrange
    let declared = full("claude");
    let model = traits(&["low", "medium", "high"], false);
    let chosen = settings(&[("effort", json!("off"))]);

    // Act
    let reason =
        apply("claude", &declared, &chosen, Some(&model)).expect_err("off is not supported");

    // Assert
    assert!(reason.contains("effort"), "{reason}");
}

#[test]
fn effort_within_the_models_thinking_options_is_passed_on() {
    for provider in PROVIDERS {
        // Arrange
        let model = traits(&["low", "high"], false);
        let chosen = settings(&[("effort", json!("high"))]);

        // Act
        let applied = apply(provider, &full(provider), &chosen, Some(&model)).expect("supported");

        // Assert
        assert_eq!(applied.thinking.as_deref(), Some("high"), "{provider}");
    }
}

#[test]
fn effort_is_not_checked_when_the_models_options_are_unknown() {
    for model in [None, Some(traits(&[], false))] {
        // Arrange
        let chosen = settings(&[("effort", json!("off"))]);

        // Act
        let applied =
            apply("claude", &full("claude"), &chosen, model.as_ref()).expect("unknown options");

        // Assert
        assert_eq!(applied.thinking.as_deref(), Some("off"), "{model:?}");
    }
}

#[test]
fn fast_mode_on_a_model_without_fast_support_is_rejected_by_name() {
    for provider in PROVIDERS {
        // Arrange
        let model = traits(&[], false);
        let chosen = settings(&[("fast_mode", json!(true))]);

        // Act
        let reason =
            apply(provider, &full(provider), &chosen, Some(&model)).expect_err("no fast support");

        // Assert
        assert!(reason.contains("fast_mode"), "{provider}: {reason}");
    }
}

#[test]
fn fast_mode_is_passed_on_when_supported_off_or_unknown() {
    for provider in PROVIDERS {
        // Arrange
        let slow = traits(&[], false);
        let fast = traits(&[], true);
        let declared = full(provider);

        // Act
        let off_on_slow = apply(
            provider,
            &declared,
            &settings(&[("fast_mode", json!(false))]),
            Some(&slow),
        )
        .expect("turning it off is always fine");
        let on_on_fast = apply(
            provider,
            &declared,
            &settings(&[("fast_mode", json!(true))]),
            Some(&fast),
        )
        .expect("supported");
        let on_unknown = apply(
            provider,
            &declared,
            &settings(&[("fast_mode", json!(true))]),
            None,
        )
        .expect("unknown model");

        // Assert
        assert_eq!(off_on_slow.fast, Some(false), "{provider}");
        assert_eq!(on_on_fast.fast, Some(true), "{provider}");
        assert_eq!(on_unknown.fast, Some(true), "{provider}");
    }
}

#[test]
fn claude_settings_map_onto_agent_create() {
    // Arrange
    let pairs = [
        ("permission_mode", json!("acceptEdits")),
        ("effort", json!("max")),
        ("fast_mode", json!(true)),
        ("append_system_prompt", json!("Be brief.")),
        ("allowed_tools", json!(["Read"])),
        ("disallowed_tools", json!(["WebFetch"])),
        ("permissions_allow", json!(["Bash(ls:*)"])),
        ("permissions_ask", json!(["Bash(git push:*)"])),
        ("permissions_deny", json!(["Bash(rm:*)"])),
        ("sandbox", json!(true)),
        ("sandbox_auto_bash", json!(false)),
        ("sandbox_allowed_domains", json!(["example.com"])),
        ("sandbox_allow_write", json!(["/tmp/out"])),
        ("bonsai_preapprove", json!(true)),
    ];
    let expected_options = json!({
        "allowedTools": ["Read"],
        "disallowedTools": ["WebFetch"],
        "settings": {"permissions": {
            "allow": ["Bash(ls:*)"],
            "ask": ["Bash(git push:*)"],
            "deny": ["Bash(rm:*)"],
        }},
        "sandbox": {
            "enabled": true,
            "autoAllowBashIfSandboxed": false,
            "network": {"allowedDomains": ["example.com"]},
            "filesystem": {"allowWrite": ["/tmp/out"]},
        },
    });

    // Act
    let applied = accepted("claude", &pairs);

    // Assert
    assert_eq!(applied.mode, "acceptEdits");
    assert!(applied.approvals);
    assert_eq!(applied.thinking.as_deref(), Some("max"));
    assert_eq!(applied.fast, Some(true));
    assert_eq!(applied.append_system_prompt.as_deref(), Some("Be brief."));
    assert!(applied.preapprove_bonsai);
    assert_eq!(Value::Object(applied.provider_options), expected_options);
}

#[test]
fn claude_provider_options_leave_out_groups_nobody_filled() {
    for (pairs, expected) in [
        (vec![], json!({})),
        (
            vec![("permissions_ask", json!(["Bash(git push:*)"]))],
            json!({"settings": {"permissions": {"ask": ["Bash(git push:*)"]}}}),
        ),
        (
            vec![("sandbox_allow_write", json!(["/tmp/out"]))],
            json!({"sandbox": {"filesystem": {"allowWrite": ["/tmp/out"]}}}),
        ),
        (
            vec![("disallowed_tools", json!(["Bash"]))],
            json!({"disallowedTools": ["Bash"]}),
        ),
        (
            vec![
                ("permission_mode", json!("plan")),
                ("effort", json!("low")),
                ("fast_mode", json!(false)),
            ],
            json!({}),
        ),
    ] {
        // Arrange + Act
        let applied = accepted("claude", &pairs);

        // Assert
        assert_eq!(
            Value::Object(applied.provider_options),
            expected,
            "{pairs:?}"
        );
    }
}

#[test]
fn codex_settings_map_onto_agent_create() {
    // Arrange
    let pairs = [
        ("permission_mode", json!("auto")),
        ("effort", json!("minimal")),
        ("fast_mode", json!(true)),
        ("append_system_prompt", json!("Be brief.")),
        ("approval_policy", json!("on-request")),
        ("sandbox_mode", json!("workspace-write")),
        ("network_access", json!(true)),
        ("web_search", json!("cached")),
        ("multi_agent", json!(false)),
        ("bonsai_preapprove", json!(true)),
    ];
    let expected_options = json!({
        "approval_policy": "on-request",
        "sandbox_mode": "workspace-write",
        "web_search": "cached",
        "sandbox_workspace_write": {"network_access": true},
        "features": {"multi_agent_v2": false},
    });

    // Act
    let applied = accepted("codex", &pairs);

    // Assert
    assert_eq!(applied.mode, "auto");
    assert!(applied.approvals);
    assert_eq!(applied.thinking.as_deref(), Some("minimal"));
    assert_eq!(applied.fast, Some(true));
    assert_eq!(applied.append_system_prompt.as_deref(), Some("Be brief."));
    assert!(applied.preapprove_bonsai);
    assert_eq!(Value::Object(applied.provider_options), expected_options);
}

#[test]
fn codex_provider_options_carry_only_what_was_filled() {
    for (pairs, expected) in [
        (vec![], json!({})),
        (
            vec![("web_search", json!("live"))],
            json!({"web_search": "live"}),
        ),
        (
            vec![("network_access", json!(false))],
            json!({"sandbox_workspace_write": {"network_access": false}}),
        ),
        (
            vec![("multi_agent", json!(true))],
            json!({"features": {"multi_agent_v2": true}}),
        ),
        (
            vec![
                ("permission_mode", json!("full-access")),
                ("effort", json!("high")),
            ],
            json!({}),
        ),
    ] {
        // Arrange + Act
        let applied = accepted("codex", &pairs);

        // Assert
        assert_eq!(
            Value::Object(applied.provider_options),
            expected,
            "{pairs:?}"
        );
    }
}

#[test]
fn provider_options_ignore_keys_of_the_other_provider() {
    // Arrange
    let claude_only = settings(&[("allowed_tools", json!(["Read"])), ("sandbox", json!(true))]);
    let codex_only = settings(&[
        ("approval_policy", json!("never")),
        ("multi_agent", json!(true)),
    ]);

    // Act
    let codex = provider_options("codex", &claude_only);
    let claude = provider_options("claude", &codex_only);

    // Assert
    assert!(codex.is_empty(), "{codex:?}");
    assert!(claude.is_empty(), "{claude:?}");
}

#[test]
fn bonsai_tool_policy_preapproves_all_14_bonsai_tools() {
    // Arrange
    let expected: BTreeSet<&str> = [
        "acknowledge_change",
        "add_task",
        "append_to_note",
        "capture",
        "create_note",
        "list_changes",
        "list_notes",
        "list_spaces",
        "list_tasks",
        "read_note",
        "replace_note",
        "search",
        "set_metadata",
        "set_task_state",
    ]
    .into_iter()
    .collect();

    // Act
    let policy = bonsai_tool_policy();

    // Assert
    let preapproved = policy["preapproved"]
        .as_array()
        .expect("preapproved is a list");
    assert_eq!(preapproved.len(), 14);
    for entry in preapproved {
        assert_eq!(entry["kind"], "mcp", "{entry}");
        assert_eq!(entry["server"], "bonsai_run", "{entry}");
    }
    let tools: Vec<&str> = preapproved
        .iter()
        .filter_map(|entry| entry["tool"].as_str())
        .collect();
    assert_eq!(tools, BONSAI_TOOLS);
    let unique: BTreeSet<&str> = tools.into_iter().collect();
    assert_eq!(unique, expected);
}

#[test]
fn bonsai_server_name_is_usable_in_a_tool_policy() {
    // Arrange + Act
    let name = BONSAI_SERVER;

    // Assert
    assert_eq!(name, "bonsai_run");
    assert!(!name.contains("__"));
    assert!(is_key_shape(name));
}

#[test]
fn default_mode_is_explicit_per_provider() {
    // Arrange + Act + Assert
    assert_eq!(default_mode("codex"), "auto");
    assert_eq!(default_mode("claude"), "default");
}

#[test]
fn mode_asks_only_for_modes_that_stop_for_a_human() {
    // Arrange
    let expected = [
        ("claude", "default", true),
        ("claude", "plan", true),
        ("claude", "acceptEdits", true),
        ("claude", "auto", false),
        ("claude", "bypassPermissions", false),
        ("codex", "auto", true),
        ("codex", "auto-review", false),
        ("codex", "full-access", false),
        ("codex", "read-only", false),
    ];

    for (provider, mode, asks) in expected {
        // Act
        let actual = mode_asks(provider, mode);

        // Assert
        assert_eq!(actual, asks, "{provider} {mode}");
    }
}

#[test]
fn a_mode_alone_reports_the_same_approvals_as_mode_asks() {
    for provider in PROVIDERS {
        // Arrange
        let declared = full(provider);

        for mode in modes_of(provider) {
            let chosen = settings(&[("permission_mode", json!(mode.id))]);

            // Act
            let applied =
                apply(provider, &declared, &chosen, None).expect("a mode alone is accepted");

            // Assert
            assert_eq!(
                applied.approvals,
                mode_asks(provider, &mode.id),
                "{provider} {}",
                mode.id
            );
        }
    }
}

#[test]
fn nul_characters_are_rejected_before_ait_refuses_them() {
    let claude = declarations("claude", &modes_of("claude"), false);
    for (key, value) in [
        ("append_system_prompt", json!("before\u{0}after")),
        ("allowed_tools", json!(["Read\u{0}"])),
    ] {
        let mut settings = Map::new();
        settings.insert(key.to_owned(), value);
        let error = apply("claude", &claude, &settings, None).expect_err("NUL is refused");
        assert!(error.contains(key), "{error}");
    }
}

#[test]
fn a_mode_list_without_valid_ids_declares_no_permission_mode() {
    let broken = vec![mode("has space", "Broken"), mode("", "Empty")];
    let declared = declarations("claude", &broken, false);
    assert!(
        declared
            .iter()
            .all(|setting| setting["key"] != "permission_mode")
    );
}

#[test]
fn a_contradiction_names_the_setting_that_pulled_it_back() {
    let codex = declarations("codex", &modes_of("codex"), false);
    let settings: Map<String, Value> = json!({
        "permission_mode": "full-access",
        "approval_policy": "on-request",
        "sandbox_mode": "read-only",
    })
    .as_object()
    .cloned()
    .expect("object");
    let error = apply("codex", &codex, &settings, None).expect_err("contradiction");
    assert!(error.contains("approval_policy=on-request"), "{error}");
    assert!(error.contains("sandbox_mode=read-only"), "{error}");
    assert!(error.contains("permission_mode=full-access"), "{error}");
}
