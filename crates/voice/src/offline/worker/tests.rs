use super::*;

#[test]
fn worker_audio_and_text_contracts_roundtrip_and_reject_invalid_inputs() {
    let audio = Audio {
        bytes: vec![0; 320],
        format: Format::Pcm(16000),
    };
    let wav = audio.clone().wav().unwrap();
    let recognized = recognize_request(wav.clone(), |input| {
        assert_eq!(input.format, Format::Wav);
        assert_eq!(input.pcm().unwrap().bytes, audio.bytes);
        Ok(crate::ports::Transcript {
            text: "你好".into(),
            language: Some("zh".into()),
        })
    })
    .unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&recognized).unwrap(),
        serde_json::json!({"text":"你好","language":"zh"})
    );
    assert_eq!(
        recognize_request(wav.clone(), |_| Err(Error::Provider)),
        Err(Error::Provider)
    );
    assert_eq!(
        synthesize_request("你好".as_bytes(), |text| {
            assert_eq!(text, "你好");
            Ok(audio)
        })
        .unwrap(),
        wav
    );
    assert_eq!(
        synthesize_request(&[0xff], |_| unreachable!()),
        Err(Error::Invalid)
    );
    assert_eq!(
        synthesize_request(&vec![b'x'; MAX_TEXT_BYTES + 1], |_| unreachable!()),
        Err(Error::Capacity)
    );
    assert_eq!(
        synthesize_request(b"text", |_| Err(Error::Unavailable)),
        Err(Error::Unavailable)
    );
}

#[test]
fn control_and_file_input_are_bounded() {
    let valid = b"{\"model\":\"SenseVoice\",\"directory\":\"/models\"}\n";
    let init = read_message::<Init>(&mut valid.as_slice())
        .unwrap()
        .unwrap();
    assert_eq!(init.model, Model::SenseVoice);
    assert!(read_message::<Init>(&mut b"".as_slice()).unwrap().is_none());
    assert!(read_message::<Init>(&mut b"{}".as_slice()).is_err());
    assert!(
        read_message::<Init>(&mut vec![b' '; usize::try_from(CONTROL_LIMIT).unwrap()].as_slice())
            .is_err()
    );
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), b"1234").unwrap();
    assert_eq!(read_file(file.path(), 4).unwrap(), b"1234");
    assert!(matches!(read_file(file.path(), 3), Err(Error::Capacity)));
}

#[test]
fn worker_protocol_acknowledges_only_completed_file_operations() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("input");
    let target = root.path().join("output");
    std::fs::write(&source, b"speech input").unwrap();
    let init = Init {
        model: Model::SenseVoice,
        directory: root.path().to_owned(),
    };
    let request = Request {
        input: source,
        output: target.clone(),
    };
    let control = format!(
        "{}\n{}\n",
        serde_json::to_string(&init).unwrap(),
        serde_json::to_string(&request).unwrap()
    );
    let mut output = Vec::new();
    serve(
        control.as_bytes(),
        &mut output,
        |input| {
            assert_eq!(input.model, init.model);
            assert_eq!(input.directory, init.directory);
            Ok("model")
        },
        |engine, bytes| {
            assert_eq!(*engine, "model");
            assert_eq!(bytes, b"speech input");
            Ok(b"recognized".to_vec())
        },
    )
    .unwrap();
    assert_eq!(output, b"ok\nok\n");
    assert_eq!(std::fs::read(&target).unwrap(), b"recognized");

    output.clear();
    assert_eq!(
        serve(
            control.as_bytes(),
            &mut output,
            |_| Ok(()),
            |(), _| Err(Error::Provider)
        ),
        Err(Error::Provider)
    );
    assert_eq!(output, b"ok\n");
    assert_eq!(std::fs::read(&target).unwrap(), b"recognized");
    assert_eq!(
        serve(
            b"".as_slice(),
            Vec::new(),
            |_| Ok(()),
            |(), _| Ok(Vec::new())
        ),
        Err(Error::Invalid)
    );
    assert_eq!(
        serve(
            control.as_bytes(),
            Vec::new(),
            |_| Err::<(), _>(Error::Unavailable),
            |(), _| unreachable!()
        ),
        Err(Error::Unavailable)
    );
}

#[cfg(unix)]
#[tokio::test]
async fn invalid_worker_acknowledgments_drop_the_process_and_allow_a_retry() {
    let root = tempfile::tempdir().unwrap();
    let program = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/offline/tests/worker.py");
    let behavior = root.path().join("behavior");
    std::fs::write(&behavior, "invalid-ack").unwrap();
    let init = Init {
        model: Model::SenseVoice,
        directory: root.path().to_owned(),
    };
    let mut worker = None;
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(
        execute(
            &mut worker,
            &program,
            &init,
            (root.path(), root.path()),
            cancel
        )
        .await,
        Err(Error::Cancelled)
    );
    assert!(worker.is_none());
    assert_eq!(
        execute(
            &mut worker,
            &program,
            &init,
            (root.path(), root.path()),
            CancellationToken::new()
        )
        .await,
        Err(Error::Provider)
    );
    assert!(worker.is_none());
    std::fs::write(&behavior, "acknowledge-only").unwrap();
    execute(
        &mut worker,
        &program,
        &init,
        (root.path(), root.path()),
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let oversized = "x".repeat(usize::try_from(CONTROL_LIMIT).unwrap());
    assert_eq!(
        worker.as_mut().unwrap().exchange(&oversized).await,
        Err(Error::Invalid)
    );
    worker.as_mut().unwrap().stop().await;
}

#[cfg(unix)]
#[tokio::test]
async fn worker_is_reused_and_cancelled_process_is_reaped() {
    let directory = tempfile::tempdir().unwrap();
    let program = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/offline/tests/worker.py");
    let behavior = directory.path().join("behavior");
    std::fs::write(&behavior, "acknowledge-only").unwrap();
    let init = Init {
        model: Model::SenseVoice,
        directory: directory.path().to_owned(),
    };
    let files = (directory.path(), directory.path());
    let mut worker = None;
    execute(
        &mut worker,
        &program,
        &init,
        files,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    let pid = worker.as_ref().unwrap().child.id();
    execute(
        &mut worker,
        &program,
        &init,
        files,
        CancellationToken::new(),
    )
    .await
    .unwrap();
    assert_eq!(worker.as_ref().unwrap().child.id(), pid);
    worker.as_mut().unwrap().stop().await;
    worker = None;
    std::fs::write(&behavior, "block").unwrap();
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        trigger.cancel();
    });
    assert!(matches!(
        execute(&mut worker, &program, &init, files, cancel).await,
        Err(Error::Cancelled)
    ));
    assert!(worker.is_none());
    assert!(Worker::spawn(&directory.path().join("missing")).is_err());
}
