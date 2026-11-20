//! Markdown image extraction and bounded image loading for rich channel replies.

use std::path::{Path, PathBuf};

use base64::Engine;
use domain::bot::Turn;
use model::ErrorCode;
use reqwest::Client;
use url::Url;

const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;

/// One user-visible part of a Bot reply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ReplyPart {
    /// Markdown text that a channel can render using its normal text path.
    Text(String),
    /// A Markdown image whose source can be sent as a native image attachment.
    Image { alt: String, source: String },
}

/// Image bytes and metadata ready for a channel upload endpoint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ImageData {
    /// The bounded image bytes.
    pub(crate) bytes: Vec<u8>,
    /// A supported image MIME type.
    pub(crate) mime_type: &'static str,
    /// A safe filename for multipart upload APIs.
    pub(crate) file_name: String,
}

/// Split each visible assistant message into text and native-image parts.
pub(crate) fn reply_parts(turn: &Turn) -> Vec<ReplyPart> {
    let messages = if turn.messages.is_empty() {
        turn.reply.as_deref().into_iter().collect::<Vec<_>>()
    } else {
        turn.messages
            .iter()
            .filter(|message| !message.text.trim().is_empty())
            .map(|message| message.text.as_str())
            .collect()
    };
    messages
        .into_iter()
        .flat_map(parse_markdown_images)
        .collect()
}

/// Parse the simple image form produced by provider image stores while retaining surrounding
/// Markdown text. Malformed image syntax remains ordinary text.
fn parse_markdown_images(markdown: &str) -> Vec<ReplyPart> {
    let mut parts = Vec::new();
    let mut cursor = 0;
    while let Some(relative) = markdown[cursor..].find("![") {
        let start = cursor + relative;
        if start > 0 && markdown.as_bytes()[start - 1] == b'\\' {
            cursor = start + 2;
            continue;
        }
        let Some(label_end) = markdown[start + 2..].find("](") else {
            break;
        };
        let label_end = start + 2 + label_end;
        let source_start = label_end + 2;
        let Some(source_end) = closing_parenthesis(markdown, source_start) else {
            break;
        };
        let source = markdown[source_start..source_end]
            .trim()
            .split_once(char::is_whitespace)
            .map_or_else(
                || markdown[source_start..source_end].trim(),
                |(source, _)| source,
            )
            .trim_matches(|character| character == '<' || character == '>');
        if source.is_empty() {
            cursor = source_end + 1;
            continue;
        }
        push_text(&mut parts, &markdown[cursor..start]);
        parts.push(ReplyPart::Image {
            alt: markdown[start + 2..label_end].to_owned(),
            source: source.to_owned(),
        });
        cursor = source_end + 1;
    }
    push_text(&mut parts, &markdown[cursor..]);
    parts
}

fn closing_parenthesis(markdown: &str, source_start: usize) -> Option<usize> {
    let mut depth = 0;
    let mut escaped = false;
    for (offset, character) in markdown[source_start..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            continue;
        }
        match character {
            '(' => depth += 1,
            ')' if depth == 0 => return Some(source_start + offset),
            ')' => depth -= 1,
            _ => {}
        }
    }
    None
}

fn push_text(parts: &mut Vec<ReplyPart>, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(ReplyPart::Text(previous)) = parts.last_mut() {
        previous.push_str(text);
    } else {
        parts.push(ReplyPart::Text(text.to_owned()));
    }
}

/// Whether a source is a URL that a channel can fetch directly.
pub(crate) fn is_remote_source(source: &str) -> bool {
    Url::parse(source).is_ok_and(|url| matches!(url.scheme(), "http" | "https"))
}

/// Load a remote or provider-owned local image for a channel upload.
pub(crate) async fn load_image(
    client: &Client,
    image_directory: Option<&Path>,
    source: &str,
) -> Result<ImageData, ErrorCode> {
    if let Some(data_url) = source.strip_prefix("data:") {
        return decode_data_url(data_url);
    }
    if is_remote_source(source) {
        return load_remote_image(client, source).await;
    }
    let image_directory = image_directory.ok_or(ErrorCode::BotIo)?;
    let path = local_path(source)?;
    let root = std::fs::canonicalize(image_directory).map_err(|_| ErrorCode::BotIo)?;
    let path = std::fs::canonicalize(path).map_err(|_| ErrorCode::BotIo)?;
    if !path.starts_with(&root) {
        return Err(ErrorCode::BotIo);
    }
    let bytes = tokio::fs::read(&path).await.map_err(|_| ErrorCode::BotIo)?;
    image_data(bytes, mime_from_path(&path), file_name(&path))
}

fn local_path(source: &str) -> Result<PathBuf, ErrorCode> {
    if source.starts_with('/') {
        let url = Url::parse(&format!("file://{source}")).map_err(|_| ErrorCode::BotIo)?;
        return url.to_file_path().map_err(|()| ErrorCode::BotIo);
    }
    Ok(PathBuf::from(source))
}

async fn load_remote_image(client: &Client, source: &str) -> Result<ImageData, ErrorCode> {
    let url = Url::parse(source).map_err(|_| ErrorCode::BotIo)?;
    let response = client
        .get(url.clone())
        .send()
        .await
        .map_err(|_| ErrorCode::BotIo)?;
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|length| length > MAX_IMAGE_BYTES as u64)
    {
        return Err(ErrorCode::BotIo);
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
        .and_then(supported_mime);
    let bytes = response.bytes().await.map_err(|_| ErrorCode::BotIo)?;
    image_data(
        bytes.to_vec(),
        content_type.or_else(|| mime_from_path(Path::new(url.path()))),
        url.path()
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty())
            .map_or_else(|| "image".to_owned(), str::to_owned),
    )
}

fn decode_data_url(data_url: &str) -> Result<ImageData, ErrorCode> {
    let (header, data) = data_url.split_once(',').ok_or(ErrorCode::BotIo)?;
    let mime = header
        .strip_suffix(";base64")
        .and_then(supported_mime)
        .ok_or(ErrorCode::BotIo)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|_| ErrorCode::BotIo)?;
    image_data(bytes, Some(mime), "image".to_owned())
}

fn image_data(
    bytes: Vec<u8>,
    mime_type: Option<&'static str>,
    file_name: String,
) -> Result<ImageData, ErrorCode> {
    if bytes.is_empty() || bytes.len() > MAX_IMAGE_BYTES {
        return Err(ErrorCode::BotIo);
    }
    let mime_type = mime_type.ok_or(ErrorCode::BotIo)?;
    let extension = extension_for_mime(mime_type);
    let file_name = if Path::new(&file_name).extension().is_some() {
        file_name
    } else {
        format!("{file_name}.{extension}")
    };
    Ok(ImageData {
        bytes,
        mime_type,
        file_name,
    })
}

fn mime_from_path(path: &Path) -> Option<&'static str> {
    supported_mime(match path.extension()?.to_str()? {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        _ => return None,
    })
}

fn supported_mime(mime_type: &str) -> Option<&'static str> {
    match mime_type {
        "image/png" => Some("image/png"),
        "image/jpeg" => Some("image/jpeg"),
        "image/gif" => Some("image/gif"),
        "image/webp" => Some("image/webp"),
        "image/bmp" => Some("image/bmp"),
        "image/tiff" => Some("image/tiff"),
        _ => None,
    }
}

fn extension_for_mime(mime_type: &str) -> &str {
    match mime_type {
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        "image/tiff" => "tiff",
        _ => "png",
    }
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map_or_else(|| "image".to_owned(), str::to_owned)
}

#[cfg(test)]
mod tests;
