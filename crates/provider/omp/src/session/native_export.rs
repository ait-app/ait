//! Native HTML rendering through the existing OMP writer and bounded private staging.

use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use provider_core::ports::agent_session::AgentSessionError;
use provider_core::ports::native_html_export::{
    MAX_NATIVE_HTML_EXPORT_BYTES, NativeHtmlExport, NativeHtmlExportError,
};
use serde_json::{Value, json};

use super::super::transport::OptionalWrite;
use super::Transport;

/// Bound abandoned records until their native responses arrive or the process is reaped.
const MAX_LATE_EXPORTS: usize = 64;
/// Abandoned blocking filesystem operations still occupy a slot until the OS call returns.
const MAX_ADOPTIONS: usize = 2;

fn adoption_budget() -> Arc<tokio::sync::Semaphore> {
    static BUDGET: OnceLock<Arc<tokio::sync::Semaphore>> = OnceLock::new();
    Arc::clone(BUDGET.get_or_init(|| Arc::new(tokio::sync::Semaphore::new(MAX_ADOPTIONS))))
}

#[derive(Debug)]
struct Export {
    id: String,
    directory: tempfile::TempDir,
    output: PathBuf,
    file_name: String,
    deadline: Instant,
    cancelled: tokio_util::sync::CancellationToken,
    written: Option<tokio::sync::oneshot::Receiver<OptionalWrite>>,
}

#[derive(Debug)]
struct Adoption {
    deadline: Instant,
    cancelled: tokio_util::sync::CancellationToken,
    result: tokio::sync::oneshot::Receiver<Result<NativeHtmlExport, NativeHtmlExportError>>,
}

/// One pending request and retained ownership of every unanswered abandoned request.
#[derive(Debug, Default)]
pub(super) struct Exports {
    current: Option<Export>,
    late: Vec<Export>,
    adoption: Option<Adoption>,
    result: Option<Result<NativeHtmlExport, NativeHtmlExportError>>,
}

impl Exports {
    pub(super) fn begin(
        &mut self,
        transport: &mut Transport,
        root: &Path,
    ) -> Result<(), AgentSessionError> {
        if self.current.is_some()
            || self.adoption.is_some()
            || self.result.is_some()
            || self.late.len() >= MAX_LATE_EXPORTS
        {
            return Err(AgentSessionError::Busy);
        }
        // TempDir creates a private directory, so only this session owns its entries. It
        // remains alive after cancellation until the late native response or process exit.
        let directory = tempfile::Builder::new()
            .prefix("ait-native-export-")
            .tempdir_in(root)
            .map_err(|_| AgentSessionError::Failed)?;
        let file_name = format!("ait-native-{}.html", uuid::Uuid::new_v4());
        let output = directory.path().join(&file_name);
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&output)
            .map_err(|_| AgentSessionError::Failed)?;
        let path = output.to_str().ok_or(AgentSessionError::Rejected)?;
        let cancelled = tokio_util::sync::CancellationToken::new();
        let (id, written) = transport.queue_optional(
            "export_html",
            json!({"outputPath":path}),
            cancelled.clone(),
        )?;
        self.current = Some(Export {
            id,
            directory,
            output,
            file_name,
            deadline: Instant::now() + transport.timeouts().request,
            cancelled,
            written: Some(written),
        });
        Ok(())
    }

    /// Consume only responses owned by this export; unrelated native responses pass through.
    pub(super) fn response(&mut self, event: &Value) -> bool {
        let Some(id) = event["id"].as_str() else {
            return false;
        };
        if let Some(index) = self.late.iter().position(|export| export.id == id) {
            self.late.remove(index);
            return true;
        }
        if self.current.as_ref().is_none_or(|export| export.id != id) {
            return false;
        }
        if let Some(export) = self.current.take() {
            if event["command"] != "export_html"
                || event["success"] != true
                || export.output.to_str().is_none()
                || event["data"]["path"].as_str() != export.output.to_str()
            {
                self.result = Some(Err(NativeHtmlExportError::Rejected));
            } else {
                self.start_adoption(export, adoption_budget(), adopt);
            }
        }
        true
    }

    fn start_adoption<F>(&mut self, export: Export, budget: Arc<tokio::sync::Semaphore>, adopt: F)
    where
        F: FnOnce(&Export) -> Result<NativeHtmlExport, NativeHtmlExportError> + Send + 'static,
    {
        let Ok(permit) = budget.try_acquire_owned() else {
            self.result = Some(Err(NativeHtmlExportError::Busy));
            return;
        };
        let deadline = export.deadline;
        let cancelled = export.cancelled.clone();
        let (sender, result) = tokio::sync::oneshot::channel();
        let worker = std::thread::Builder::new()
            .name("omp-html-export".to_owned())
            .spawn(move || {
                let _permit = permit;
                let outcome = adopt(&export);
                drop(export);
                let _ = sender.send(outcome);
            });
        match worker {
            Ok(_) => {
                self.adoption = Some(Adoption {
                    deadline,
                    cancelled,
                    result,
                });
            }
            Err(_) => self.result = Some(Err(NativeHtmlExportError::Io)),
        }
    }

    pub(super) fn expire(&mut self) {
        if let Some(adoption) = &mut self.adoption {
            match adoption.result.try_recv() {
                Ok(result) => {
                    self.result = Some(result);
                    self.adoption = None;
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                    self.result = Some(Err(NativeHtmlExportError::Io));
                    self.adoption = None;
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty)
                    if Instant::now() >= adoption.deadline =>
                {
                    adoption.cancelled.cancel();
                    self.result = Some(Err(NativeHtmlExportError::Io));
                    self.adoption = None;
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
            }
        }
        self.late.retain_mut(|export| match write_status(export) {
            Some(OptionalWrite::Unsent) => false,
            Some(OptionalWrite::Sent | OptionalWrite::Failed) | None => true,
        });
        let status = self.current.as_mut().and_then(write_status);
        if matches!(status, Some(OptionalWrite::Unsent)) {
            self.current.take();
            self.result = Some(Err(NativeHtmlExportError::Io));
            return;
        }
        if matches!(status, Some(OptionalWrite::Failed)) {
            self.abandon();
            self.result = Some(Err(NativeHtmlExportError::Io));
            return;
        }
        if self
            .current
            .as_ref()
            .is_some_and(|export| Instant::now() >= export.deadline)
        {
            self.abandon();
            self.result = Some(Err(NativeHtmlExportError::Io));
        }
    }

    pub(super) fn take(&mut self) -> Option<Result<NativeHtmlExport, NativeHtmlExportError>> {
        self.result.take()
    }

    pub(super) fn cancel(&mut self) {
        self.result = None;
        if let Some(adoption) = self.adoption.take() {
            adoption.cancelled.cancel();
        }
        self.abandon();
    }

    fn abandon(&mut self) {
        if let Some(export) = self.current.take() {
            export.cancelled.cancel();
            let _ = std::fs::remove_file(&export.output);
            self.late.push(export);
        }
    }

    /// Clean immediately only after observing native exit; otherwise retain the paths until
    /// exit is observed, even if the runtime stops the cleanup task unexpectedly.
    pub(super) fn after_close(&mut self, transport: &mut Transport) {
        if let Some(adoption) = self.adoption.take() {
            adoption.cancelled.cancel();
            self.result = Some(Err(NativeHtmlExportError::Io));
        }
        if transport.exit_confirmed() {
            self.closed();
            return;
        }
        if self.current.is_none() && self.late.is_empty() {
            return;
        }
        let (exit, watcher) = transport.take_exit_watcher();
        self.defer_cleanup(exit, watcher);
    }

    fn defer_cleanup(
        &mut self,
        mut exit: tokio::sync::watch::Receiver<Option<std::process::ExitStatus>>,
        watcher: Option<tokio_util::task::AbortOnDropHandle<()>>,
    ) {
        let current = self.current.take();
        if current.is_some() {
            self.result = Some(Err(NativeHtmlExportError::Io));
        }
        let paths: Vec<_> = current
            .into_iter()
            .chain(self.late.drain(..))
            .map(|export| {
                export.cancelled.cancel();
                // A dropped Tokio task must not accidentally unlink an unconfirmed writer's
                // directory. Keep it until we have a real process-exit observation.
                export.directory.keep()
            })
            .collect();
        tokio::spawn(async move {
            let _watcher = watcher;
            if exit.wait_for(Option::is_some).await.is_ok() {
                let _ = tokio::task::spawn_blocking(move || {
                    for path in paths {
                        let _ = std::fs::remove_dir_all(path);
                    }
                })
                .await;
            }
        });
    }

    /// The native process is reaped, so no request can create a late staging file.
    fn closed(&mut self) {
        if self.current.take().is_some() {
            self.result = Some(Err(NativeHtmlExportError::Io));
        }
        self.late.clear();
    }
}

fn write_status(export: &mut Export) -> Option<OptionalWrite> {
    let receiver = export.written.as_mut()?;
    match receiver.try_recv() {
        Ok(status) => {
            export.written = None;
            Some(status)
        }
        Err(tokio::sync::oneshot::error::TryRecvError::Empty) => None,
        Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
            export.written = None;
            Some(OptionalWrite::Failed)
        }
    }
}

/// Copy the exact regular-file bytes from the private directory to an anonymous artifact.
fn adopt(export: &Export) -> Result<NativeHtmlExport, NativeHtmlExportError> {
    let directory =
        std::fs::canonicalize(export.directory.path()).map_err(|_| NativeHtmlExportError::Io)?;
    let parent = export
        .output
        .parent()
        .ok_or(NativeHtmlExportError::Rejected)?;
    if std::fs::canonicalize(parent).map_err(|_| NativeHtmlExportError::Io)? != directory
        || export.output.file_name().and_then(|name| name.to_str()) != Some(&export.file_name)
    {
        return Err(NativeHtmlExportError::Rejected);
    }
    let metadata =
        std::fs::symlink_metadata(&export.output).map_err(|_| NativeHtmlExportError::Io)?;
    if !metadata.is_file() {
        return Err(NativeHtmlExportError::Rejected);
    }
    let mut source = open_nofollow(&export.output).map_err(|_| NativeHtmlExportError::Rejected)?;
    let metadata = source.metadata().map_err(|_| NativeHtmlExportError::Io)?;
    if !metadata.is_file() {
        return Err(NativeHtmlExportError::Rejected);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // Opened with FILE_FLAG_OPEN_REPARSE_POINT below: reject any reparse data on the
        // retained handle, including junctions and links beyond ordinary symlinks.
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(NativeHtmlExportError::Rejected);
        }
    }
    if metadata.len() > MAX_NATIVE_HTML_EXPORT_BYTES {
        return Err(NativeHtmlExportError::TooLarge);
    }
    let mut file = tempfile::tempfile().map_err(|_| NativeHtmlExportError::Io)?;
    let mut size = 0_u64;
    let mut buffer = [0_u8; 16 * 1024];
    loop {
        if export.cancelled.is_cancelled() {
            return Err(NativeHtmlExportError::Io);
        }
        let count = source
            .read(&mut buffer)
            .map_err(|_| NativeHtmlExportError::Io)?;
        if count == 0 {
            break;
        }
        size = size.saturating_add(count as u64);
        if size > MAX_NATIVE_HTML_EXPORT_BYTES {
            return Err(NativeHtmlExportError::TooLarge);
        }
        file.write_all(&buffer[..count])
            .map_err(|_| NativeHtmlExportError::Io)?;
    }
    file.rewind().map_err(|_| NativeHtmlExportError::Io)?;
    Ok(NativeHtmlExport {
        file,
        file_name: export.file_name.clone(),
        size,
    })
}

#[cfg(unix)]
fn open_nofollow(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
}

#[cfg(windows)]
fn open_nofollow(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
    std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

#[cfg(not(any(unix, windows)))]
fn open_nofollow(_path: &Path) -> std::io::Result<std::fs::File> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "native export requires no-follow open",
    ))
}

#[cfg(test)]
mod tests;
