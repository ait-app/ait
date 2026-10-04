//! Harness settings (adapter §4.5): what each provider declares in the hello, final
//! validation of a dispatch's `settings`, and how they map onto `agent.create`.
//!
//! Bonsai knows no setting; it renders this declaration and returns what members filled in.
//! Everything AIT's whitelist accepts is declared, except directories that would silently widen
//! what "needs approval" means (`add_dirs`, `writable_roots`).

use serde_json::{Map, Value, json};

use crate::wire::is_token;

/// Name of the injected Bonsai MCP server (no `__`, distinct from plugins and user servers).
pub const BONSAI_SERVER: &str = "bonsai_run";
/// Tools of Bonsai's MCP server, for `bonsai_preapprove`.
pub const BONSAI_TOOLS: [&str; 14] = [
    "list_spaces",
    "list_notes",
    "read_note",
    "search",
    "list_tasks",
    "list_changes",
    "capture",
    "create_note",
    "append_to_note",
    "replace_note",
    "add_task",
    "set_task_state",
    "set_metadata",
    "acknowledge_change",
];
const LIST_ITEMS: usize = 32;
const LIST_ITEM_BYTES: usize = 256;
const TEXT_BYTES: usize = 2048;

/// A permission mode a provider offers (`provider.modes.list`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mode {
    /// Mode ID passed as `modeId`.
    pub id: String,
    /// Display label.
    pub label: String,
}

/// What a model supports, for validating `effort` and `fast_mode`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModelTraits {
    /// Accepted `thinkingOptionId` values; empty when unknown.
    pub thinking: Vec<String>,
    /// Whether `fast_mode` is supported.
    pub fast: bool,
}

/// The validated result of a dispatch's settings.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Applied {
    /// Mode passed to AIT (always explicit).
    pub mode: String,
    /// Whether tool calls stop for approval with these settings.
    pub approvals: bool,
    /// `thinkingOptionId`.
    pub thinking: Option<String>,
    /// `featureValues.fast_mode`.
    pub fast: Option<bool>,
    /// Text appended to the system prompt after the preamble and wrapup.
    pub append_system_prompt: Option<String>,
    /// `providerOptions`.
    pub provider_options: Map<String, Value>,
    /// Pre-approve the injected Bonsai MCP server's tools.
    pub preapprove_bonsai: bool,
}

/// Default mode per provider when nothing is filled.
#[must_use]
pub fn default_mode(provider: &str) -> &'static str {
    if provider == "codex" {
        "auto"
    } else {
        "default"
    }
}

/// Whether a mode the owner switched to stops for approval, keeping the run's own codex
/// `approval_policy` / `sandbox_mode` overrides (they still apply after a mode change).
#[must_use]
pub fn mode_asks_with(provider: &str, mode: &str, settings: &Map<String, Value>) -> bool {
    match provider {
        "codex" => codex_asks(
            Some(mode),
            string_value(settings, "approval_policy"),
            string_value(settings, "sandbox_mode"),
        ),
        _ => mode_asks(provider, mode),
    }
}

/// Whether a mode alone, with no other settings, stops for human approval.
#[must_use]
pub fn mode_asks(provider: &str, mode: &str) -> bool {
    match provider {
        "codex" => codex_asks(Some(mode), None, None),
        _ => !matches!(mode, "bypassPermissions" | "auto"),
    }
}

fn codex_asks(
    mode: Option<&str>,
    approval_policy: Option<&str>,
    sandbox_mode: Option<&str>,
) -> bool {
    let mode = mode.unwrap_or("auto");
    let (approval, sandbox) = match mode {
        "auto" | "auto-review" => ("on-request", "workspace-write"),
        "full-access" => ("never", "danger-full-access"),
        _ => ("never", "read-only"),
    };
    let approval = approval_policy.unwrap_or(approval);
    let sandbox = sandbox_mode.unwrap_or(sandbox);
    let reviewer_is_human = mode != "auto-review";
    !(approval == "never"
        || (sandbox == "danger-full-access" && approval != "untrusted")
        || !reviewer_is_human)
}

/// Whether choosing this option alone means running without approval.
fn unattended(provider: &str, key: &str, value: &str) -> bool {
    match (provider, key) {
        (_, "permission_mode") => !mode_asks(provider, value),
        ("codex", "approval_policy") => value == "never",
        ("codex", "sandbox_mode") => value == "danger-full-access",
        _ => false,
    }
}

fn choice(
    provider: &str,
    key: &str,
    label: &str,
    help: &str,
    options: &[(&str, &str)],
    default: Option<&str>,
) -> Value {
    let options: Vec<Value> = options
        .iter()
        .filter(|(value, _)| is_token(value, 64))
        .map(|(value, label)| {
            let mut option = json!({"value": value, "label": label});
            if unattended(provider, key, value) {
                option["unattended"] = Value::Bool(true);
            }
            option
        })
        .collect();
    let mut setting =
        json!({"key": key, "label": label, "help": help, "type": "choice", "options": options});
    if let Some(default) = default {
        setting["default"] = Value::from(default);
    }
    setting
}

fn text(key: &str, label: &str, help: &str) -> Value {
    json!({"key": key, "label": label, "help": help, "type": "text", "multiline": true, "max_bytes": TEXT_BYTES})
}

fn flag(key: &str, label: &str, help: &str) -> Value {
    json!({"key": key, "label": label, "help": help, "type": "flag"})
}

fn list(key: &str, label: &str, help: &str) -> Value {
    json!({"key": key, "label": label, "help": help, "type": "list"})
}

/// The settings a provider declares in the hello.
///
/// # Arguments
///
/// * `provider` - `claude` or `codex`.
/// * `modes` - Modes reported by `provider.modes.list`; `permission_mode` is omitted when empty.
/// * `bonsai_write` - Whether the Bonsai MCP server is injected (enables `bonsai_preapprove`).
#[must_use]
pub fn declarations(provider: &str, modes: &[Mode], bonsai_write: bool) -> Vec<Value> {
    let mut settings = Vec::new();
    let modes: Vec<&Mode> = modes.iter().filter(|mode| is_token(&mode.id, 64)).collect();
    if !modes.is_empty() {
        let options: Vec<(&str, &str)> = modes
            .iter()
            .map(|mode| (mode.id.as_str(), mode.label.as_str()))
            .collect();
        let default = default_mode(provider);
        let default = modes
            .iter()
            .any(|mode| mode.id == default)
            .then_some(default);
        settings.push(choice(
            provider,
            "permission_mode",
            "权限模式",
            "这次会话里工具调用怎么批(官方 harness 的 permission mode)",
            &options,
            default,
        ));
    }
    let preapprove_help = "Bonsai 的 MCP 工具不经审批就跑(只放行 Bonsai 自己的工具)";
    if provider == "codex" {
        codex_declarations(provider, &mut settings);
    } else {
        claude_declarations(provider, &mut settings);
    }
    if bonsai_write {
        settings.push(flag(
            "bonsai_preapprove",
            "Bonsai 工具不用批",
            preapprove_help,
        ));
    }
    settings
}

/// Codex settings after `permission_mode` (AIT's codex option whitelist).
fn codex_declarations(provider: &str, settings: &mut Vec<Value>) {
    settings.push(choice(
        provider,
        "effort",
        "思考力度",
        "model_reasoning_effort;要模型支持",
        &[
            ("none", "不思考"),
            ("minimal", "最少"),
            ("low", "低"),
            ("medium", "中"),
            ("high", "高"),
            ("xhigh", "很高"),
            ("max", "最高"),
            ("ultra", "极限"),
        ],
        None,
    ));
    settings.push(flag(
        "fast_mode",
        "快速模式",
        "service tier fast;要模型支持",
    ));
    settings.push(text(
        "append_system_prompt",
        "追加的 system prompt",
        "追加在固定前言和收尾指令之后(developer instructions)",
    ));
    settings.push(choice(
        provider,
        "approval_policy",
        "审批策略",
        "盖过权限模式里的审批策略",
        // `untrusted` is left out: codex 0.156 refuses to load a configuration that has it.
        &[("on-request", "按需要问"), ("never", "从不问")],
        None,
    ));
    settings.push(choice(
        provider,
        "sandbox_mode",
        "沙箱",
        "盖过权限模式里的沙箱",
        &[
            ("read-only", "只读"),
            ("workspace-write", "可写工作区"),
            ("danger-full-access", "不设沙箱"),
        ],
        None,
    ));
    settings.push(flag(
        "network_access",
        "允许联网",
        "只在可写工作区的沙箱里生效",
    ));
    settings.push(choice(
        provider,
        "web_search",
        "网页搜索",
        "web_search",
        &[
            ("disabled", "关"),
            ("cached", "缓存"),
            ("indexed", "索引"),
            ("live", "实时"),
        ],
        None,
    ));
    settings.push(flag("multi_agent", "多 agent", "features.multi_agent_v2"));
}

/// Claude settings after `permission_mode` (AIT's claude option whitelist).
fn claude_declarations(provider: &str, settings: &mut Vec<Value>) {
    settings.push(choice(
        provider,
        "effort",
        "思考力度",
        "--effort / --thinking;off 只有部分模型支持",
        &[
            ("off", "关"),
            ("low", "低"),
            ("medium", "中"),
            ("high", "高"),
            ("xhigh", "很高"),
            ("max", "最高"),
            ("ultracode", "ultracode"),
        ],
        None,
    ));
    settings.push(flag("fast_mode", "快速模式", "只有支持的模型可用"));
    settings.push(text(
        "append_system_prompt",
        "追加的 system prompt",
        "追加在固定前言和收尾指令之后(--append-system-prompt)",
    ));
    settings.push(list(
        "allowed_tools",
        "不用批的工具",
        "列进去的工具不经审批就跑;每项一条规则,不能有逗号",
    ));
    settings.push(list(
        "disallowed_tools",
        "禁用的工具",
        "每项一条规则,不能有逗号",
    ));
    settings.push(list(
        "permissions_allow",
        "放行规则",
        "settings.permissions.allow:列进去的不经审批就跑",
    ));
    settings.push(list(
        "permissions_ask",
        "要问的规则",
        "settings.permissions.ask",
    ));
    settings.push(list(
        "permissions_deny",
        "拒绝规则",
        "settings.permissions.deny",
    ));
    settings.push(flag("sandbox", "沙箱", "sandbox.enabled"));
    settings.push(flag(
        "sandbox_auto_bash",
        "沙箱里的命令不问",
        "sandbox.autoAllowBashIfSandboxed:沙箱里的 Bash 不经审批就跑",
    ));
    settings.push(list(
        "sandbox_allowed_domains",
        "沙箱可访问的域名",
        "sandbox.network.allowedDomains",
    ));
    settings.push(list(
        "sandbox_allow_write",
        "沙箱可写的路径",
        "sandbox.filesystem.allowWrite",
    ));
}

fn declared<'a>(declarations: &'a [Value], key: &str) -> Option<&'a Value> {
    declarations.iter().find(|setting| setting["key"] == key)
}

fn string_value<'a>(settings: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    settings.get(key).and_then(Value::as_str)
}

/// Validate a dispatch's settings against the declaration and map them for `agent.create`.
///
/// # Arguments
///
/// * `provider` - Resolved provider.
/// * `declarations` - What this runtime declared for it.
/// * `settings` - The dispatch's filled settings.
/// * `model` - Traits of the model the run uses, when known.
///
/// # Errors
///
/// Returns a reason naming the offending setting: unknown keys, values that do not match the
/// declaration, combinations the model does not support, and options marked unattended whose
/// combination would still stop for approval.
pub fn apply(
    provider: &str,
    declarations: &[Value],
    settings: &Map<String, Value>,
    model: Option<&ModelTraits>,
) -> Result<Applied, String> {
    for (key, value) in settings {
        let setting = declared(declarations, key).ok_or_else(|| format!("不认识的设定:{key}"))?;
        check(key, setting, value)?;
    }
    let mode = string_value(settings, "permission_mode")
        .map(str::to_owned)
        .or_else(|| {
            declared(declarations, "permission_mode")
                .and_then(|setting| setting["default"].as_str().map(str::to_owned))
        })
        .unwrap_or_else(|| default_mode(provider).to_owned());
    let asks = match provider {
        "codex" => codex_asks(
            Some(&mode),
            string_value(settings, "approval_policy"),
            string_value(settings, "sandbox_mode"),
        ),
        _ => mode_asks(provider, &mode),
    };
    let chosen_unattended: Vec<String> = declarations
        .iter()
        .filter(|setting| setting["type"] == "choice")
        .filter_map(|setting| {
            let key = setting["key"].as_str()?;
            let value = string_value(settings, key).or_else(|| setting["default"].as_str())?;
            unattended(provider, key, value).then(|| format!("{key}={value}"))
        })
        .collect();
    if asks && !chosen_unattended.is_empty() {
        let overrides: Vec<String> = ["approval_policy", "sandbox_mode"]
            .iter()
            .filter_map(|key| string_value(settings, key).map(|value| format!("{key}={value}")))
            .filter(|setting| !chosen_unattended.contains(setting))
            .collect();
        let by = if overrides.is_empty() {
            String::new()
        } else {
            format!("被 {} 拉回了要审批:", overrides.join("、"))
        };
        return Err(format!(
            "{by}{} 标着不经审批运行,但和其余设定合起来仍会停下来等人批",
            chosen_unattended.join("、")
        ));
    }
    let thinking = string_value(settings, "effort").map(str::to_owned);
    if let (Some(thinking), Some(model)) = (&thinking, model)
        && !model.thinking.is_empty()
        && !model.thinking.contains(thinking)
    {
        return Err(format!("这个模型不支持 effort={thinking}"));
    }
    let fast = settings.get("fast_mode").and_then(Value::as_bool);
    if fast == Some(true) && model.is_some_and(|model| !model.fast) {
        return Err("这个模型不支持 fast_mode".to_owned());
    }
    Ok(Applied {
        mode,
        approvals: asks,
        thinking,
        fast,
        append_system_prompt: string_value(settings, "append_system_prompt")
            .filter(|text| !text.is_empty())
            .map(str::to_owned),
        provider_options: provider_options(provider, settings),
        preapprove_bonsai: settings.get("bonsai_preapprove").and_then(Value::as_bool) == Some(true),
    })
}

fn check(key: &str, setting: &Value, value: &Value) -> Result<(), String> {
    let invalid = || format!("设定 {key} 的值不合声明");
    match setting["type"].as_str() {
        Some("choice") => {
            let value = value.as_str().ok_or_else(invalid)?;
            let known = setting["options"]
                .as_array()
                .is_some_and(|options| options.iter().any(|option| option["value"] == value));
            known.then_some(()).ok_or_else(invalid)
        }
        Some("flag") => value.is_boolean().then_some(()).ok_or_else(invalid),
        Some("text") => {
            let text = value.as_str().ok_or_else(invalid)?;
            let limit = setting["max_bytes"]
                .as_u64()
                .and_then(|max| usize::try_from(max).ok())
                .unwrap_or(TEXT_BYTES);
            let single_line = setting["multiline"] != true && text.contains('\n');
            // AIT refuses NUL in prompts and option strings.
            (text.len() <= limit && !single_line && !text.contains('\0'))
                .then_some(())
                .ok_or_else(invalid)
        }
        Some("list") => {
            let items = value.as_array().ok_or_else(invalid)?;
            let comma_free = matches!(key, "allowed_tools" | "disallowed_tools");
            let valid = items.len() <= LIST_ITEMS
                && items.iter().all(|item| {
                    item.as_str().is_some_and(|item| {
                        !(item.len() > LIST_ITEM_BYTES
                            || item.is_empty()
                            || item.contains('\0')
                            || (comma_free && item.contains(',')))
                    })
                });
            valid.then_some(()).ok_or_else(invalid)
        }
        _ => Err(invalid()),
    }
}

fn provider_options(provider: &str, settings: &Map<String, Value>) -> Map<String, Value> {
    let mut options = Map::new();
    let get = |key: &str| settings.get(key).cloned();
    if provider == "codex" {
        for (key, target) in [
            ("approval_policy", "approval_policy"),
            ("sandbox_mode", "sandbox_mode"),
            ("web_search", "web_search"),
        ] {
            if let Some(value) = get(key) {
                options.insert(target.to_owned(), value);
            }
        }
        if let Some(value) = get("network_access") {
            options.insert(
                "sandbox_workspace_write".to_owned(),
                json!({"network_access": value}),
            );
        }
        if let Some(value) = get("multi_agent") {
            options.insert("features".to_owned(), json!({"multi_agent_v2": value}));
        }
        return options;
    }
    for (key, target) in [
        ("allowed_tools", "allowedTools"),
        ("disallowed_tools", "disallowedTools"),
    ] {
        if let Some(value) = get(key) {
            options.insert(target.to_owned(), value);
        }
    }
    let mut permissions = Map::new();
    for (key, target) in [
        ("permissions_allow", "allow"),
        ("permissions_ask", "ask"),
        ("permissions_deny", "deny"),
    ] {
        if let Some(value) = get(key) {
            permissions.insert(target.to_owned(), value);
        }
    }
    if !permissions.is_empty() {
        options.insert("settings".to_owned(), json!({"permissions": permissions}));
    }
    let mut sandbox = Map::new();
    if let Some(value) = get("sandbox") {
        sandbox.insert("enabled".to_owned(), value);
    }
    if let Some(value) = get("sandbox_auto_bash") {
        sandbox.insert("autoAllowBashIfSandboxed".to_owned(), value);
    }
    if let Some(value) = get("sandbox_allowed_domains") {
        sandbox.insert("network".to_owned(), json!({"allowedDomains": value}));
    }
    if let Some(value) = get("sandbox_allow_write") {
        sandbox.insert("filesystem".to_owned(), json!({"allowWrite": value}));
    }
    if !sandbox.is_empty() {
        options.insert("sandbox".to_owned(), Value::Object(sandbox));
    }
    options
}

/// `toolPolicy` pre-approving every tool of the injected Bonsai server.
#[must_use]
pub fn bonsai_tool_policy() -> Value {
    let preapproved: Vec<Value> = BONSAI_TOOLS
        .iter()
        .map(|tool| json!({"kind": "mcp", "server": BONSAI_SERVER, "tool": tool}))
        .collect();
    json!({"preapproved": preapproved})
}

#[cfg(test)]
mod tests;
