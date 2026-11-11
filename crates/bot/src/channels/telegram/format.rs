use std::fmt::Write;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

pub(super) fn render(markdown: &str) -> String {
    let options =
        Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS;
    let mut output = String::with_capacity(markdown.len());
    let mut lists = Vec::new();
    for event in Parser::new_ext(markdown, options) {
        match event {
            Event::Start(tag) => start(&mut output, &mut lists, tag),
            Event::End(tag) => end(&mut output, &mut lists, tag),
            Event::Text(text) => escape(&mut output, &text, false),
            Event::Code(text) | Event::InlineMath(text) | Event::DisplayMath(text) => {
                output.push_str("<code>");
                escape(&mut output, &text, false);
                output.push_str("</code>");
            }
            Event::Html(html) | Event::InlineHtml(html) => escape(&mut output, &html, false),
            Event::SoftBreak | Event::HardBreak => output.push('\n'),
            Event::Rule => output.push_str("\n———\n"),
            Event::TaskListMarker(checked) => {
                output.push_str(if checked { "[x] " } else { "[ ] " });
            }
            Event::FootnoteReference(label) => {
                output.push('[');
                escape(&mut output, &label, false);
                output.push(']');
            }
        }
    }
    normalize_newlines(output.trim_end())
}

fn normalize_newlines(text: &str) -> String {
    let mut normalized = String::with_capacity(text.len());
    let mut consecutive = 0;
    for character in text.chars() {
        if character == '\n' {
            consecutive += 1;
            if consecutive > 2 {
                continue;
            }
        } else {
            consecutive = 0;
        }
        normalized.push(character);
    }
    normalized
}

fn start(output: &mut String, lists: &mut Vec<Option<u64>>, tag: Tag<'_>) {
    match tag {
        Tag::Heading { .. } | Tag::Strong | Tag::TableHead => output.push_str("<b>"),
        Tag::Emphasis => output.push_str("<i>"),
        Tag::Strikethrough => output.push_str("<s>"),
        Tag::BlockQuote(_) => output.push_str("<blockquote>"),
        Tag::CodeBlock(_) => output.push_str("<pre><code>"),
        Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
            output.push_str("<a href=\"");
            escape(output, &dest_url, true);
            output.push_str("\">");
        }
        Tag::List(first) => lists.push(first),
        Tag::Item => match lists.last_mut() {
            Some(Some(next)) => {
                write!(output, "{next}. ").expect("writing to a String cannot fail");
                *next = next.saturating_add(1);
            }
            Some(None) | None => output.push_str("• "),
        },
        Tag::Paragraph
        | Tag::HtmlBlock
        | Tag::FootnoteDefinition(_)
        | Tag::DefinitionList
        | Tag::DefinitionListTitle
        | Tag::DefinitionListDefinition
        | Tag::Table(_)
        | Tag::TableRow
        | Tag::TableCell
        | Tag::Superscript
        | Tag::Subscript
        | Tag::MetadataBlock(_) => {}
    }
}

fn end(output: &mut String, lists: &mut Vec<Option<u64>>, tag: TagEnd) {
    match tag {
        TagEnd::Heading(_) => output.push_str("</b>\n"),
        TagEnd::Strong => output.push_str("</b>"),
        TagEnd::Emphasis => output.push_str("</i>"),
        TagEnd::Strikethrough => output.push_str("</s>"),
        TagEnd::BlockQuote(_) => output.push_str("</blockquote>\n"),
        TagEnd::CodeBlock => output.push_str("</code></pre>\n"),
        TagEnd::Link | TagEnd::Image => output.push_str("</a>"),
        TagEnd::List(_) => {
            lists.pop();
            output.push('\n');
        }
        TagEnd::Item | TagEnd::Paragraph | TagEnd::Table => output.push('\n'),
        TagEnd::TableCell => output.push_str(" │ "),
        TagEnd::TableHead => {
            trim_table_separator(output);
            output.push_str("</b>\n");
        }
        TagEnd::TableRow => {
            trim_table_separator(output);
            output.push('\n');
        }
        TagEnd::HtmlBlock
        | TagEnd::FootnoteDefinition
        | TagEnd::DefinitionList
        | TagEnd::DefinitionListTitle
        | TagEnd::DefinitionListDefinition
        | TagEnd::Superscript
        | TagEnd::Subscript
        | TagEnd::MetadataBlock(_) => {}
    }
}

fn trim_table_separator(output: &mut String) {
    if output.ends_with(" │ ") {
        output.truncate(output.len() - " │ ".len());
    }
}

fn escape(output: &mut String, text: &str, attribute: bool) {
    for character in text.chars() {
        match character {
            '&' => output.push_str("&amp;"),
            '<' => output.push_str("&lt;"),
            '>' => output.push_str("&gt;"),
            '"' if attribute => output.push_str("&quot;"),
            character => output.push(character),
        }
    }
}

#[cfg(test)]
mod tests;
