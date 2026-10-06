use std::time::Duration;

use super::*;

struct Fixture {
    root: tempfile::TempDir,
    voice: Arc<Offline>,
}

impl Fixture {
    fn new(model: Model) -> Self {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join(model.directory());
        std::fs::create_dir(&directory).unwrap();
        for name in model.files() {
            let path = directory.join(name);
            if matches!(*name, "espeak-ng-data" | "dict") {
                std::fs::create_dir(path).unwrap();
            } else {
                std::fs::write(path, "fixture").unwrap();
            }
        }
        // Never write executable files while parallel tests spawn processes: another fork can
        // inherit the writable descriptor and cause ETXTBSY. Mutable state stays per fixture.
        let program = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/offline/tests/worker.py");
        let voice = Arc::new(Offline::new(root.path().to_owned(), program, model));
        Self { root, voice }
    }

    fn behavior(&self, value: &str) {
        std::fs::write(self.voice.preparation.directory().join("behavior"), value).unwrap();
    }

    fn audio() -> Audio {
        Audio {
            bytes: vec![0; 320],
            format: Format::Pcm(16000),
        }
    }

    fn assert_private_files_removed(&self) {
        let directory = self.voice.preparation.directory();
        let input = std::fs::read_to_string(directory.join("last-input")).unwrap();
        assert!(!std::path::Path::new(&input).exists());
        assert!(self.root.path().exists());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_worker_fixtures_keep_retries_and_behavior_isolated() {
    let barrier = Arc::new(tokio::sync::Barrier::new(16));
    let mut tasks = tokio::task::JoinSet::new();
    for index in 0..16 {
        let barrier = barrier.clone();
        tasks.spawn(async move {
            let fixture = Fixture::new(Model::SenseVoice);
            let invalid_ack = index % 2 == 0;
            if invalid_ack {
                fixture.behavior("invalid-ack");
            }
            barrier.wait().await;
            let result = fixture
                .voice
                .transcribe(Fixture::audio(), CancellationToken::new())
                .await;
            if invalid_ack {
                assert_eq!(result.unwrap_err(), Error::Provider);
                assert!(fixture.voice.worker.lock().await.is_none());
            } else {
                assert_eq!(result.unwrap().text, "recognized offline");
            }
            fixture.behavior("ready");
            assert_eq!(
                fixture
                    .voice
                    .transcribe(Fixture::audio(), CancellationToken::new())
                    .await
                    .unwrap()
                    .text,
                "recognized offline"
            );
            fixture.assert_private_files_removed();
            assert_eq!(
                std::fs::read_to_string(fixture.voice.preparation.directory().join("starts"))
                    .unwrap(),
                if invalid_ack { "1\n1\n" } else { "1\n" }
            );
        });
    }
    while let Some(result) = tasks.join_next().await {
        result.unwrap();
    }
}

#[tokio::test]
async fn offline_recognition_reuses_the_worker_and_removes_private_audio() {
    let fixture = Fixture::new(Model::SenseVoice);
    assert_eq!(Transcriber::readiness(fixture.voice.as_ref()), Ok(()));
    for _ in 0..2 {
        let transcript = fixture
            .voice
            .transcribe(Fixture::audio(), CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(transcript.text, "recognized offline");
        assert_eq!(transcript.language.as_deref(), Some("en"));
        fixture.assert_private_files_removed();
    }
    assert_eq!(
        std::fs::read_to_string(fixture.voice.preparation.directory().join("starts")).unwrap(),
        "1\n"
    );
}

#[tokio::test]
async fn offline_synthesis_decodes_worker_wav_and_bounds_its_output() {
    let fixture = Fixture::new(Model::KokoroEnglish);
    assert_eq!(Synthesizer::readiness(fixture.voice.as_ref()), Ok(()));
    let audio = fixture
        .voice
        .synthesize("speak this", CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(audio.format, Format::Pcm(24000));
    assert_eq!(audio.bytes, vec![0; 240]);
    fixture.assert_private_files_removed();
    fixture.behavior("oversized-audio");
    assert_eq!(
        fixture
            .voice
            .synthesize("speak this", CancellationToken::new())
            .await
            .unwrap_err(),
        Error::Capacity
    );
    fixture.behavior("malformed");
    assert_eq!(
        fixture
            .voice
            .synthesize("speak this", CancellationToken::new())
            .await
            .unwrap_err(),
        Error::Invalid
    );
    fixture.assert_private_files_removed();
}

#[tokio::test]
async fn offline_recognition_rejects_broken_and_oversized_worker_payloads() {
    let fixture = Fixture::new(Model::SenseVoice);
    for (behavior, expected) in [
        ("malformed", Error::Provider),
        ("oversized-text", Error::Capacity),
        ("missing", Error::Provider),
    ] {
        fixture.behavior(behavior);
        assert_eq!(
            fixture
                .voice
                .transcribe(Fixture::audio(), CancellationToken::new())
                .await
                .unwrap_err(),
            expected
        );
        fixture.assert_private_files_removed();
    }
}

#[tokio::test]
async fn canceled_requests_do_not_start_or_retain_a_worker() {
    let fixture = Fixture::new(Model::SenseVoice);
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert_eq!(
        fixture
            .voice
            .transcribe(Fixture::audio(), cancelled)
            .await
            .unwrap_err(),
        Error::Cancelled
    );
    assert!(fixture.voice.worker.lock().await.is_none());
    fixture.behavior("block");
    let voice = fixture.voice.clone();
    let cancel = CancellationToken::new();
    let signal = cancel.clone();
    let task = tokio::spawn(async move { voice.transcribe(Fixture::audio(), signal).await });
    tokio::time::timeout(Duration::from_secs(5), async {
        while !fixture
            .voice
            .preparation
            .directory()
            .join("last-input")
            .exists()
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    cancel.cancel();
    assert_eq!(task.await.unwrap().unwrap_err(), Error::Cancelled);
    assert!(fixture.voice.worker.lock().await.is_none());
    fixture.assert_private_files_removed();
    fixture.behavior("ready");
    assert_eq!(
        fixture
            .voice
            .transcribe(Fixture::audio(), CancellationToken::new())
            .await
            .unwrap()
            .text,
        "recognized offline"
    );
}
