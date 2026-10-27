//! Ordered RPC-wait events with bounded memory and private, automatically removed spill files.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Seek, SeekFrom, Write};
use std::task::Poll;

use futures_util::FutureExt;
use serde_json::Value;
use tokio_util::task::AbortOnDropHandle;

use crate::ports::agent_session::AgentSessionError;

const MEMORY_EVENTS: usize = 128;
type ReadBatch = (Option<Spool>, VecDeque<Value>);

/// Retain notifications while waiting for RPC acknowledgments without blocking their arrival.
#[derive(Debug, Default)]
pub(super) struct Queue {
    memory: VecDeque<Value>,
    spool: Option<Spool>,
    reading: Option<AbortOnDropHandle<Result<ReadBatch, AgentSessionError>>>,
}

impl Queue {
    /// Await the oldest event without passing a pending spill read.
    /// Returns an error if temporary storage cannot be read or decoded.
    pub(super) async fn next(&mut self) -> Result<Option<Value>, AgentSessionError> {
        loop {
            match self.poll()? {
                Poll::Ready(event) => return Ok(event),
                Poll::Pending => {
                    let reading = self.reading.take().ok_or(AgentSessionError::Failed)?;
                    let (spool, batch) = reading.await.map_err(|_| AgentSessionError::Failed)??;
                    self.spool = spool;
                    self.memory = batch;
                }
            }
        }
    }

    /// Append `event` in wire order; disk I/O runs outside the async reactor.
    /// Returns an error when temporary storage cannot retain the event.
    pub(super) async fn push(&mut self, event: Value) -> Result<(), AgentSessionError> {
        if let Some(reading) = self.reading.take() {
            let (spool, batch) = reading.await.map_err(|_| AgentSessionError::Failed)??;
            self.spool = spool;
            self.memory = batch;
        }
        if self.spool.is_none() && self.memory.len() < MEMORY_EVENTS {
            self.memory.push_back(event);
            return Ok(());
        }
        let spool = self.spool.take();
        self.spool = Some(
            tokio::task::spawn_blocking(move || {
                let mut spool = match spool {
                    Some(spool) => spool,
                    None => Spool {
                        file: tempfile::tempfile().map_err(|_| AgentSessionError::Failed)?,
                        read_position: 0,
                        remaining: 0,
                    },
                };
                spool
                    .file
                    .seek(SeekFrom::End(0))
                    .map_err(|_| AgentSessionError::Failed)?;
                let mut writer = BufWriter::new(&mut spool.file);
                serde_json::to_writer(&mut writer, &event)
                    .map_err(|_| AgentSessionError::Failed)?;
                writer
                    .write_all(b"\n")
                    .map_err(|_| AgentSessionError::Failed)?;
                writer.flush().map_err(|_| AgentSessionError::Failed)?;
                drop(writer);
                spool.remaining += 1;
                Ok::<_, AgentSessionError>(spool)
            })
            .await
            .map_err(|_| AgentSessionError::Failed)??,
        );
        Ok(())
    }

    /// Poll in source order. Pending disk reads must precede newer transport messages.
    /// Returns an error if the spill file cannot be read or decoded.
    pub(super) fn poll(&mut self) -> Result<Poll<Option<Value>>, AgentSessionError> {
        if let Some(event) = self.memory.pop_front() {
            return Ok(Poll::Ready(Some(event)));
        }
        if let Some(reading) = &mut self.reading {
            let Some(result) = reading.now_or_never() else {
                return Ok(Poll::Pending);
            };
            self.reading = None;
            let (spool, batch) = result.map_err(|_| AgentSessionError::Failed)??;
            self.spool = spool;
            self.memory = batch;
            return Ok(Poll::Ready(self.memory.pop_front()));
        }
        if let Some(spool) = self.spool.take() {
            self.reading = Some(AbortOnDropHandle::new(tokio::task::spawn_blocking(
                move || spool.read(),
            )));
            return Ok(Poll::Pending);
        }
        Ok(Poll::Ready(None))
    }
}

#[derive(Debug)]
struct Spool {
    file: File,
    read_position: u64,
    remaining: usize,
}

impl Spool {
    fn read(mut self) -> Result<ReadBatch, AgentSessionError> {
        self.file
            .seek(SeekFrom::Start(self.read_position))
            .map_err(|_| AgentSessionError::Failed)?;
        let mut reader = BufReader::new(&mut self.file);
        let count = self.remaining.min(MEMORY_EVENTS);
        let mut batch = VecDeque::with_capacity(count);
        let mut bytes = Vec::new();
        for _ in 0..count {
            bytes.clear();
            let size = reader
                .read_until(b'\n', &mut bytes)
                .map_err(|_| AgentSessionError::Failed)?;
            if size == 0 || bytes.last() != Some(&b'\n') {
                return Err(AgentSessionError::Failed);
            }
            batch.push_back(serde_json::from_slice(&bytes).map_err(|_| AgentSessionError::Failed)?);
            self.read_position += size as u64;
        }
        self.remaining -= count;
        // Exhaustion closes the anonymous file, reclaiming all spool storage.
        Ok(((self.remaining > 0).then_some(self), batch))
    }
}

#[cfg(test)]
mod tests;
