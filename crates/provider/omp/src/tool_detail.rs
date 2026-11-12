//! Display projection of OMP tool calls (D10), ported from Paseo's OMP tool-call mapper
//! (`tool-call-mapper.ts`, `tool-call-detail.ts` and `tool-call-id.ts`).
//!
//! A projection depends only on the assistant's tool call and OMP's reported result, so a
//! live `tool_execution_end` and a replayed `toolResult` project the same item.

use serde_json::{Value, json};

use super::stream::preview;

/// Tools shown as a labelled text row, as Paseo's `OPAQUE_TEXT_TOOLS`.
const OPAQUE_TEXT_TOOLS: [&str; 17] = [
    "ast_edit",
    "debug",
    "eval",
    "github",
    "checkpoint",
    "rewind",
    "context_notes",
    "new_context",
    "security_scan",
    "memory_edit",
    "retain",
    "recall",
    "reflect",
    "learn",
    "manage_skill",
    "yield",
    "goal",
];
/// Arguments whose JSON exceeds this are shown only as a bounded preview.
const MAX_ARGUMENT_BYTES: usize = 256 * 1024;
/// Characters kept from a one-line failure reason, as Paseo's `toolFailureMessage`.
const FAILURE_CHARACTERS: usize = 240;
/// OMP's virtual tool device scheme.
const DEVICE: &str = "xd://";

/// Display status of one native tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Status {
    Running,
    Completed,
    Failed,
    /// The turn ended before OMP reported a result.
    Canceled,
}

impl Status {
    const fn name(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Canceled => "canceled",
        }
    }
}

/// What OMP reported for a tool call: its (partial) result object, carrying `content`,
/// `details` and sometimes an exit code, and the display text of that content.
#[derive(Debug, Clone, Copy)]
pub(super) struct Report<'a> {
    pub(super) result: &'a Value,
    pub(super) text: Option<&'a str>,
}

impl<'a> Report<'a> {
    fn details(self) -> &'a Value {
        &self.result["details"]
    }
}

/// Project the assistant's tool `call` (`{id, name, arguments}`) into a `tool_call` item.
///
/// # Arguments
///
/// * `call` - The native tool call block.
/// * `report` - OMP's result or partial result, when it reported one.
/// * `status` - The call's display status; a failure carries Paseo's failure message.
///
/// # Returns
///
/// The timeline item, or none for the todo tool, which shows as a todo list instead (M4).
pub(super) fn item(call: &Value, report: Option<Report<'_>>, status: Status) -> Option<Value> {
    let name = call["name"].as_str().unwrap_or("tool");
    if name == "todo" {
        return None;
    }
    let arguments = &call["arguments"];
    let id = call["id"].as_str().unwrap_or_default();
    let error = match (status, report) {
        (Status::Failed, Some(report)) => json!(failure_message(report)),
        (Status::Failed, None) => json!("Tool call failed"),
        _ => Value::Null,
    };
    Some(
        json!({"type":"tool_call","callId":call_id(id, name, arguments),
        "name":display_name(name, arguments, report),"detail":detail(name, arguments, report),
        "status":status.name(),"error":error}),
    )
}

/// Whether OMP's `report` is a failure, as Paseo's `isOmpToolFailure`: OMP flagged an error,
/// or `web_search` reported one in its details without flagging it.
pub(super) fn failed(name: &str, report: Report<'_>, is_error: bool) -> bool {
    is_error || (name == "web_search" && report.details()["error"].is_string())
}

/// Paseo's `toolFailureMessage`: the reported error's first line, else a command exit line,
/// else the output's first line, else the exit code.
fn failure_message(report: Report<'_>) -> String {
    let error = &report.details()["error"];
    if let Some(error) = error.as_str().or_else(|| error["message"].as_str())
        && let Some(line) = error.split('\n').next().filter(|line| !line.is_empty())
    {
        return line.chars().take(FAILURE_CHARACTERS).collect();
    }
    if let Some(text) = report.text {
        if let Some(exit) = exit_line(text) {
            return exit.to_owned();
        }
        if let Some(line) = text.lines().map(str::trim).find(|line| !line.is_empty()) {
            return line.chars().take(FAILURE_CHARACTERS).collect();
        }
    }
    match report.result["exitCode"]
        .as_i64()
        .or_else(|| report.result["code"].as_i64())
    {
        Some(code) => format!("Tool exited with code {code}"),
        None => "Tool call failed".to_owned(),
    }
}

/// The first `Command exited with code N` or `Process exited with code N` in `text`, ignoring
/// ASCII case, as Paseo's `/(?:Command|Process) exited with code \d+/i`.
fn exit_line(text: &str) -> Option<&str> {
    let lower = text.to_ascii_lowercase();
    let mut from = 0;
    while let Some(found) = lower[from..].find(" exited with code ") {
        let start = from + found;
        let digits = start + " exited with code ".len();
        let end = digits
            + lower[digits..]
                .bytes()
                .take_while(u8::is_ascii_digit)
                .count();
        for subject in ["command", "process"] {
            if end > digits && lower[..start].ends_with(subject) {
                return Some(&text[start - subject.len()..end]);
            }
        }
        from = digits;
    }
    None
}

/// The emitted call id, as Paseo's `resolveOmpEmittedToolCallId`: a `subagent` poll of the
/// same jobs keeps one row (`omp-poll:<sorted ids>`) however often it is repeated.
fn call_id(id: &str, name: &str, arguments: &Value) -> String {
    if name != "subagent" {
        return id.to_owned();
    }
    let Some(targets) = arguments["poll"].as_array().filter(|poll| !poll.is_empty()) else {
        return id.to_owned();
    };
    let mut ids = Vec::with_capacity(targets.len());
    for target in targets {
        match target
            .as_str()
            .map(str::trim)
            .filter(|text| !text.is_empty())
        {
            Some(text) => ids.push(text),
            None => return id.to_owned(),
        }
    }
    ids.sort_unstable();
    format!("omp-poll:{}", ids.join(","))
}

/// The row's tool name, as Paseo's `resolveToolCallName`: a device read or write names its
/// `xd://` tool, and a device execution reported in `details.xdev` names the executed tool.
fn display_name(name: &str, arguments: &Value, report: Option<Report<'_>>) -> String {
    if let Some(device) = device_name(name, arguments) {
        return device.to_owned();
    }
    if name == "write"
        && is_write(arguments)
        && let Some(tool) = report.and_then(|report| executed_device(report.details()))
    {
        return tool.to_owned();
    }
    name.to_owned()
}

/// The `xd://<tool>` a read or write addresses, or `name` when the URL names none.
fn device_name<'a>(name: &'a str, arguments: &'a Value) -> Option<&'a str> {
    if !(name == "read" || name == "write" && is_write(arguments)) {
        return None;
    }
    let path = arguments["path"].as_str()?.strip_prefix(DEVICE)?;
    let tool = path.split(['/', '?', '#']).next().unwrap_or_default();
    Some(if tool.is_empty() { name } else { tool })
}

/// The tool OMP executed through a device write, from a valid `details.xdev`.
fn executed_device(details: &Value) -> Option<&str> {
    let xdev = &details["xdev"];
    if xdev["mode"] != "execute" {
        return None;
    }
    xdev["tool"].as_str().filter(|tool| !tool.trim().is_empty())
}

/// Paseo parses a write only with both a path and content.
fn is_write(arguments: &Value) -> bool {
    arguments["path"].is_string() && arguments["content"].is_string()
}

/// The display detail of tool `name`, as Paseo's `mapOmpToolDetail`.
fn detail(name: &str, arguments: &Value, report: Option<Report<'_>>) -> Value {
    let encoded = arguments.to_string();
    if encoded.len() > MAX_ARGUMENT_BYTES {
        return json!({"type":"unknown","input":preview(&encoded),"output":null});
    }
    // O11: process resources keep their native input instead of a file operation.
    if matches!(name, "read" | "edit" | "write")
        && arguments["path"]
            .as_str()
            .is_some_and(|path| path.starts_with("proc://"))
    {
        return unknown(arguments, report);
    }
    if name == "task" {
        return task(arguments);
    }
    device(name, arguments, report)
        .or_else(|| extended(name, arguments, report))
        .unwrap_or_else(|| match name {
            "edit" => edit(arguments, report),
            "read" => read(arguments, report),
            _ => core(name, arguments, report),
        })
}

/// A device write that runs a text-shaped tool shows that tool's own row.
fn device(name: &str, arguments: &Value, report: Option<Report<'_>>) -> Option<Value> {
    if name != "write" || !is_write(arguments) {
        return None;
    }
    let content = arguments["content"].as_str()?;
    arguments["path"].as_str()?.strip_prefix(DEVICE)?;
    let tool = display_name(name, arguments, report);
    if !OPAQUE_TEXT_TOOLS.contains(&tool.as_str()) && tool != "ast_grep" {
        return None;
    }
    let parsed = serde_json::from_str(content).unwrap_or(Value::Null);
    extended(&tool, &parsed, report)
}

/// Paseo's `mapOmpExtendedToolDetail` for OMP-specific tools.
fn extended(name: &str, arguments: &Value, report: Option<Report<'_>>) -> Option<Value> {
    let text = report.and_then(|report| report.text);
    let detail = match name {
        "yield" => plain_text(Some("Submitted subagent result"), text),
        "wait" => wait(report),
        "web_search" => {
            let query = first_text(&[&arguments["query"]]).unwrap_or("Web search");
            search(query, Some("web_search"), text)
        }
        "ask" | "ask_user" => ask(arguments, report),
        "glob" | "ast_grep" => {
            // OMP 18.18.8's glob takes its pattern as `path`.
            let query = first_text(&[
                &arguments["pattern"],
                &arguments["pat"],
                &arguments["query"],
                &arguments["path"],
            ])
            .unwrap_or(name);
            search(query, (name == "glob").then_some("glob"), text)
        }
        "think" => plain_text(Some("Thinking"), first_text(&[&arguments["thoughts"]])),
        _ if OPAQUE_TEXT_TOOLS.contains(&name) => opaque(name, arguments, text),
        _ => return None,
    };
    Some(detail)
}

/// A labelled text row: the action and target the arguments name, else the tool name.
fn opaque(name: &str, arguments: &Value, text: Option<&str>) -> Value {
    let action = first_text(&[
        &arguments["action"],
        &arguments["op"],
        &arguments["operation"],
        &arguments["command"],
    ]);
    let target = first_text(&[
        &arguments["name"],
        &arguments["goal"],
        &arguments["path"],
        &arguments["query"],
        &arguments["id"],
        &arguments["report"],
    ]);
    let label = match (action, target) {
        (Some(action), Some(target)) => format!("{action} {target}"),
        (Some(part), None) | (None, Some(part)) => part.to_owned(),
        (None, None) => name.replace('_', " "),
    };
    plain_text(Some(&label), text)
}

/// The first background job `wait` reports, else the result's first line.
fn wait(report: Option<Report<'_>>) -> Value {
    let text = report.and_then(|report| report.text).map(str::trim);
    let job = report.and_then(|report| {
        report.details()["jobs"]
            .as_array()
            .and_then(|jobs| jobs.iter().find(|job| job.is_object()))
    });
    let summary = job.and_then(|job| {
        let kind = first_text(&[&job["type"]])?;
        let status = first_text(&[&job["status"]])?;
        let label = first_text(&[&job["label"], &job["id"]])?;
        Some(format!("{kind} {status}: {label}"))
    });
    let label = summary.unwrap_or_else(|| {
        text.map_or("Waiting for background work", |text| {
            text.split('\n').next().unwrap_or_default()
        })
        .to_owned()
    });
    let body = job
        .and_then(|job| first_text(&[&job["resultText"], &job["errorText"]]))
        .or(text);
    plain_text(Some(&label), body)
}

/// An `ask` row: each answered question with its choices, else the result text.
fn ask(arguments: &Value, report: Option<Report<'_>>) -> Value {
    let details = report
        .map(Report::details)
        .filter(|details| details.is_object())
        .unwrap_or(&Value::Null);
    let answers = details["results"]
        .as_array()
        .map_or_else(|| std::slice::from_ref(details), Vec::as_slice);
    let lines: Vec<String> = answers
        .iter()
        .filter_map(|answer| {
            let question = first_text(&[&answer["question"]])?;
            let mut chosen: Vec<&str> = answer["selectedOptions"]
                .as_array()
                .map(|options| options.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            chosen.extend(first_text(&[&answer["customInput"]]));
            let response = if chosen.is_empty() {
                "No selection".to_owned()
            } else {
                chosen.join(", ")
            };
            Some(format!("{question}\n{response}"))
        })
        .collect();
    let label = first_text(&[
        &details["question"],
        &arguments["questions"][0]["question"],
        &arguments["question"],
        &arguments["title"],
    ])
    .unwrap_or("Question");
    let joined = lines.join("\n\n");
    let text = if lines.is_empty() {
        report.and_then(|report| report.text)
    } else {
        Some(joined.as_str())
    };
    plain_text(Some(label), text)
}

/// A `task` row: the first task's name and summary; the stream names its child (D16).
fn task(arguments: &Value) -> Value {
    let first = arguments["tasks"]
        .as_array()
        .and_then(|tasks| tasks.iter().find(|task| task.is_object()))
        .unwrap_or(&Value::Null);
    let mut detail = json!({"type":"sub_agent","log":"","actions":[]});
    if let Some(kind) = first_text(&[
        &first["name"],
        &arguments["name"],
        &arguments["agent"],
        &arguments["subAgentType"],
        &arguments["agentType"],
        &arguments["type"],
    ]) {
        detail["subAgentType"] = json!(kind);
    }
    let summary = first_text(&[&first["task"]]).and_then(instruction_summary);
    let description = first_text(&[&arguments["i"]]).or(summary).or_else(|| {
        first_text(&[
            &arguments["description"],
            &arguments["task"],
            &arguments["prompt"],
            &arguments["assignment"],
        ])
    });
    if let Some(description) = description {
        detail["description"] = json!(preview(description));
    }
    detail
}

/// The job id of a `task` result's first child, which names its transcript: the first of
/// `details.results` (a finished task), else of `details.progress`, else the background
/// job, `details.async.jobId`, as OMP 18.18.8 reports them. Paseo instead links a child by
/// its session file path; Ait names it by its qualified identity (D16).
pub(super) fn task_child(details: &Value) -> Option<&str> {
    ["results", "progress"]
        .into_iter()
        .find_map(|field| {
            details[field]
                .as_array()?
                .iter()
                .find_map(|child| child["id"].as_str())
        })
        .or_else(|| details["async"]["jobId"].as_str())
        .filter(|job| super::history::children::valid_job(job))
}

/// The first nonblank line of a task instruction, without a `# Target`/`# Goal`/`# Task`
/// heading, as Paseo's `taskInstructionSummary`.
fn instruction_summary(instruction: &str) -> Option<&str> {
    instruction
        .split('\n')
        .map(|line| {
            let Some(rest) = line.strip_prefix('#') else {
                return line.trim();
            };
            let rest = rest.trim_start_matches('#').trim_start();
            let heading = ["target", "goal", "task"].into_iter().find(|word| {
                rest.get(..word.len())
                    .is_some_and(|start| start.eq_ignore_ascii_case(word))
            });
            heading.map_or(line, |word| &rest[word.len()..]).trim()
        })
        .find(|line| !line.is_empty())
}

/// An edit row from OMP's result details (`path`, `oldText`, `newText`, `diff`), else the
/// path named by the hashline patch input, as Paseo's `mapOmpEditDetail`.
fn edit(arguments: &Value, report: Option<Report<'_>>) -> Value {
    let details = report.map_or(&Value::Null, |report| report.details());
    let Some(path) =
        first_text(&[&details["path"], &details["filePath"]]).or_else(|| patch_path(arguments))
    else {
        return core("edit", arguments, report);
    };
    let mut detail = json!({"type":"edit","filePath":path});
    for (field, sources) in [
        ("oldString", ["oldText", "old_string"]),
        ("newString", ["newText", "new_string"]),
    ] {
        if let Some(text) = first_text(&[&details[sources[0]], &details[sources[1]]]) {
            detail[field] = json!(preview(text));
        }
    }
    if let Some(diff) = first_text(&[&details["diff"]]) {
        detail["unifiedDiff"] = json!(preview(diff));
    }
    detail
}

/// The file a hashline patch (`[path#TAG]`) edits, as Paseo's `readPatchInputPath`.
fn patch_path(arguments: &Value) -> Option<&str> {
    arguments["input"].as_str()?.lines().find_map(|line| {
        let header = line.strip_prefix('[')?;
        let end = header.find(']')?;
        let (path, tag) = header[..end].split_once('#')?;
        (!path.is_empty() && !tag.is_empty()).then_some(path)
    })
}

/// A read row, showing OMP's `displayContent` text when it reports one.
fn read(arguments: &Value, report: Option<Report<'_>>) -> Value {
    let mut detail = core("read", arguments, report);
    if detail["type"] == "read"
        && let Some(text) =
            report.and_then(|report| first_text(&[&report.details()["displayContent"]["text"]]))
    {
        detail["content"] = json!(preview(text));
    }
    detail
}

/// Paseo's core `mapToolDetail` for the tools pi defines.
fn core(name: &str, arguments: &Value, report: Option<Report<'_>>) -> Value {
    let text = report.and_then(|report| report.text);
    match name {
        "bash" if arguments["command"].is_string() => shell(arguments, report),
        "read" if arguments["path"].is_string() => read_path(arguments, text),
        "edit" => edit_arguments(arguments, report).unwrap_or_else(|| unknown(arguments, report)),
        "write" if is_write(arguments) => write(arguments, report),
        "find" if arguments["pattern"].is_string() => {
            search(text_of(&arguments["pattern"]), Some("search"), text)
        }
        "grep" if arguments["pattern"].is_string() => {
            search(text_of(&arguments["pattern"]), Some("grep"), text)
        }
        "ls" => search(arguments["path"].as_str().unwrap_or("ls"), None, text),
        _ => unknown(arguments, report),
    }
}

/// A shell row; once OMP reports, its exit code, or null when it reported none.
fn shell(arguments: &Value, report: Option<Report<'_>>) -> Value {
    let mut detail = json!({"type":"shell","command":preview(text_of(&arguments["command"]))});
    if let Some(cwd) = arguments["cwd"].as_str() {
        detail["cwd"] = json!(cwd);
    }
    if let Some(report) = report {
        if let Some(text) = report.text {
            detail["output"] = json!(text);
        }
        // OMP 18.18.8 reports the exit code in its details.
        detail["exitCode"] = [
            &report.result["exitCode"],
            &report.result["code"],
            &report.details()["exitCode"],
        ]
        .into_iter()
        .find(|code| code.is_number())
        .cloned()
        .unwrap_or(Value::Null);
    }
    detail
}

/// A read of a device, a URL or a file.
fn read_path(arguments: &Value, text: Option<&str>) -> Value {
    let path = text_of(&arguments["path"]);
    if path.starts_with(DEVICE) {
        let label = device_name("read", arguments).unwrap_or("read");
        return plain_text(Some(label), text);
    }
    if path.starts_with("http://") || path.starts_with("https://") {
        let mut detail = json!({"type":"fetch","url":path});
        if let Some(text) = text {
            detail["result"] = json!(text);
        }
        return detail;
    }
    let mut detail = json!({"type":"read","filePath":path});
    if let Some(text) = text {
        detail["content"] = json!(text);
    }
    for field in ["offset", "limit"] {
        if arguments[field].is_number() {
            detail[field] = arguments[field].clone();
        }
    }
    detail
}

/// An edit from pi's `{path, edits:[{oldText,newText}]}` or legacy single-edit arguments.
fn edit_arguments(arguments: &Value, report: Option<Report<'_>>) -> Option<Value> {
    let path = arguments["path"].as_str()?;
    let (old, new) = match arguments["edits"].as_array() {
        Some(edits) => (&edits.first()?["oldText"], &edits.first()?["newText"]),
        None => (
            [&arguments["old_string"], &arguments["oldString"]]
                .into_iter()
                .find(|value| value.as_str().is_some_and(|text| !text.is_empty()))?,
            [&arguments["new_string"], &arguments["newString"]]
                .into_iter()
                .find(|value| value.is_string())?,
        ),
    };
    let mut detail = json!({"type":"edit","filePath":path});
    if let Some(old) = old.as_str() {
        detail["oldString"] = json!(preview(old));
    }
    if let Some(new) = new.as_str() {
        detail["newString"] = json!(preview(new));
    }
    if let Some(diff) = report.and_then(|report| report.details()["diff"].as_str()) {
        detail["unifiedDiff"] = json!(preview(diff));
    }
    Some(detail)
}

/// A write row; a device execution OMP reported shows the executed tool's input instead.
fn write(arguments: &Value, report: Option<Report<'_>>) -> Value {
    if let Some(report) = report
        && report.details().get("xdev").is_some()
    {
        let text = report.text.map_or(Value::Null, Value::from);
        return match executed_device(report.details()) {
            Some(_) => json!({"type":"unknown","input":report.details()["xdev"]["args"],
                "output":text}),
            None => json!({"type":"unknown","input":arguments,"output":text}),
        };
    }
    // A device write runs a tool rather than writing a file; name it while it runs.
    if let Some(tool) = device_name("write", arguments) {
        return plain_text(Some(tool), report.and_then(|report| report.text));
    }
    json!({"type":"write","filePath":arguments["path"],
        "content":preview(text_of(&arguments["content"]))})
}

fn search(query: &str, tool: Option<&str>, text: Option<&str>) -> Value {
    let mut detail = json!({"type":"search","query":query});
    if let Some(tool) = tool {
        detail["toolName"] = json!(tool);
    }
    if let Some(text) = text {
        detail["content"] = json!(text);
    }
    detail
}

fn plain_text(label: Option<&str>, text: Option<&str>) -> Value {
    let mut detail = json!({"type":"plain_text"});
    if let Some(label) = label {
        detail["label"] = json!(label);
    }
    if let Some(text) = text {
        detail["text"] = json!(preview(text));
    }
    detail
}

/// Native input and output text, for a tool without a typed display.
fn unknown(arguments: &Value, report: Option<Report<'_>>) -> Value {
    let output = report
        .and_then(|report| report.text)
        .map_or(Value::Null, Value::from);
    json!({"type":"unknown","input":arguments,"output":output})
}

/// The first value that is a string with visible content, as Paseo's `firstString`.
fn first_text<'a>(values: &[&'a Value]) -> Option<&'a str> {
    values
        .iter()
        .find_map(|value| value.as_str().filter(|text| !text.trim().is_empty()))
}

fn text_of(value: &Value) -> &str {
    value.as_str().unwrap_or_default()
}

#[cfg(test)]
mod tests;
