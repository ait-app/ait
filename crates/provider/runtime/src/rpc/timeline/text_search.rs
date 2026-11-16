//! Rendered-text estimates for Paseo chat Find; the client verifies visible occurrences.

use std::borrow::Cow;

use model::ErrorCode;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use regex::{Regex, RegexBuilder};

/// Compile a literal, case-insensitive query with flexible inter-word whitespace.
/// Returns no pattern for whitespace-only queries and rejects queries over 4096 normalized bytes.
pub(super) fn pattern(query: &str) -> Result<Option<Regex>, ErrorCode> {
    let normalized = query.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.len() > 4096 {
        return Err(ErrorCode::InvalidMessage);
    }
    if normalized.is_empty() {
        return Ok(None);
    }
    let source = normalized
        .split(' ')
        .map(regex::escape)
        .collect::<Vec<_>>()
        .join(r"\s+");
    RegexBuilder::new(&source)
        .case_insensitive(true)
        .build()
        .map(Some)
        .map_err(|_| ErrorCode::InvalidMessage)
}

/// Count non-overlapping hits in literal user text or separate rendered assistant blocks.
/// Markdown syntax, link destinations and image alt text do not contribute occurrences.
pub(super) fn count(pattern: &Regex, text: &str, markdown: bool) -> usize {
    let text = if text.contains('\r') {
        Cow::Owned(text.replace('\r', ""))
    } else {
        Cow::Borrowed(text)
    };
    if !markdown {
        return pattern.find_iter(&text).count();
    }
    let mut block = String::new();
    let mut count = 0;
    let mut image_depth = 0;
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    for event in Parser::new_ext(&text, options) {
        match event {
            Event::Start(Tag::Image { .. }) => image_depth += 1,
            Event::End(TagEnd::Image) => image_depth -= 1,
            _ if image_depth > 0 => {}
            Event::Text(text) | Event::Code(text) | Event::Html(text) | Event::InlineHtml(text) => {
                block.push_str(&text);
            }
            Event::SoftBreak | Event::HardBreak => block.push('\n'),
            Event::Start(tag) if is_block(tag.to_end()) => {
                count += pattern.find_iter(&block).count();
                block.clear();
            }
            Event::End(tag) if is_block(tag) => {
                count += pattern.find_iter(&block).count();
                block.clear();
            }
            Event::Start(_)
            | Event::End(_)
            | Event::InlineMath(_)
            | Event::DisplayMath(_)
            | Event::FootnoteReference(_)
            | Event::Rule
            | Event::TaskListMarker(_) => {}
        }
    }
    count + pattern.find_iter(&block).count()
}

fn is_block(tag: TagEnd) -> bool {
    match tag {
        TagEnd::Emphasis
        | TagEnd::Strong
        | TagEnd::Strikethrough
        | TagEnd::Superscript
        | TagEnd::Subscript
        | TagEnd::Link
        | TagEnd::Image => false,
        TagEnd::Paragraph
        | TagEnd::Heading(_)
        | TagEnd::BlockQuote(_)
        | TagEnd::CodeBlock
        | TagEnd::HtmlBlock
        | TagEnd::List(_)
        | TagEnd::Item
        | TagEnd::FootnoteDefinition
        | TagEnd::DefinitionList
        | TagEnd::DefinitionListTitle
        | TagEnd::DefinitionListDefinition
        | TagEnd::Table
        | TagEnd::TableHead
        | TagEnd::TableRow
        | TagEnd::TableCell
        | TagEnd::MetadataBlock(_) => true,
    }
}

#[cfg(test)]
mod tests;
