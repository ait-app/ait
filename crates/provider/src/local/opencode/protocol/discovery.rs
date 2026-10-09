//! Native session discovery normalizes lists, previews, and saved selections.
fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, AgentSessionError> {
    value[key]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or(AgentSessionError::Failed)
}

pub(in crate::local::opencode) fn timestamp(value: &Value) -> Result<String, AgentSessionError> {
    value
        .as_i64()
        .and_then(chrono::DateTime::from_timestamp_millis)
        .map(|value| value.to_rfc3339())
        .ok_or(AgentSessionError::Failed)
}
use super::{Version, http::Api};
use crate::local::opencode::{client::error, session};
use crate::ports::{agent_session::AgentSessionError, native_history::SessionDescriptor};
use domain::agent_runtime::StoredAgentConfig;
use reqwest::Url;
use serde_json::Value;
use std::path::Path;

impl Api {
    pub(in crate::local::opencode) fn session_list_path(
        &self,
        remaining: usize,
        cwd: Option<&str>,
        cursor: Option<&str>,
    ) -> String {
        let mut url = Url::parse("http://127.0.0.1/").expect("constant loopback URL");
        url.set_path(match self.version {
            Version::V1 => "/experimental/session",
            Version::V2 => "/api/session",
        });
        {
            let mut query = url.query_pairs_mut();
            let limit = match self.version {
                Version::V1 => remaining,
                Version::V2 => remaining.min(100),
            };
            query.append_pair("limit", &limit.to_string());
            if let Some(cwd) = cwd {
                query.append_pair("directory", cwd);
            }
            if self.version == Version::V2 {
                if let Some(cursor) = cursor {
                    query.append_pair("cursor", cursor);
                } else {
                    query.append_pair("order", "desc");
                }
            }
        }
        format!("{}?{}", url.path(), url.query().unwrap_or_default())
    }

    pub(in crate::local::opencode) fn session_list_cursor(
        &self,
        response: &Value,
    ) -> Result<Option<String>, AgentSessionError> {
        if self.version == Version::V1 {
            return Ok(None);
        }
        match response.pointer("/cursor/next") {
            Some(Value::String(next)) if !next.is_empty() => Ok(Some(next.clone())),
            Some(Value::Null) | None => Ok(None),
            Some(_) => Err(AgentSessionError::Failed),
        }
    }

    pub(in crate::local::opencode) fn session_descriptor(
        &self,
        info: &Value,
    ) -> Result<SessionDescriptor, AgentSessionError> {
        descriptor(self.version, info)
    }

    pub(in crate::local::opencode) fn prompt_preview(&self, message: &Value) -> Option<String> {
        prompt(self.version, message)
    }

    pub(in crate::local::opencode) async fn saved_config(
        &self,
        info: &Value,
        id: &str,
    ) -> Result<StoredAgentConfig, AgentSessionError> {
        let (model, agent) = match self.version {
            Version::V2 => (
                info["model"].clone(),
                info["agent"].as_str().unwrap_or("build").to_owned(),
            ),
            Version::V1 => {
                let messages = self.history(id).await.map_err(error)?;
                messages
                    .iter()
                    .rev()
                    .find_map(|message| {
                        let info = &message["info"];
                        if info["role"] == "user" {
                            Some((
                                info["model"].clone(),
                                info["agent"].as_str().unwrap_or("build").to_owned(),
                            ))
                        } else {
                            None
                        }
                    })
                    .ok_or(AgentSessionError::Rejected)?
            }
        };
        let model_id = format!(
            "{}/{}",
            text(&model, "providerID")?,
            text(
                &model,
                match self.version {
                    Version::V1 => "modelID",
                    Version::V2 => "id",
                }
            )?
        );
        Ok(StoredAgentConfig {
            mode_id: Some(agent),
            model: Some(model_id),
            thinking_option_id: model["variant"]
                .as_str()
                .filter(|variant| *variant != "default")
                .map(str::to_owned),
            ..Default::default()
        })
    }
}

pub(in crate::local::opencode) fn prompt(version: Version, message: &Value) -> Option<String> {
    use crate::local::session_preview;
    match version {
        Version::V1 if message["info"]["role"] == "user" => session_preview::text(
            message["parts"]
                .as_array()?
                .iter()
                .filter(|part| {
                    part["type"] == "text" && part["synthetic"] != true && part["ignored"] != true
                })
                .filter_map(|part| part["text"].as_str()),
        ),
        Version::V2 if message["type"] == "user" => {
            session_preview::text(message["text"].as_str().into_iter())
        }
        Version::V1 | Version::V2 => None,
    }
}

pub(in crate::local::opencode) fn descriptor(
    version: Version,
    info: &Value,
) -> Result<SessionDescriptor, AgentSessionError> {
    let id = text(info, "id")?;
    if !session::valid_id(id) {
        return Err(AgentSessionError::Rejected);
    }
    let cwd = match version {
        Version::V1 => text(info, "directory")?,
        Version::V2 => text(&info["location"], "directory")?,
    };
    if !Path::new(cwd).is_absolute() {
        return Err(AgentSessionError::Rejected);
    }
    Ok(SessionDescriptor {
        provider_id: "opencode".into(),
        provider_label: "OpenCode".into(),
        provider_handle_id: id.into(),
        cwd: cwd.into(),
        title: info["title"].as_str().map(str::to_owned),
        first_prompt_preview: None,
        last_prompt_preview: None,
        last_activity_at: timestamp(&info["time"]["updated"])?,
    })
}
