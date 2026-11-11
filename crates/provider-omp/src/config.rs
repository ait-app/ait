use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use domain::agent_runtime::{StoredAgentConfig, StoredAgentRuntimeInfo};
use serde_json::{Value, json};

use super::{
    PROVIDER,
    transport::{Timeouts, Transport},
};
use provider::ports::agent_session::{AgentSessionError, AgentSessionSpec};
use provider::protocol::provider::Details;

/// OMP's thinking levels in Paseo's display order: id, label and description.
/// `--thinking` accepts exactly these ids.
const THINKING_LEVELS: [(&str, &str, &str); 8] = [
    ("auto", "Auto", "OMP picks the effort each turn"),
    ("off", "Off", "No extra reasoning"),
    ("minimal", "Minimal", "Light reasoning"),
    ("low", "Low", "Faster reasoning"),
    ("medium", "Medium", "Balanced reasoning"),
    ("high", "High", "Deeper reasoning"),
    ("xhigh", "XHigh", "Extra-high reasoning"),
    ("max", "Max", "Maximum reasoning"),
];
/// Default for reasoning models that report no efforts of their own.
const DEFAULT_THINKING: &str = "medium";
/// Feature id of OMP's live fast-mode toggle.
const FAST_MODE: &str = "fastMode";
/// Shown when Fast is on but the current model ignores it, as Paseo shows.
const FAST_INACTIVE: &str = "Fast is enabled but does not apply to this model.";
/// Boolean feature keys older releases persisted; accepted and ignored.
const LEGACY_FEATURES: [&str; 3] = ["autoCompaction", "autoRetry", "slowMode"];
/// Paseo's model-role provider options and the OMP flag each becomes, in Paseo's argv order.
const MODEL_ROLES: [(&str, &str); 3] = [
    ("smolModel", "--smol"),
    ("slowModel", "--slow"),
    ("planModel", "--plan"),
];
/// Paseo's provider option bounding both the wait for `ready` and each request.
const RPC_TIMEOUT: &str = "rpcTimeoutMs";
/// Longest accepted `rpcTimeoutMs`: Paseo's Node timers cannot wait longer.
const MAX_RPC_TIMEOUT_MS: u64 = 2_147_483_647;

pub(super) fn validate(config: &StoredAgentConfig) -> Result<(), AgentSessionError> {
    if config
        .mode_id
        .as_deref()
        .is_some_and(|mode| !matches!(mode, "full" | "write" | "ask" | "ask-cached"))
        || config
            .provider_options
            .as_ref()
            .is_some_and(|options| !provider_options(options))
        || config
            .mcp_servers
            .as_ref()
            .is_some_and(|values| !values.is_empty())
        || config
            .tool_policy
            .as_ref()
            .is_some_and(|value| !value.is_null())
        || config
            .system_prompt
            .as_ref()
            .is_some_and(|value| value.len() > 65536 || value.contains('\0'))
        || config.thinking_option_id.as_deref().is_some_and(|value| {
            value.is_empty() || value.len() > 128 || value.chars().any(char::is_control)
        })
        || config.feature_values.iter().flatten().any(|(key, value)| {
            if key == "transport" {
                return !matches!(value.as_str(), Some("acp" | "rpc"));
            }
            (key != FAST_MODE && !LEGACY_FEATURES.contains(&key.as_str())) || !value.is_boolean()
        })
    {
        return Err(AgentSessionError::Rejected);
    }
    if let Some(model) = &config.model {
        model_route(model)?;
    }
    Ok(())
}

/// Whether `options` hold only Paseo's OMP options with valid values. As Paseo's strict schema
/// does, any other key is refused, including Paseo's `sessionDir`: Ait's listing and import
/// ports carry no provider options, and OMP itself honours only `PI_CODING_AGENT_SESSION_DIR`,
/// which the session-file lookup already reads from the agent's environment.
fn provider_options(options: &BTreeMap<String, Value>) -> bool {
    options.iter().all(|(key, value)| {
        if key == RPC_TIMEOUT {
            return value
                .as_u64()
                .is_some_and(|millis| (1..=MAX_RPC_TIMEOUT_MS).contains(&millis));
        }
        MODEL_ROLES.iter().any(|(name, _)| name == key)
            && value.as_str().is_some_and(|model| {
                !model.trim().is_empty()
                    && model.len() <= 1024
                    && !model.chars().any(char::is_control)
            })
    })
}

/// Return `defaults` with `rpcTimeoutMs` from `config` applied to both the wait for `ready`
/// and each request, as Paseo applies it.
pub(super) fn timeouts(config: &StoredAgentConfig, defaults: Timeouts) -> Timeouts {
    config
        .provider_options
        .as_ref()
        .and_then(|options| options.get(RPC_TIMEOUT)?.as_u64())
        .map_or(defaults, |millis| Timeouts {
            ready: Duration::from_millis(millis),
            request: Duration::from_millis(millis),
        })
}

pub(super) fn validate_spec(spec: &AgentSessionSpec) -> Result<(), AgentSessionError> {
    if spec.provider != PROVIDER
        || !Path::new(&spec.cwd).is_absolute()
        || !Path::new(&spec.cwd).is_dir()
    {
        return Err(AgentSessionError::Rejected);
    }
    validate(&spec.config)
}

pub(super) fn model_route(id: &str) -> Result<[String; 2], AgentSessionError> {
    let route: [String; 2] = serde_json::from_str(id).map_err(|_| AgentSessionError::Rejected)?;
    if route
        .iter()
        .any(|part| part.is_empty() || part.len() > 1024 || part.chars().any(char::is_control))
    {
        return Err(AgentSessionError::Rejected);
    }
    Ok(route)
}

/// Convert Ait's JSON model route into OMP's exact `provider/id` model argument.
///
/// # Errors
///
/// Returns `AgentSessionError::Rejected` for a malformed route.
pub(super) fn model_argument(id: &str) -> Result<String, AgentSessionError> {
    let [provider, model] = model_route(id)?;
    Ok(format!("{provider}/{model}"))
}

/// Return OMP's `provider/id` for the model `state` reports, if any.
///
/// # Errors
///
/// Returns `AgentSessionError::Failed` for a malformed native model.
pub(super) fn state_model(state: &Value) -> Result<Option<String>, AgentSessionError> {
    state
        .get("model")
        .filter(|model| !model.is_null())
        .map(|model| {
            Ok(format!(
                "{}/{}",
                text(model, "provider")?,
                text(model, "id")?
            ))
        })
        .transpose()
}

/// Return `level` when OMP's `--thinking` accepts it; Paseo ignores any other level.
pub(super) fn thinking_level(level: &str) -> Option<&'static str> {
    THINKING_LEVELS
        .iter()
        .map(|(id, _, _)| *id)
        .find(|id| *id == level)
}

/// Fast mode as OMP reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FastMode {
    /// The session asked for Fast.
    pub(super) enabled: bool,
    /// Fast applies to the current model.
    pub(super) active: bool,
}

/// Return Fast from `state` only when OMP reports both facts as booleans, as Paseo requires.
pub(super) fn fast_mode(state: &Value) -> Option<FastMode> {
    Some(FastMode {
        enabled: state["fastModeEnabled"].as_bool()?,
        active: state["fastModeActive"].as_bool()?,
    })
}

/// Return the Fast selection `config` carries, if any.
pub(super) fn requested_fast_mode(config: &StoredAgentConfig) -> Option<bool> {
    config.feature_values.as_ref()?.get(FAST_MODE)?.as_bool()
}

/// Return the live Fast toggle when `state` reports Fast. Its value is the `pending` selection
/// not yet applied, otherwise OMP's. No state means no toggle.
pub(super) fn features(pending: Option<bool>, state: Option<&Value>) -> Vec<Value> {
    let Some(fast) = state.and_then(fast_mode) else {
        return Vec::new();
    };
    let mut toggle = json!({"type":"toggle","id":FAST_MODE,"label":"Fast","icon":"zap",
        "value":pending.unwrap_or(fast.enabled)});
    if fast.enabled && !fast.active {
        toggle["description"] = json!(FAST_INACTIVE);
        toggle["tooltip"] = json!(FAST_INACTIVE);
    }
    vec![toggle]
}

/// Ask OMP for Fast `enabled` and record its answer in `state`. Does nothing when OMP does not
/// report Fast or already matches; a native refusal keeps OMP's value.
///
/// # Errors
///
/// Returns transport failures and malformed answers.
pub(super) async fn set_fast_mode(
    transport: &mut Transport,
    state: &mut Value,
    enabled: bool,
) -> Result<(), AgentSessionError> {
    if fast_mode(state).is_none_or(|fast| fast.enabled == enabled) {
        return Ok(());
    }
    let result = match transport
        .request("set_fast_mode", json!({"enabled":enabled}))
        .await
    {
        Ok(result) => result,
        Err(AgentSessionError::Rejected) => {
            tracing::warn!(
                enabled,
                "OMP refused to change fast mode; keeping its value"
            );
            return Ok(());
        }
        Err(error) => return Err(error),
    };
    let fast = fast_mode(&json!({"fastModeEnabled":result["enabled"],
        "fastModeActive":result["active"]}))
    .ok_or(AgentSessionError::Failed)?;
    state["fastModeEnabled"] = json!(fast.enabled);
    state["fastModeActive"] = json!(fast.active);
    Ok(())
}

/// How one OMP process starts and what is restored once it is ready.
#[derive(Debug, Default)]
pub(super) struct Launch<'a> {
    /// `provider/id` passed with `--model`; omitted on the first resume of a session file, so
    /// OMP restores the recorded model, but carried across restarts.
    pub(super) model: Option<String>,
    /// Known level passed with `--thinking`.
    pub(super) thinking: Option<&'static str>,
    /// Session file passed with `--session`, or `None` for a new session.
    pub(super) session: Option<&'a str>,
    /// Do not save the session, for disposable probes.
    pub(super) ephemeral: bool,
    /// Requested JSON route applied after resuming only when OMP lists it (D29).
    pub(super) resume_model: Option<&'a str>,
    /// Fast selection restored once OMP is ready.
    pub(super) fast: Option<bool>,
}

/// Build OMP's argv in Paseo's order: `--mode rpc-ui --approval-mode X [--smol m] [--slow m]
/// [--plan m] [--model p/id] [--thinking level] [--no-session | --session file]
/// [--append-system-prompt text]`.
pub(super) fn launch_arguments(config: &StoredAgentConfig, launch: &Launch<'_>) -> Vec<String> {
    let mut args = vec![
        "--mode".to_owned(),
        "rpc-ui".to_owned(),
        "--approval-mode".to_owned(),
        approval_mode(config.mode_id.as_deref()).to_owned(),
    ];
    if let Some(options) = &config.provider_options {
        for (option, flag) in MODEL_ROLES {
            if let Some(model) = options.get(option).and_then(Value::as_str) {
                args.extend([flag.to_owned(), model.to_owned()]);
            }
        }
    }
    if let Some(model) = &launch.model {
        args.extend(["--model".to_owned(), model.clone()]);
    }
    if let Some(level) = launch.thinking {
        args.extend(["--thinking".to_owned(), level.to_owned()]);
    }
    if launch.ephemeral {
        args.push("--no-session".to_owned());
    } else if let Some(file) = launch.session {
        args.extend(["--session".to_owned(), file.to_owned()]);
    }
    if let Some(prompt) = config
        .system_prompt
        .as_deref()
        .map(str::trim)
        .filter(|prompt| !prompt.is_empty())
    {
        args.extend(["--append-system-prompt".to_owned(), prompt.to_owned()]);
    }
    args
}

/// Apply `next`'s selections that differ from `previous` to a live process between turns,
/// then return OMP's refreshed state.
///
/// # Errors
///
/// Returns invalid configuration, native refusals of a model change, and transport failures.
pub(super) async fn apply(
    transport: &mut Transport,
    previous: &StoredAgentConfig,
    next: &StoredAgentConfig,
) -> Result<Value, AgentSessionError> {
    validate(next)?;
    if next.model != previous.model
        && let Some(model) = &next.model
    {
        let [provider, model] = model_route(model)?;
        transport
            .request("set_model", json!({"provider":provider,"modelId":model}))
            .await?;
    }
    if next.thinking_option_id != previous.thinking_option_id
        && let Some(level) = next.thinking_option_id.as_deref().and_then(thinking_level)
    {
        transport
            .request("set_thinking_level", json!({"level":level}))
            .await?;
    }
    let mut state = transport.request("get_state", json!({})).await?;
    if requested_fast_mode(next) != requested_fast_mode(previous)
        && let Some(enabled) = requested_fast_mode(next)
    {
        set_fast_mode(transport, &mut state, enabled).await?;
    }
    Ok(state)
}

/// Switch a resumed session to `requested` only when it differs from OMP's choice in `state`
/// and OMP lists it within the request timeout, as Paseo does; otherwise keep OMP's model and
/// log why. The bound keeps a slow catalog refresh from holding the resume indefinitely.
///
/// # Errors
///
/// Returns a malformed route or transport failures.
pub(super) async fn resume_model(
    transport: &mut Transport,
    requested: &str,
    state: Value,
) -> Result<Value, AgentSessionError> {
    let [provider, id] = model_route(requested)?;
    let selected = |model: &Value| model["provider"] == provider && model["id"] == id;
    if selected(&state["model"]) {
        return Ok(state);
    }
    let limit = transport.timeouts().request;
    let Some(models) = transport
        .request_unless_late("get_available_models", json!({}), limit)
        .await?
    else {
        tracing::warn!(
            requested,
            session_model = %state["model"]["id"],
            "OMP resumed on the session's model because it did not list its models in time"
        );
        return Ok(state);
    };
    if models["models"]
        .as_array()
        .is_some_and(|models| models.iter().any(selected))
    {
        match transport
            .request("set_model", json!({"provider":provider,"modelId":id}))
            .await
        {
            Ok(_) => return transport.request("get_state", json!({})).await,
            Err(AgentSessionError::Rejected) => {}
            Err(error) => return Err(error),
        }
    }
    tracing::warn!(
        requested,
        session_model = %state["model"]["id"],
        "OMP resumed on the session's model because the requested model is unavailable"
    );
    Ok(state)
}

pub(super) async fn discover(transport: &mut Transport) -> Result<Details, AgentSessionError> {
    let state = transport.request("get_state", json!({})).await?;
    // Catalog refreshes can be slow, so this request waits without a timeout as Paseo does.
    let models = transport
        .request_within("get_available_models", json!({}), None)
        .await?;
    let models = models["models"]
        .as_array()
        .filter(|models| models.len() <= 8192)
        .ok_or(AgentSessionError::Failed)?;
    Ok(Details {
        models: models
            .iter()
            .map(|model| model_entry(model, &state["model"]))
            .collect::<Result<_, _>>()?,
        modes: modes(),
        features: Vec::new(),
    })
}

/// Project one native model as Paseo does: the label is the normalized last segment of its
/// name, the description is `provider/id`, the metadata names the native provider and model,
/// and a non-reasoning model has no thinking options. The id stays Ait's JSON route (ADR-180).
fn model_entry(model: &Value, current: &Value) -> Result<Value, AgentSessionError> {
    let provider = text(model, "provider")?;
    let id = text(model, "id")?;
    let name = model["name"]
        .as_str()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or(id);
    let mut entry = json!({"provider":PROVIDER,"id":json!([provider,id]).to_string(),
        "label":model_label(name),"description":format!("{provider}/{id}"),"isSelectable":true,
        "isDefault":current["provider"] == provider && current["id"] == id,
        "metadata":{"provider":provider,"modelId":id}});
    if let Some((options, default)) = thinking_options(model) {
        entry["thinkingOptions"] = Value::Array(options);
        entry["defaultThinkingOptionId"] = json!(default);
    }
    Ok(entry)
}

/// Return `name`'s last `/` segment with underscore and whitespace runs collapsed and any
/// `Vendor: ` prefix removed.
fn model_label(name: &str) -> String {
    let segment = name
        .rsplit('/')
        .find(|segment| !segment.is_empty())
        .unwrap_or(name);
    let normalized = segment
        .split(|character: char| character == '_' || character.is_whitespace())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    match normalized.split_once(": ") {
        Some((_, label)) if !label.trim().is_empty() => label.trim().to_owned(),
        _ => normalized,
    }
}

/// Return Paseo's thinking options and default for `model`: none for a non-reasoning model,
/// all levels (default medium) when it reports no known efforts, otherwise `auto` plus its
/// efforts with OMP's `thinking.defaultLevel`.
fn thinking_options(model: &Value) -> Option<(Vec<Value>, &'static str)> {
    if model["reasoning"] != true {
        return None;
    }
    let efforts = model["thinking"]["efforts"].as_array();
    let reported = |id: &str| efforts.is_some_and(|efforts| efforts.iter().any(|item| item == id));
    let levels: Vec<_> = THINKING_LEVELS
        .iter()
        .filter(|(id, _, _)| reported(id))
        .collect();
    let (levels, default): (Vec<_>, _) = if levels.is_empty() {
        (THINKING_LEVELS.iter().collect(), DEFAULT_THINKING)
    } else {
        let default = model["thinking"]["defaultLevel"]
            .as_str()
            .and_then(|level| levels.iter().find(|(id, _, _)| *id == level))
            .unwrap_or(&levels[0])
            .0;
        // `auto` is a session-level OMP mode, so it leads rather than following the efforts.
        let efforts = levels.into_iter().filter(|(id, _, _)| *id != "auto");
        (
            std::iter::once(&THINKING_LEVELS[0])
                .chain(efforts)
                .collect(),
            default,
        )
    };
    let options = levels
        .into_iter()
        .map(|(id, label, description)| {
            let mut option = json!({"id":id,"label":label,"description":description});
            if *id == default {
                option["isDefault"] = json!(true);
            }
            option
        })
        .collect();
    Some((options, default))
}

pub(super) fn runtime(state: &Value) -> Result<StoredAgentRuntimeInfo, AgentSessionError> {
    let id = text(state, "sessionId")?;
    let model = state
        .get("model")
        .filter(|model| !model.is_null())
        .map(|model| Ok(json!([text(model, "provider")?, text(model, "id")?]).to_string()))
        .transpose()?;
    Ok(StoredAgentRuntimeInfo {
        provider: PROVIDER.to_owned(),
        session_id: Some(id.to_owned()),
        model,
        thinking_option_id: state["thinkingLevel"].as_str().map(str::to_owned),
        mode_id: None,
        extra: None,
    })
}

pub(super) fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, AgentSessionError> {
    value[key]
        .as_str()
        .filter(|text| {
            !text.is_empty() && text.len() <= 4096 && !text.chars().any(char::is_control)
        })
        .ok_or(AgentSessionError::Failed)
}

#[cfg(test)]
mod tests;

pub(super) fn modes() -> Vec<Value> {
    [
        (
            "full",
            "Full Access",
            "Approves ordinary read, write and execution tools.",
            "dangerous",
        ),
        (
            "write",
            "Allow File Writes",
            "Allows reads and file writes; command execution requires approval.",
            "moderate",
        ),
        (
            "ask",
            "Always Ask",
            "Requires approval for write and execution tools.",
            "safe",
        ),
        (
            "ask-cached",
            "Ask and Remember",
            "Requires approval for ungranted files and execution tools.",
            "safe",
        ),
    ]
    .into_iter()
    .map(|(id, label, description, tier)| {
        json!({"id":id,"label":label,"description":description,
            "colorTier":tier,"isUnattended":id=="full"})
    })
    .collect()
}

/// Return OMP's `--approval-mode` for `mode`; no mode means Full Access, as in Paseo.
pub(super) fn approval_mode(mode: Option<&str>) -> &'static str {
    match mode {
        None | Some("full") => "yolo",
        Some("ask") => "always-ask",
        Some("ask-cached") => "always-ask-cached",
        Some(_) => "write",
    }
}
