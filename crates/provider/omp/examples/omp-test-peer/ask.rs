use serde_json::{Map, Value, json};

use super::Peer;

const OTHER: &str = "Other (type your own)";
const DONE: &str = "✔ Done selecting";

#[derive(Default)]
pub(super) struct AskFlow {
    field: usize,
    typing: bool,
    checked: Vec<Value>,
    other: Option<Value>,
    answers: Map<String, Value>,
}

struct Field {
    name: &'static str,
    title: &'static str,
    labels: [&'static str; 2],
    multiple: bool,
    required: bool,
}

impl AskFlow {
    fn field(&self) -> Field {
        if self.field == 0 {
            Field {
                name: "db",
                title: "Which database?",
                labels: ["A,B", "C"],
                multiple: false,
                required: false,
            }
        } else {
            Field {
                name: "features",
                title: "Which features?",
                labels: ["Auth", "Search"],
                multiple: true,
                required: std::env::var("OMP_FIXTURE_ASK_REQUIRED").as_deref() == Ok("1"),
            }
        }
    }
}

impl Peer {
    pub(super) fn start_ask(&mut self) {
        self.ask_flow = Some(AskFlow::default());
        self.ask_prompt();
    }

    pub(super) fn ask_answer(&mut self, command: &Value) {
        let value = &command["value"];
        if command["cancelled"] == true || value.is_null() {
            self.ask_flow = None;
            self.tool_end(
                "ask-call",
                "ask",
                "Ask tool was cancelled by the user",
                true,
                Value::Null,
            );
            self.end_aborted_run();
            self.prompt_result("aborted", true, true);
            self.emit(&json!({ "type": "session_settled" }));
            return;
        }
        let ask = self.ask_flow.as_mut().expect("pending ask flow");
        let field = ask.field();
        if ask.typing {
            ask.typing = false;
            if !field.multiple {
                self.ask_next(value.clone());
                return;
            }
            ask.other = Some(value.clone());
        } else if value == OTHER {
            ask.typing = true;
        } else if field.multiple && value == DONE {
            let mut selected = std::mem::take(&mut ask.checked);
            selected.extend(ask.other.take().filter(|value| value != ""));
            self.ask_next(json!(selected));
            return;
        } else if value
            .as_str()
            .is_some_and(|label| field.labels.contains(&label))
        {
            if !field.multiple {
                self.ask_next(value.clone());
                return;
            }
            if let Some(index) = ask.checked.iter().position(|item| item == value) {
                ask.checked.remove(index);
            } else {
                ask.checked.push(value.clone());
            }
        }
        self.ask_prompt();
    }

    fn ask_prompt(&mut self) {
        let ask = self.ask_flow.as_ref().expect("pending ask flow");
        let field = ask.field();
        if ask.typing {
            self.ui_request("input", json!({ "title": field.title }));
            return;
        }
        let count = ask.checked.len() + usize::from(ask.other.is_some());
        let mut options = Vec::from(field.labels);
        options.push(OTHER);
        if field.multiple && (!field.required || count > 0) {
            options.push(DONE);
        }
        let title = if field.multiple && count > 0 {
            format!("({count} selected) {}", field.title)
        } else {
            field.title.to_owned()
        };
        self.ui_request("select", json!({ "title": title, "options": options }));
    }

    fn ask_next(&mut self, value: Value) {
        let mut ask = self.ask_flow.take().expect("pending ask flow");
        ask.answers.insert(ask.field().name.to_owned(), value);
        if ask.field == 0 {
            self.ask_flow = Some(AskFlow {
                field: 1,
                answers: ask.answers,
                ..AskFlow::default()
            });
            self.ask_prompt();
            return;
        }
        let answers = serde_json::to_string_pretty(&ask.answers).expect("serialize ask answers");
        self.tool_end(
            "ask-call",
            "ask",
            &format!("User answers:\n{answers}"),
            false,
            json!({ "answers": ask.answers }),
        );
        self.finish(true);
    }
}
