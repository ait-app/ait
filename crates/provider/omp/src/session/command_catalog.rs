//! Per-session native command ownership, read through the existing writer.

use provider_core::ports::agent_session::CommandRoute;
use serde_json::{Value, json};
use tokio::sync::oneshot;
use tokio::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

use crate::commands::{self, Catalog, Claim};
use crate::transport::{OptionalWrite, Transport};

const CATALOG_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
struct Pending {
    id: String,
    generation: u64,
    cancel: CancellationToken,
    written: Option<oneshot::Receiver<OptionalWrite>>,
    deadline: Instant,
}

/// The current session's complete catalog; absence is never assumed to be builtin ownership.
#[derive(Debug, Default)]
pub(super) struct Commands {
    catalog: Option<Catalog>,
    generation: u64,
    pending: Option<Pending>,
}

impl Commands {
    pub(super) fn refresh(&mut self, transport: &mut Transport) -> bool {
        if let Some(pending) = self.pending.take() {
            pending.cancel.cancel();
        }
        let changed = self.catalog.take().is_some();
        self.generation = self.generation.wrapping_add(1);
        let cancel = CancellationToken::new();
        if let Ok((id, written)) =
            transport.queue_optional("get_available_commands", json!({}), cancel.clone())
        {
            self.pending = Some(Pending {
                id,
                generation: self.generation,
                cancel,
                written: Some(written),
                deadline: Instant::now() + CATALOG_TIMEOUT,
            });
        }
        changed
    }

    pub(super) fn poll(&mut self) {
        let failed = self.pending.as_mut().is_some_and(|pending| {
            if pending.deadline <= Instant::now() {
                return true;
            }
            match pending.written.as_mut().map(oneshot::Receiver::try_recv) {
                Some(Ok(OptionalWrite::Sent)) => {
                    pending.written = None;
                    false
                }
                Some(
                    Ok(OptionalWrite::Unsent | OptionalWrite::Failed)
                    | Err(oneshot::error::TryRecvError::Closed),
                ) => true,
                None | Some(Err(oneshot::error::TryRecvError::Empty)) => false,
            }
        });
        if failed && let Some(pending) = self.pending.take() {
            pending.cancel.cancel();
        }
    }

    pub(super) fn response(&mut self, event: &Value) -> Option<bool> {
        if !self
            .pending
            .as_ref()
            .is_some_and(|pending| event["id"] == pending.id)
        {
            return None;
        }
        let pending = self.pending.take()?;
        if pending.deadline <= Instant::now() {
            pending.cancel.cancel();
            return Some(false);
        }
        if pending.generation == self.generation {
            let next = (event["command"] == "get_available_commands" && event["success"] == true)
                .then(|| Catalog::parse(&event["data"]).ok())
                .flatten();
            let changed = self.catalog != next;
            self.catalog = next;
            return Some(changed);
        }
        Some(false)
    }

    pub(super) fn update(&mut self, event: &Value) -> bool {
        self.generation = self.generation.wrapping_add(1);
        let next = Catalog::parse(event).ok();
        let changed = self.catalog != next;
        self.catalog = next;
        changed
    }

    pub(super) fn catalog(&self) -> Option<Vec<Value>> {
        self.catalog.as_ref().map(Catalog::project)
    }

    pub(super) fn route(&self, text: &str) -> Option<CommandRoute> {
        let (name, _) = commands::split(text)?;
        let Some(catalog) = &self.catalog else {
            return Some(
                if self.pending.as_ref().is_some_and(|pending| {
                    pending.generation == self.generation && pending.deadline > Instant::now()
                }) {
                    CommandRoute::Pending
                } else {
                    CommandRoute::Unavailable
                },
            );
        };
        let route = match catalog.claim(name) {
            Some(Claim::Ambiguous) => CommandRoute::Unavailable,
            Some(Claim::Builtin(canonical)) => {
                let (_, args) = commands::split(text).expect("validated slash text");
                let canonical_text = format!("/{canonical} {args}");
                if commands::gui_managed(text).is_some()
                    || commands::builtin(text).is_some()
                    || commands::gui_managed(&canonical_text).is_some()
                    || commands::builtin(&canonical_text).is_some()
                {
                    CommandRoute::OutOfBand
                } else {
                    CommandRoute::Foreground
                }
            }
            Some(Claim::Unknown(canonical)) => {
                let (_, args) = commands::split(text).expect("validated slash text");
                let canonical_text = format!("/{canonical} {args}");
                if commands::gui_managed(text).is_some()
                    || commands::builtin(text).is_some()
                    || commands::gui_managed(&canonical_text).is_some()
                    || commands::builtin(&canonical_text).is_some()
                {
                    CommandRoute::Unavailable
                } else {
                    CommandRoute::Foreground
                }
            }
            None if commands::builtin(text).is_some() => CommandRoute::OutOfBand,
            None if commands::gui_managed(text).is_some() => CommandRoute::Unavailable,
            Some(Claim::Native) | None => CommandRoute::Foreground,
        };
        Some(route)
    }

    pub(super) fn canonical_text(&self, text: &str) -> Option<String> {
        if commands::gui_managed(text).is_some() || commands::builtin(text).is_some() {
            return None;
        }
        let (name, args) = commands::split(text)?;
        let Some(Claim::Builtin(canonical)) = self.catalog.as_ref()?.claim(name) else {
            return None;
        };
        Some(if args.is_empty() {
            format!("/{canonical}")
        } else {
            format!("/{canonical} {args}")
        })
    }
}

#[cfg(test)]
mod tests;
