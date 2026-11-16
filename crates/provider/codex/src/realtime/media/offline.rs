use super::{AgentSessionError, Endpoint, Event, Future, Media, Pin, oneshot, watch};

/// Offline media boundary for native app-server lifecycle tests.
pub(crate) fn fixture(
    endpoint: Endpoint,
) -> Pin<Box<dyn Future<Output = Result<Media, AgentSessionError>> + Send>> {
    Box::pin(async move {
        let (answer, answer_rx) = oneshot::channel();
        let media = Media {
            offer: "fixture-offer".to_owned(),
            answer: Some(answer),
            cancel: endpoint.cancel.clone(),
            interruptions: watch::channel(()).0,
        };
        tokio::spawn(async move {
            let Endpoint {
                ready,
                mut input,
                output,
                cancel,
            } = endpoint;
            tokio::select! {
                () = cancel.cancelled() => {}
                result = async {
                    let Ok(answer) = answer_rx.await else { return; };
                    assert_eq!(answer, "fixture-answer");
                    let _ = ready.send(());
                    while let Some(audio) = input.recv().await {
                        let bytes = super::super::Resampler::default().convert(&audio)
                            .expect("fixture audio converts to native PCM");
                        for event in [Event::Interrupted, Event::Transcript("hello".to_owned()),
                            Event::Audio(model::realtime::Audio { bytes, sample_rate: 24_000 })] {
                            if output.send(event).await.is_err() { return; }
                        }
                    }
                } => result,
            }
        });
        Ok(media)
    })
}
