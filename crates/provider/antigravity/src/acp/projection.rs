//! AGY tool semantics layered on the shared bounded ACP stream projection.

use std::collections::BTreeMap;

use provider_core::{
    ports::agent_session::{AgentSessionError, AgentTurnEvent},
    protocol::timeline::NativeItem,
};
use provider_native::{acp_streaming::Stream, images::ImageStore};
use serde_json::{Value, json};

/// Shared streaming projection plus the still-running AGY tool cards to settle on failure.
#[derive(Debug)]
pub(super) struct Projection {
    /// Shared text, images, compactions and context occupancy.
    pub(super) stream: Stream,
    /// Only releases with verified native background-work completion avoid compatibility notices.
    pub(super) completion_fixes: bool,
    tools: BTreeMap<String, NativeItem>,
    turn: String,
    warned_background: bool,
}

impl Projection {
    /// Whether the native completion left tool work unfinished.
    pub(super) fn has_running_tools(&self) -> bool {
        !self.tools.is_empty()
    }

    /// Start a bounded projection for one native prompt, with durable image previews.
    pub(super) fn new(turn: &str, images: ImageStore) -> Self {
        let mut stream = Stream::new(images);
        stream.begin(turn.to_owned());
        Self {
            stream,
            completion_fixes: true,
            tools: BTreeMap::new(),
            turn: turn.to_owned(),
            warned_background: false,
        }
    }

    /// Preserve native names and enrich file, shell and subagent events without inventing IDs.
    pub(super) fn update(&mut self, update: &Value) -> Result<(), AgentSessionError> {
        if !self.completion_fixes
            && !self.warned_background
            && update["sessionUpdate"] == "tool_call"
            && matches!(
                update["name"].as_str(),
                Some("invoke_subagent" | "run_command")
            )
        {
            self.stream.events.push_back(notice(&self.turn, "background", "当前 AGY 版本可能在子代理或后台命令完成前结束回复，完成状态和用量可能不完整。升级 AGY 到 1.5.15 或更新版本可获得原生修复；命令和子代理仍按原生规则执行。"));
            self.warned_background = true;
        }
        let first = self.stream.events.len();
        if matches!(
            update["sessionUpdate"].as_str(),
            Some("tool_call" | "tool_call_update")
        ) {
            let mut update = update.clone();
            if let Some(name) = update["name"].as_str() {
                update["title"] = json!(name);
            }
            // ACP uses failed for a cancelled tool; the adapter closes any remaining cards.
            if update["status"] == "cancelled" {
                update["status"] = json!("failed");
            }
            self.stream.update(&update)?;
        } else {
            self.stream.update(update)?;
        }
        for event in self.stream.events.iter_mut().skip(first) {
            let entry = match event {
                AgentTurnEvent::Timeline(entry) | AgentTurnEvent::Progress { entry, .. }
                    if entry.item["type"] == "tool_call" =>
                {
                    entry
                }
                _ => continue,
            };
            enrich(entry);
            let id = entry.item["callId"]
                .as_str()
                .ok_or(AgentSessionError::Failed)?;
            if entry.item["status"] == "running" {
                self.tools.insert(id.to_owned(), entry.clone());
            } else {
                self.tools.remove(id);
            }
        }
        Ok(())
    }

    /// Flush partial output and settle every still-running tool before the single turn terminal.
    pub(super) fn finish(&mut self, cancelled: bool) {
        self.stream.flush();
        self.stream.finish_compactions();
        for (_, mut entry) in std::mem::take(&mut self.tools) {
            entry.item["status"] = json!(if cancelled { "canceled" } else { "failed" });
            entry.item["error"] = json!({"message":if cancelled {"AGY tool cancelled"} else {"AGY tool ended without a completion update"}});
            self.stream
                .events
                .push_back(AgentTurnEvent::Timeline(entry));
        }
    }
}

fn enrich(entry: &mut NativeItem) {
    let input = &entry.item["detail"]["input"];
    let output = &entry.item["detail"]["output"];
    let path = input["Path"]
        .as_str()
        .or_else(|| input["AbsolutePath"].as_str())
        .or_else(|| input["TargetFile"].as_str());
    let detail = match entry.item["name"].as_str() {
        Some("read_file" | "view_file") if path.is_some() => {
            let mut detail = json!({"type":"read","filePath":path});
            if output.is_string() {
                detail["content"] = output.clone();
            }
            detail
        }
        Some("write_file" | "write_to_file") if path.is_some() => {
            let mut detail = json!({"type":"write","filePath":path});
            if let Some(content) = input["Content"].as_str() {
                detail["content"] = json!(content);
            }
            detail
        }
        Some("replace_file_content" | "multi_replace_file_content" | "edit_file")
            if path.is_some() =>
        {
            json!({"type":"edit","filePath":path})
        }
        Some("run_command") if input["CommandLine"].is_string() => {
            let mut detail = json!({"type":"shell","command":input["CommandLine"]});
            if input["Cwd"].is_string() {
                detail["cwd"] = input["Cwd"].clone();
            }
            if let Some(output) = output.as_str().or_else(|| output["output"].as_str()) {
                detail["output"] = json!(output);
            }
            if let Some(code) = output["exit_code"].as_i64() {
                detail["exitCode"] = json!(code);
            }
            detail
        }
        Some("invoke_subagent") => json!({"type":"sub_agent","log":"","actions":[],
            "description":input["Prompt"].as_str().unwrap_or_default(),
            "subAgentType":input["TypeName"].as_str().unwrap_or_default()}),
        _ => return,
    };
    entry.item["metadata"]["nativeInput"] = input.clone();
    entry.item["detail"] = detail;
}

/// A display-only feature notice; never submitted to the native model as instructions.
pub(super) fn notice(turn: &str, feature: &str, message: &str) -> AgentTurnEvent {
    AgentTurnEvent::Timeline(NativeItem {
        key: format!("native:{turn}:upgrade:{feature}"),
        turn_id: Some(turn.to_owned()),
        timestamp: chrono::Utc::now().to_rfc3339(),
        item: json!({"type":"notification","level":"warning","message":message}),
    })
}
