//! The current runtime decisions each built-in adapter declares on the port (ADR-299).

use std::ffi::OsString;
use std::path::Path;

use provider::testing::clients;

use super::configured;

/// Assemble the built-in adapters without searching for installed programs.
fn offline(root: &Path, transport: Option<&str>) -> provider::Providers {
    configured(root, |name| match name {
        "AIT_SERVER_ANTIGRAVITY_BIN" | "AIT_SERVER_DEEPSEEK_HARNESS_BIN" => {
            Some(root.join(name).into_os_string())
        }
        "AIT_SERVER_DEEPSEEK_HARNESS_TRANSPORT" => transport.map(OsString::from),
        _ => None,
    })
}

#[test]
fn harness_evidence_keeps_declared_sources_in_report_order() {
    let root = tempfile::tempdir().unwrap();
    let providers = offline(root.path(), None);

    let sources: Vec<_> = clients(&providers)
        .iter()
        .filter_map(|client| {
            client.harness_evidence().map(|source| {
                (
                    client.provider(),
                    source.program_variable,
                    source.default_program,
                    source.log_directory_variable,
                    format!("{:?}", source.native_logs),
                )
            })
        })
        .collect();

    let under = |root: &str, home: &str, logs: &str| {
        format!(
            "UnderRoot {{ root_variable: {root:?}, home_directory: {home:?}, subdirectory: {logs:?} }}"
        )
    };
    assert_eq!(
        sources,
        [
            (
                "opencode",
                "AIT_SERVER_OPENCODE_BIN",
                "opencode",
                "AIT_DIAGNOSTICS_OPENCODE_LOG_DIR",
                r#"ReportedByProgram(["debug", "paths", "log"])"#.to_owned(),
            ),
            (
                "codex",
                "AIT_SERVER_CODEX_BIN",
                "codex",
                "AIT_DIAGNOSTICS_CODEX_LOG_DIR",
                under("CODEX_HOME", ".codex", "log"),
            ),
            (
                "claude",
                "AIT_SERVER_CLAUDE_BIN",
                "claude",
                "AIT_DIAGNOSTICS_CLAUDE_LOG_DIR",
                under("CLAUDE_CONFIG_DIR", ".claude", "debug"),
            ),
            (
                "deepseek-harness",
                "AIT_SERVER_DEEPSEEK_HARNESS_BIN",
                "dsh",
                "AIT_DIAGNOSTICS_DEEPSEEK_HARNESS_LOG_DIR",
                "Unknown".to_owned(),
            ),
            (
                "antigravity",
                "AIT_SERVER_ANTIGRAVITY_BIN",
                "agy",
                "AIT_DIAGNOSTICS_ANTIGRAVITY_LOG_DIR",
                under("HOME", "", ".gemini/antigravity-cli/log"),
            ),
            (
                "pi",
                "AIT_SERVER_PI_BIN",
                "pi",
                "AIT_DIAGNOSTICS_PI_LOG_DIR",
                "Unknown".to_owned(),
            ),
        ]
    );
}

#[test]
fn catalog_presentation_keeps_the_legacy_labels_descriptions_and_default_modes() {
    let root = tempfile::tempdir().unwrap();
    for (transport, dsh_description) in [
        (None, "DeepSeek Harness native interactive Host"),
        (Some("acp"), "DeepSeek Harness via Agent Client Protocol"),
    ] {
        let providers = offline(root.path(), transport);
        let mut presented: Vec<_> = clients(&providers)
            .iter()
            .filter_map(|client| {
                client.catalog_presentation().map(|presentation| {
                    (
                        client.provider(),
                        presentation.label,
                        presentation.description,
                        format!("{:?}", presentation.default_mode),
                    )
                })
            })
            .collect();
        presented.sort_unstable();

        assert_eq!(
            presented,
            [
                (
                    "antigravity",
                    "Antigravity",
                    "Google Antigravity via the official AGY CLI",
                    r#"Fixed("default")"#.to_owned(),
                ),
                (
                    "claude",
                    "Claude",
                    "Anthropic's multi-tool assistant with MCP support, streaming, and deep reasoning",
                    r#"Fixed("auto")"#.to_owned(),
                ),
                (
                    "codex",
                    "Codex",
                    "OpenAI's Codex workspace agent with sandbox controls and optional network access",
                    r#"Fixed("auto")"#.to_owned(),
                ),
                (
                    "deepseek-harness",
                    "DeepSeek Harness",
                    dsh_description,
                    "Discovered".to_owned(),
                ),
                (
                    "minimax-code",
                    "MiniMax Code",
                    "MiniMax Code via Agent Client Protocol",
                    "Omitted".to_owned(),
                ),
                (
                    "omp",
                    "Oh My Pi",
                    "Oh My Pi native approvals, questions and subagents",
                    r#"Fixed("full")"#.to_owned(),
                ),
            ]
        );
    }
}

#[test]
fn session_decisions_keep_the_legacy_provider_behavior() {
    let root = tempfile::tempdir().unwrap();
    let providers = offline(root.path(), None);

    let mut decisions: Vec<_> = clients(&providers)
        .iter()
        .map(|client| {
            (
                client.provider(),
                client.continues_interrupted_turns(),
                client.live_subagents_are_complete(),
                client.imported_session_mode(),
            )
        })
        .collect();
    decisions.sort_unstable();

    assert_eq!(
        decisions,
        [
            ("antigravity", false, false, "read-only"),
            ("claude", false, false, "default"),
            ("codex", true, false, "read-only"),
            ("deepseek-harness", false, false, "read-only"),
            ("minimax-code", false, false, "read-only"),
            ("omp", false, false, "read-only"),
            ("opencode", false, true, "read-only"),
            ("pi", false, false, "read-only"),
        ]
    );
}
