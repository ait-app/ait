//! Local automatic incident capture. The tracing callback never performs file or process I/O.

mod store;

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use metadata::ports::diagnostics::DaemonDiagnostics;
use tracing::field::{Field, Visit};
use tracing_subscriber::Layer;

use store::REPORT_LIMIT;

const EVENT_LIMIT: usize = 128;
const CAPTURE_INTERVAL: Duration = Duration::from_secs(30);

trait EvidenceSource: Send {
    fn collect(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Pin<Box<dyn Future<Output = String> + '_>>;
}

impl EvidenceSource for provider::diagnostics::HarnessEvidence {
    fn collect(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Pin<Box<dyn Future<Output = String> + '_>> {
        Box::pin(self.collect(start, end))
    }
}

#[derive(Debug)]
struct Event {
    at: DateTime<Utc>,
    text: String,
}

#[derive(Debug, Default)]
struct Recent {
    events: VecDeque<Event>,
    last_capture: Option<Instant>,
    overwritten: usize,
}

#[derive(Debug)]
enum Job {
    Incident(DateTime<Utc>),
    Report(mpsc::SyncSender<String>),
}

#[derive(Clone, Debug)]
pub(crate) struct Diagnostics {
    sender: mpsc::SyncSender<Job>,
    recent: Arc<Mutex<Recent>>,
    dropped: Arc<AtomicUsize>,
    storage_failures: Arc<AtomicUsize>,
}

impl Diagnostics {
    /// Start a bounded evidence worker using `directory` for sanitized incident files.
    /// Returns the tracing layer / report port, or a thread creation error.
    pub(crate) fn start(directory: PathBuf) -> std::io::Result<Self> {
        Self::with_sources(directory, provider::diagnostics::HarnessEvidence::default())
    }

    fn with_sources(
        directory: PathBuf,
        sources: impl EvidenceSource + 'static,
    ) -> std::io::Result<Self> {
        let (sender, receiver) = mpsc::sync_channel(4);
        let recent = Arc::new(Mutex::new(Recent::default()));
        let dropped = Arc::new(AtomicUsize::new(0));
        let storage_failures = Arc::new(AtomicUsize::new(0));
        let worker_storage_failures = storage_failures.clone();
        let worker_recent = recent.clone();
        let worker_dropped = dropped.clone();
        std::thread::Builder::new()
            .name("diagnostic-evidence".into())
            .spawn(move || {
                Worker {
                    directory,
                    sources: Box::new(sources),
                    recent: worker_recent,
                    dropped: worker_dropped,
                    storage_failures: worker_storage_failures,
                }
                .run(&receiver);
            })?;
        Ok(Self {
            sender,
            recent,
            dropped,
            storage_failures,
        })
    }

    fn record(&self, text: String, at: DateTime<Utc>) {
        let Ok(mut recent) = self.recent.lock() else {
            return;
        };
        if recent.events.len() == EVENT_LIMIT {
            recent.events.pop_front();
            recent.overwritten += 1;
        }
        recent.events.push_back(Event { at, text });
        if recent
            .last_capture
            .is_none_or(|time| time.elapsed() >= CAPTURE_INTERVAL)
        {
            if self.sender.try_send(Job::Incident(at)).is_ok() {
                recent.last_capture = Some(Instant::now());
            } else {
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
        }
    }
}

struct Worker {
    directory: PathBuf,
    sources: Box<dyn EvidenceSource>,
    recent: Arc<Mutex<Recent>>,
    dropped: Arc<AtomicUsize>,
    storage_failures: Arc<AtomicUsize>,
}

impl Worker {
    fn run(self, receiver: &mpsc::Receiver<Job>) {
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return;
        };
        let mut cache: Option<(Instant, String)> = None;
        let mut captured_through = Utc::now();
        let mut maintenance = Instant::now();
        let _ = store::prune(&self.directory, Utc::now());
        loop {
            match receiver.recv_timeout(Duration::from_secs(1)) {
                Ok(Job::Incident(at)) => {
                    captured_through = Utc::now();
                    self.save(&runtime, at);
                    cache = None;
                }
                Ok(Job::Report(reply)) => {
                    if cache
                        .as_ref()
                        .is_none_or(|(time, _)| time.elapsed() >= CAPTURE_INTERVAL)
                    {
                        let mut report = self.capture(&runtime, Utc::now());
                        match store::prune(&self.directory, Utc::now()) {
                            Ok(()) => report.push_str(&sanitize(&store::recent(&self.directory))),
                            Err(_) => report.push_str("\nIncident storage unavailable; automatic evidence may not have been saved\n"),
                        }
                        cache = Some((Instant::now(), report));
                    }
                    if let Some((_, report)) = &cache {
                        let _ = reply.try_send(report.clone());
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            if maintenance.elapsed() >= CAPTURE_INTERVAL {
                let _ = store::prune(&self.directory, Utc::now());
                if let Some(at) = self.pending(captured_through) {
                    captured_through = Utc::now();
                    self.save(&runtime, at);
                    cache = None;
                }
                maintenance = Instant::now();
            }
        }
    }

    fn pending(&self, captured_through: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let mut recent = self.recent.lock().ok()?;
        let at = recent.events.back()?.at;
        if at <= captured_through
            || recent
                .last_capture
                .is_some_and(|time| time.elapsed() < CAPTURE_INTERVAL)
        {
            return None;
        }
        recent.last_capture = Some(Instant::now());
        Some(at)
    }

    fn capture(&self, runtime: &tokio::runtime::Runtime, at: DateTime<Utc>) -> String {
        capture(
            runtime,
            self.sources.as_ref(),
            &self.recent,
            &self.dropped,
            at,
        )
    }

    fn save(&self, runtime: &tokio::runtime::Runtime, at: DateTime<Utc>) {
        let report = self.capture(runtime, at);
        if store::save(&self.directory, &report, Utc::now()).is_err() {
            self.storage_failures.fetch_add(1, Ordering::Relaxed);
        }
    }
}

impl DaemonDiagnostics for Diagnostics {
    fn report(&self) -> String {
        let (sender, receiver) = mpsc::sync_channel(1);
        if self.sender.try_send(Job::Report(sender)).is_err() {
            return "Evidence collector busy or unavailable; retry shortly\n".into();
        }
        let report = receiver
            .recv_timeout(Duration::from_secs(15))
            .unwrap_or_else(|_| "Evidence collection timed out; retry shortly\n".into());
        format!(
            "Incident persistence failures: {}\n{report}",
            self.storage_failures.load(Ordering::Relaxed)
        )
    }
}

impl<S: tracing::Subscriber> Layer<S> for Diagnostics {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _context: tracing_subscriber::layer::Context<'_, S>,
    ) {
        if *event.metadata().level() > tracing::Level::WARN {
            return;
        }
        let mut fields = Fields(String::new());
        event.record(&mut fields);
        let text = sanitize(&format!(
            "{} {} {}",
            event.metadata().level(),
            event.metadata().target(),
            fields.0
        ));
        self.record(text, Utc::now());
    }
}

struct Fields(String);

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        // Only diagnostic fields, never request parameters, environment or native payloads.
        if matches!(
            field.name(),
            "message"
                | "error"
                | "error_code"
                | "request_id"
                | "provider"
                | "handler"
                | "fault"
                | "reason"
                | "operation"
                | "agent_id"
        ) {
            let mut bounded = LimitedText(String::new());
            if write!(bounded, "{value:?}").is_err() {
                bounded.0 = "[oversized field omitted]".into();
            }
            let _ = write!(self.0, "{}={} ", field.name(), bounded.0);
        }
    }
}

struct LimitedText(String);
impl std::fmt::Write for LimitedText {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        let remaining = 2048_usize.saturating_sub(self.0.len());
        if text.len() > remaining {
            return Err(std::fmt::Error);
        }
        self.0.push_str(text);
        Ok(())
    }
}

fn capture(
    runtime: &tokio::runtime::Runtime,
    sources: &dyn EvidenceSource,
    recent: &Mutex<Recent>,
    dropped: &AtomicUsize,
    at: DateTime<Utc>,
) -> String {
    let start = at - chrono::Duration::minutes(5);
    // Include records emitted while an error was being propagated to the collector.
    let end = Utc::now();
    let mut report = format!(
        "\nAit incident evidence v1\nIncident ID: {}\nCollected at: {end}\nIncident at: {at}\nUTC window: {start} through {end}\nNaive native timestamps are interpreted as UTC; untimestamped records are omitted.\nDaemon version: {}\nOS: {} / {}\nRetention: 7 days, 20 incidents, 5 MiB; automatic capture at most once per 30 seconds\nDropped capture jobs: {}\n",
        uuid::Uuid::new_v4(),
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        dropped.load(Ordering::Relaxed)
    );
    if let Ok(mut recent) = recent.lock() {
        recent
            .events
            .retain(|event| event.at >= end - chrono::Duration::days(7));
        let _ = writeln!(report, "Overwritten event records: {}", recent.overwritten);
        for event in recent
            .events
            .iter()
            .filter(|event| event.at >= start && event.at <= end)
        {
            let _ = writeln!(report, "{} {}", event.at, event.text);
        }
    }
    report.push_str(&runtime.block_on(sources.collect(start, end)));
    let mut safe = sanitize(&report);
    if safe.len() > REPORT_LIMIT {
        let end = safe[..safe.floor_char_boundary(REPORT_LIMIT - 64)]
            .rfind('\n')
            .unwrap_or(0);
        safe.truncate(end);
        safe.push_str("\n[report truncated at 256 KiB]\n");
    }
    safe
}

fn sanitize(text: &str) -> String {
    text.lines()
        .map(|line| {
            let lower = line.to_ascii_lowercase();
            if [
                "authorization",
                "bearer ",
                "password",
                "secret",
                "api_key",
                "apikey",
                "api-key",
                "access_token",
                "refresh_token",
                "token",
                "cookie",
                "private key",
                "prompt",
                "\"content\"",
                "sk-",
                "ghp_",
                "github_pat_",
                "-----begin",
            ]
            .iter()
            .any(|key| lower.contains(key))
            {
                "[sensitive log line omitted]".to_owned()
            } else {
                // Home paths, URLs and email addresses are not needed to correlate incidents.
                line.split_whitespace()
                    .map(|word| {
                        if word.contains("://")
                            || word.contains('@')
                            || word.contains("/home/")
                            || word.contains("/Users/")
                            || word.contains("\\Users\\")
                        {
                            "[private location omitted]"
                        } else {
                            word
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

#[cfg(test)]
mod tests;
