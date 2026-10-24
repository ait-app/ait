//! Display projection of Codex's contextual realtime delegation envelope.

use serde_json::{Value, json};

const MAX_BYTES: usize = 64 * 1024;
const MAX_ENTRIES: usize = 256;

/// Replace a complete native envelope with readable input and ordered speech entries.
/// Unrecognized or oversized text is preserved unchanged; native prompt history is untouched.
pub(super) fn project(item: &mut Value) {
    if item.get("imageSources").is_some() {
        return;
    }
    let Some((input, entries)) = item["text"].as_str().and_then(parse) else {
        return;
    };
    item["text"] = json!(input);
    item["voiceTranscript"] = json!(entries);
}

fn parse(text: &str) -> Option<(String, Vec<Value>)> {
    if text.len() > MAX_BYTES {
        return None;
    }
    let mut body = text
        .trim()
        .strip_prefix("<realtime_delegation>")?
        .strip_suffix("</realtime_delegation>")?
        .trim();
    if let Some(rest) = body.strip_prefix("<source>transcript_tail_flush</source>") {
        body = rest.trim_start();
    }
    let (input, rest) = body.strip_prefix("<input>")?.split_once("</input>")?;
    let input = decode(input)?;
    let rest = rest.trim();
    let transcript = if rest.is_empty() {
        String::new()
    } else {
        decode(
            rest.strip_prefix("<transcript_delta>")?
                .strip_suffix("</transcript_delta>")?,
        )?
    };
    let entries = transcript_entries(&transcript)?;
    if entries.is_empty() {
        return (!input.trim().is_empty()).then(|| {
            let entries = vec![json!({"role":"user","text":input})];
            (input, entries)
        });
    }
    let text = if input.trim().is_empty() {
        transcript
    } else {
        input
    };
    Some((text, entries))
}

fn decode(text: &str) -> Option<String> {
    // Codex escapes only &, < and >. Decode once, keeping literal entities intact.
    if text.contains(['<', '>']) {
        return None;
    }
    Some(
        text.replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&amp;", "&"),
    )
}

fn transcript_entries(text: &str) -> Option<Vec<Value>> {
    let mut entries = Vec::new();
    let mut role = "unknown";
    let mut lines = Vec::new();
    for line in text.lines() {
        let next = line
            .strip_prefix("user: ")
            .map(|text| ("user", text))
            .or_else(|| {
                line.strip_prefix("assistant: ")
                    .map(|text| ("assistant", text))
            });
        if let Some((next_role, text)) = next {
            push_entry(&mut entries, role, &lines)?;
            lines.clear();
            role = next_role;
            lines.push(text);
        } else {
            lines.push(line);
        }
    }
    push_entry(&mut entries, role, &lines)?;
    Some(entries)
}

fn push_entry(entries: &mut Vec<Value>, role: &str, lines: &[&str]) -> Option<()> {
    let text = lines.join("\n");
    if !text.trim().is_empty() {
        if entries.len() >= MAX_ENTRIES {
            return None;
        }
        entries.push(json!({"role":role,"text":text}));
    }
    Some(())
}

#[cfg(test)]
mod tests;
