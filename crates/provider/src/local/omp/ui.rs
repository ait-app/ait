use std::collections::BTreeMap;

use serde_json::{Value, json};

use crate::ports::agent_session::AgentSessionError;

/// Bounded, connection-scoped UI emitted by the official OMP RPC implementation.
#[derive(Debug, Default)]
pub(super) struct State {
    statuses: BTreeMap<String, String>,
    widgets: BTreeMap<String, Value>,
    title: Option<String>,
    editor: Option<Value>,
    link: Option<Value>,
}

impl State {
    /// Apply an exported native UI event, rejecting malformed or oversized state.
    pub(super) fn capture(&mut self, event: &Value) -> Result<(), AgentSessionError> {
        if event["type"] == "dictation_end" && event["error"].is_null() {
            text(&event["captureId"], 256)?;
            self.editor = Some(json!({"id":uuid::Uuid::new_v4().to_string(),
                "text":text(&event["text"],65536)?}));
        }
        if event["type"] == "response" && event["command"] == "login" {
            self.link = None;
        }
        if event["type"] == "response"
            && event["success"] == true
            && let Some(editor) = event["data"]["editorText"].as_str()
        {
            if editor.len() > 65536 {
                return Err(AgentSessionError::Failed);
            }
            self.editor = Some(json!({"id":uuid::Uuid::new_v4().to_string(),"text":editor}));
        } else if event["type"] == "response"
            && event["success"] == true
            && event["command"] == "new_session"
            && event["data"]["cancelled"] != true
        {
            self.editor = Some(json!({"id":uuid::Uuid::new_v4().to_string(),"text":""}));
        }
        match event["method"].as_str() {
            Some("setStatus") => {
                let key = text(&event["statusKey"], 256)?.to_owned();
                if event["statusText"].is_null() {
                    self.statuses.remove(&key);
                } else {
                    let value = text(&event["statusText"], 4096)?.to_owned();
                    check_capacity(&self.statuses, &key)?;
                    self.statuses.insert(key, value);
                }
            }
            Some("setWidget") => {
                let key = text(&event["widgetKey"], 256)?.to_owned();
                if event["widgetLines"].is_null() {
                    self.widgets.remove(&key);
                } else {
                    let lines = event["widgetLines"]
                        .as_array()
                        .filter(|lines| lines.len() <= 128)
                        .ok_or(AgentSessionError::Failed)?;
                    let mut bytes = 0;
                    for line in lines {
                        bytes += text(line, 8192)?.len();
                    }
                    if bytes > 32768 {
                        return Err(AgentSessionError::Failed);
                    }
                    check_capacity(&self.widgets, &key)?;
                    let placement = if event["widgetPlacement"] == "belowEditor" {
                        "belowEditor"
                    } else {
                        "aboveEditor"
                    };
                    let widget = json!({"key":key,"lines":lines,"placement":placement});
                    self.widgets.insert(key, widget);
                }
            }
            Some("setTitle") => self.title = Some(text(&event["title"], 4096)?.to_owned()),
            Some("set_editor_text") => {
                text(&event["id"], 256)?;
                self.editor = Some(json!({"id":uuid::Uuid::new_v4().to_string(),
                    "text":text(&event["text"],65536)?}));
            }
            Some("open_url") => {
                let url = text(&event["url"], 8192)?;
                let launch_url = event
                    .get("launchUrl")
                    .filter(|value| !value.is_null())
                    .map_or(Ok(url), |value| text(value, 8192))?;
                for candidate in [url, launch_url] {
                    let parsed =
                        url::Url::parse(candidate).map_err(|_| AgentSessionError::Failed)?;
                    if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
                        return Err(AgentSessionError::Rejected);
                    }
                }
                let instructions = event
                    .get("instructions")
                    .filter(|value| !value.is_null())
                    .map(|value| text(value, 8192))
                    .transpose()?;
                self.link =
                    Some(json!({"url":url,"launchUrl":launch_url,"instructions":instructions}));
            }
            _ => {}
        }
        Ok(())
    }

    /// Return the current ephemeral view; credentials and drafts never enter native history.
    pub(super) fn snapshot(&self) -> Value {
        json!({"statuses":self.statuses.iter().map(|(key,text)|json!({"key":key,"text":text})).collect::<Vec<_>>(),
            "widgets":self.widgets.values().collect::<Vec<_>>(),"title":self.title,
            "editor":self.editor,"link":self.link})
    }
}

fn text(value: &Value, limit: usize) -> Result<&str, AgentSessionError> {
    value
        .as_str()
        .filter(|value| value.len() <= limit)
        .ok_or(AgentSessionError::Failed)
}

fn check_capacity<T>(values: &BTreeMap<String, T>, key: &str) -> Result<(), AgentSessionError> {
    if values.len() >= 32 && !values.contains_key(key) {
        return Err(AgentSessionError::Failed);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
