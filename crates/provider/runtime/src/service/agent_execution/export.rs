//! Read-only, bounded rendering of a committed Timeline snapshot, outside native command lanes.

use std::io::{BufWriter, Seek, Write};
use std::sync::Arc;

use model::ErrorCode;
use model::download::{GeneratedArtifact, MAX_ARTIFACT_BYTES};
use tokio_util::sync::CancellationToken;

use crate::ports::timeline_export::{ExportedTimeline, Format, Request, Scope};
use crate::timeline_projection::Entry;

use super::AgentExecution;

mod display;

impl AgentExecution {
    /// Render saved rows without restoring a provider, invoking native input, or acquiring a writer.
    /// # Errors
    /// Returns identity/storage, cancellation, capacity, or explicit complete-export size failures.
    pub(crate) async fn export_timeline(
        &self,
        request: Request,
    ) -> Result<ExportedTimeline, ErrorCode> {
        let permit = Arc::clone(&self.0.exports)
            .try_acquire_owned()
            .map_err(|_| ErrorCode::ResourceExhausted)?;
        let cancellation = self.0.cancellation.child_token();
        let _cancel_on_drop = cancellation.clone().drop_guard();
        let worker = Arc::clone(&self.0);
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let directory = worker
                .template
                .lock()
                .map_err(|_| ErrorCode::AgentIo)?
                .directory
                .clone();
            let resolved = directory
                .get_readable(&request.agent_id)
                .map_err(|error| ErrorCode::from(crate::rpc::agent_runtime::map_error(&error)))?;
            let rows = worker
                .timeline
                .export_rows(&resolved.agent.id, &cancellation)?;
            let entries = crate::timeline_projection::project(&rows);
            drop(rows);
            render(
                &resolved.agent.id,
                request.format,
                request.scope,
                &entries,
                cancellation,
            )
        })
        .await
        .map_err(|_| ErrorCode::AgentIo)?
    }
}

fn render(
    agent_id: &str,
    format: Format,
    scope: Scope,
    entries: &[Entry],
    cancellation: CancellationToken,
) -> Result<ExportedTimeline, ErrorCode> {
    let file = tempfile::tempfile().map_err(|error| io(&error))?;
    let mut writer = BufWriter::with_capacity(
        32 * 1024,
        LimitedWriter {
            file,
            bytes: 0,
            cancellation,
        },
    );
    let selection = match scope {
        Scope::Conversation => {
            "Conversation only: saved user/assistant messages and voice transcripts."
        }
        Scope::Timeline => "Whole saved Timeline: all durable display items and result cards.",
    };
    match format {
        Format::Markdown => writeln!(writer, "# Ait saved Timeline\n\n{selection}\n\nPoint-in-time persisted content; no native re-execution or unsaved editor state.\n"),
        Format::Html => writeln!(writer, "<!doctype html><html><head><meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; base-uri 'none'; form-action 'none'\"><title>Ait saved Timeline</title></head><body><h1>Ait saved Timeline</h1><p>{selection}</p><p>Point-in-time persisted content; no native re-execution or unsaved editor state.</p>"),
    }.map_err(|error| io(&error))?;
    let mut count = 0;
    for entry in entries {
        if display::entry(&mut writer, format, scope, entry)? {
            count += 1;
        }
    }
    if format == Format::Html {
        writeln!(writer, "</body></html>").map_err(|error| io(&error))?;
    }
    writer.flush().map_err(|error| io(&error))?;
    let mut completed = writer
        .into_inner()
        .map_err(|error| io(&error.into_error()))?;
    completed.file.rewind().map_err(|error| io(&error))?;
    let (extension, mime_type) = match format {
        Format::Markdown => ("md", "text/markdown; charset=utf-8"),
        Format::Html => ("html", "text/html; charset=utf-8"),
    };
    Ok(ExportedTimeline {
        artifact: GeneratedArtifact {
            agent_id: agent_id.to_owned(),
            file_name: format!("ait-timeline-{}.{}", uuid::Uuid::new_v4(), extension),
            mime_type: mime_type.to_owned(),
            size: completed.bytes,
            file: completed.file,
        },
        format,
        scope,
        entries: count,
    })
}

fn escape_html(writer: &mut impl Write, encoded: &[u8]) -> std::io::Result<()> {
    let mut start = 0;
    for (index, byte) in encoded.iter().enumerate() {
        let escaped: &[u8] = match byte {
            b'&' => b"&amp;",
            b'<' => b"&lt;",
            b'>' => b"&gt;",
            b'\"' => b"&quot;",
            b'\'' => b"&#39;",
            _ => continue,
        };
        writer.write_all(&encoded[start..index])?;
        writer.write_all(escaped)?;
        start = index + 1;
    }
    writer.write_all(&encoded[start..])
}

struct LimitedWriter {
    file: std::fs::File,
    bytes: u64,
    cancellation: CancellationToken,
}

impl Write for LimitedWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        if self.cancellation.is_cancelled() {
            return Err(std::io::Error::other("Timeline export cancelled"));
        }
        if self.bytes.saturating_add(buffer.len() as u64) > MAX_ARTIFACT_BYTES {
            return Err(std::io::Error::from(std::io::ErrorKind::FileTooLarge));
        }
        let written = self.file.write(buffer)?;
        self.bytes += written as u64;
        Ok(written)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}

fn io(error: &std::io::Error) -> ErrorCode {
    if error.kind() == std::io::ErrorKind::FileTooLarge {
        ErrorCode::ResourceExhausted
    } else {
        ErrorCode::AgentIo
    }
}

#[cfg(test)]
mod tests;
