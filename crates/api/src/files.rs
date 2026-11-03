use std::collections::BTreeMap;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use filesystem::files::service::transfer::Cursor;
use futures_util::stream;
use model::server::ErrorCode;

use crate::Shared;

/// Serve a one-use download. The relay names the visiting account of a guest download in
/// [`model::share::DOWNLOAD_VISITOR_HEADER`]; tokens are bound to the account they were
/// issued for, so the header only narrows what a request may consume.
pub(crate) async fn download(
    State(state): State<Arc<Shared>>,
    headers: HeaderMap,
    Query(query): Query<BTreeMap<String, String>>,
) -> Response {
    let Some(token) = query.get("token").filter(|token| !token.trim().is_empty()) else {
        return (StatusCode::BAD_REQUEST, "Missing download token").into_response();
    };
    if query.len() != 1 {
        return (StatusCode::BAD_REQUEST, "Unexpected download parameter").into_response();
    }
    let token = token.trim().to_owned();
    let visitor = match headers.get(model::share::DOWNLOAD_VISITOR_HEADER) {
        None => None,
        Some(value) => match value.to_str() {
            Ok(visitor) if !visitor.is_empty() => Some(visitor.to_owned()),
            _ => return (StatusCode::BAD_REQUEST, "Invalid download visitor").into_response(),
        },
    };
    let result = state
        .run(
            state.filesystem.files.clone(),
            ErrorCode::ProjectIo,
            move |files| Ok(files.consume_download(&token, visitor.as_deref())),
        )
        .await;
    let (reader, file_name) = match result {
        Ok(Ok(download)) => download,
        Ok(Err(error)) if error.0 == "Invalid or expired token" => {
            return (StatusCode::FORBIDDEN, error.0).into_response();
        }
        Ok(Err(_)) => return (StatusCode::NOT_FOUND, "File not found").into_response(),
        Err(_) => {
            return (StatusCode::SERVICE_UNAVAILABLE, "File service unavailable").into_response();
        }
    };
    let info = reader.info().clone();
    let cursor = Cursor::new(reader);
    let tasks = state.tasks.clone();
    let cancel = state.cancellation.clone();
    let body = stream::unfold(Some(cursor), move |cursor| {
        let tasks = tasks.clone();
        let cancel = cancel.clone();
        async move {
            let cursor = cursor?;
            if cancel.is_cancelled() {
                return None;
            }
            match filesystem::files::connection::transfer::chunk(cursor, &tasks).await {
                Ok((next, Some(bytes))) => Some((Ok::<_, std::io::Error>(bytes), Some(next))),
                Ok((_, None)) => None,
                Err(error) => Some((Err(std::io::Error::other(error.0)), None)),
            }
        }
    });
    let name: String = file_name
        .chars()
        .map(|character| {
            if character.is_ascii()
                && !matches!(character, '"' | '\r' | '\n' | '\\')
                && !character.is_control()
            {
                character
            } else {
                '_'
            }
        })
        .collect();
    let mut response = Body::from_stream(body).into_response();
    let headers = response.headers_mut();
    if let Ok(value) = info.mime_type.parse() {
        headers.insert(header::CONTENT_TYPE, value);
    }
    if let Ok(value) = info.size.to_string().parse() {
        headers.insert(header::CONTENT_LENGTH, value);
    }
    if let Ok(value) = format!("attachment; filename=\"{name}\"").parse() {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    headers.insert(
        header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    headers.insert(
        "x-content-type-options",
        axum::http::HeaderValue::from_static("nosniff"),
    );
    response
}
