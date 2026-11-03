//! Automatically prepared, local ONNX speech with reusable, cancellable worker processes.

mod catalog;
mod download;
mod engine;
mod worker;

use std::{path::PathBuf, sync::Arc};

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::{
    Error,
    audio::{Audio, Format, MAX_AUDIO_BYTES, MAX_TEXT_BYTES},
    local::read_bounded,
    ports::{Synthesizer, Transcriber, Transcript},
};
use catalog::Model;
use download::Preparation;
use worker::{Init, Worker};

pub use worker::run as run_worker;

#[derive(Debug)]
pub(crate) struct Offline {
    model: Model,
    preparation: Preparation,
    program: PathBuf,
    worker: Mutex<Option<Worker>>,
}

impl Offline {
    pub(crate) fn transcriber(
        root: PathBuf,
        program: PathBuf,
        id: &str,
    ) -> Result<Arc<Self>, Error> {
        Ok(Arc::new(Self::new(root, program, Model::stt(id)?)))
    }

    pub(crate) fn synthesizer(
        root: PathBuf,
        program: PathBuf,
        id: &str,
    ) -> Result<Arc<Self>, Error> {
        Ok(Arc::new(Self::new(root, program, Model::tts(id)?)))
    }

    fn new(root: PathBuf, program: PathBuf, model: Model) -> Self {
        Self {
            model,
            preparation: Preparation::new(root, model),
            program,
            worker: Mutex::new(None),
        }
    }

    async fn execute(
        &self,
        bytes: Vec<u8>,
        cancel: CancellationToken,
        limit: usize,
    ) -> Result<Vec<u8>, Error> {
        self.preparation.readiness()?;
        let directory = tempfile::tempdir().map_err(|_| Error::Provider)?;
        let input = directory.path().join("input");
        let output = directory.path().join("output");
        tokio::fs::write(&input, bytes)
            .await
            .map_err(|_| Error::Provider)?;
        let mut worker = tokio::select! { biased;
            () = cancel.cancelled() => return Err(Error::Cancelled),
            worker = self.worker.lock() => worker,
        };
        worker::execute(
            &mut worker,
            &self.program,
            &Init {
                model: self.model,
                directory: self.preparation.directory(),
            },
            (&input, &output),
            cancel,
        )
        .await?;
        read_bounded(&output, limit).await
    }
}

#[async_trait::async_trait]
impl Transcriber for Offline {
    fn readiness(&self) -> Result<(), Error> {
        self.preparation.readiness()
    }

    async fn transcribe(
        &self,
        audio: Audio,
        cancel: CancellationToken,
    ) -> Result<Transcript, Error> {
        let wav = tokio::task::spawn_blocking(move || audio.wav())
            .await
            .map_err(|_| Error::Provider)??;
        let bytes = self.execute(wav, cancel, MAX_TEXT_BYTES * 2).await?;
        serde_json::from_slice(&bytes).map_err(|_| Error::Provider)
    }
}

#[async_trait::async_trait]
impl Synthesizer for Offline {
    fn readiness(&self) -> Result<(), Error> {
        self.preparation.readiness()
    }

    async fn synthesize(&self, text: &str, cancel: CancellationToken) -> Result<Audio, Error> {
        if text.trim().is_empty() || text.len() > MAX_TEXT_BYTES || text.contains('\0') {
            return Err(Error::Invalid);
        }
        let bytes = self
            .execute(text.as_bytes().to_vec(), cancel, MAX_AUDIO_BYTES)
            .await?;
        Audio {
            bytes,
            format: Format::Wav,
        }
        .pcm()
    }
}

#[cfg(test)]
mod tests;
