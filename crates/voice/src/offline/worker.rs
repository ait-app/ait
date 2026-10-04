use std::{
    io::{BufRead, Read, Write},
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
};
use tokio_util::sync::CancellationToken;

use super::{catalog::Model, engine::Engine};
use crate::{
    Error,
    audio::{Audio, Format, MAX_AUDIO_BYTES, MAX_TEXT_BYTES},
};

const CONTROL_LIMIT: u64 = 32 * 1024;

#[derive(Debug, Serialize, Deserialize)]
pub(super) struct Init {
    pub model: Model,
    pub directory: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
struct Request {
    input: PathBuf,
    output: PathBuf,
}

#[derive(Debug)]
pub(super) struct Worker {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Worker {
    pub(super) fn spawn(program: &Path) -> Result<Self, Error> {
        let mut command = Command::new(program);
        for name in model::process::private_environment() {
            command.env_remove(name);
        }
        let mut child = command
            .arg("--speech-worker")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|_| Error::Unavailable)?;
        let input = child.stdin.take().ok_or(Error::Provider)?;
        let output = BufReader::new(child.stdout.take().ok_or(Error::Provider)?);
        Ok(Self {
            child,
            input,
            output,
        })
    }

    async fn exchange(&mut self, message: &impl Serialize) -> Result<(), Error> {
        let mut line = serde_json::to_vec(message).map_err(|_| Error::Invalid)?;
        if line.len() as u64 >= CONTROL_LIMIT {
            return Err(Error::Invalid);
        }
        line.push(b'\n');
        self.input
            .write_all(&line)
            .await
            .map_err(|_| Error::Provider)?;
        self.input.flush().await.map_err(|_| Error::Provider)?;
        let mut response = Vec::new();
        (&mut self.output)
            .take(CONTROL_LIMIT)
            .read_until(b'\n', &mut response)
            .await
            .map_err(|_| Error::Provider)?;
        if response != b"ok\n" {
            return Err(Error::Provider);
        }
        Ok(())
    }

    pub(super) async fn stop(&mut self) {
        let _ = self.child.start_kill();
        let _ = self.child.wait().await;
    }
}

pub(super) async fn execute(
    worker: &mut Option<Worker>,
    program: &Path,
    init: &Init,
    files: (&Path, &Path),
    cancel: CancellationToken,
) -> Result<(), Error> {
    if cancel.is_cancelled() {
        return Err(Error::Cancelled);
    }
    let work = async {
        if worker.is_none() {
            *worker = Some(Worker::spawn(program)?);
            worker
                .as_mut()
                .ok_or(Error::Provider)?
                .exchange(init)
                .await?;
        }
        worker
            .as_mut()
            .ok_or(Error::Provider)?
            .exchange(&Request {
                input: files.0.to_owned(),
                output: files.1.to_owned(),
            })
            .await
    };
    let result = tokio::select! { biased;
        () = cancel.cancelled() => Err(Error::Cancelled),
        () = tokio::time::sleep(Duration::from_secs(120)) => Err(Error::Timeout),
        result = work => result,
    };
    if result.is_err()
        && let Some(mut process) = worker.take()
    {
        process.stop().await;
    }
    result
}

/// Serve private, bounded speech requests on stdin/stdout in an isolated native inference process.
///
/// The first line selects a model and installed directory; subsequent lines contain private input
/// and output file paths. EOF closes the worker. This entry point is only for the server child mode.
/// # Errors
/// Returns a safe error for malformed requests, missing models, native failures or file I/O.
pub fn run() -> Result<(), Error> {
    let input = std::io::stdin();
    serve(input.lock(), std::io::stdout().lock(), Engine::new, process)
}

fn serve<E>(
    mut input: impl BufRead,
    mut output: impl Write,
    initialize: impl FnOnce(&Init) -> Result<E, Error>,
    mut execute: impl FnMut(&E, Vec<u8>) -> Result<Vec<u8>, Error>,
) -> Result<(), Error> {
    let init: Init = read_message(&mut input)?.ok_or(Error::Invalid)?;
    let engine = initialize(&init)?;
    output.write_all(b"ok\n").map_err(|_| Error::Provider)?;
    output.flush().map_err(|_| Error::Provider)?;
    while let Some(request) = read_message::<Request>(&mut input)? {
        let bytes = read_file(&request.input, MAX_AUDIO_BYTES + 44)?;
        let result = execute(&engine, bytes)?;
        std::fs::write(request.output, result).map_err(|_| Error::Provider)?;
        output.write_all(b"ok\n").map_err(|_| Error::Provider)?;
        output.flush().map_err(|_| Error::Provider)?;
    }
    Ok(())
}

fn process(engine: &Engine, bytes: Vec<u8>) -> Result<Vec<u8>, Error> {
    match engine {
        Engine::Recognizer(recognizer) => {
            recognize_request(bytes, |audio| super::engine::transcribe(recognizer, audio))
        }
        Engine::Synthesizer(tts, speaker) => synthesize_request(&bytes, |text| {
            super::engine::synthesize(tts, *speaker, text)
        }),
    }
}

fn recognize_request(
    bytes: Vec<u8>,
    recognize: impl FnOnce(Audio) -> Result<crate::ports::Transcript, Error>,
) -> Result<Vec<u8>, Error> {
    let transcript = recognize(Audio {
        bytes,
        format: Format::Wav,
    })?;
    serde_json::to_vec(&transcript).map_err(|_| Error::Provider)
}

fn synthesize_request(
    bytes: &[u8],
    synthesize: impl FnOnce(&str) -> Result<Audio, Error>,
) -> Result<Vec<u8>, Error> {
    if bytes.len() > MAX_TEXT_BYTES {
        return Err(Error::Capacity);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| Error::Invalid)?;
    synthesize(text)?.wav()
}

fn read_message<T: serde::de::DeserializeOwned>(
    input: &mut impl BufRead,
) -> Result<Option<T>, Error> {
    let mut line = Vec::new();
    Read::take(input, CONTROL_LIMIT)
        .read_until(b'\n', &mut line)
        .map_err(|_| Error::Provider)?;
    if line.is_empty() {
        return Ok(None);
    }
    if line.len() as u64 >= CONTROL_LIMIT || !line.ends_with(b"\n") {
        return Err(Error::Invalid);
    }
    serde_json::from_slice(&line)
        .map(Some)
        .map_err(|_| Error::Invalid)
}

fn read_file(path: &Path, limit: usize) -> Result<Vec<u8>, Error> {
    let file = std::fs::File::open(path).map_err(|_| Error::Provider)?;
    let mut bytes = Vec::new();
    Read::take(file, limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Provider)?;
    if bytes.len() > limit {
        return Err(Error::Capacity);
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests;
