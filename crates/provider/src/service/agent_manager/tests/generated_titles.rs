use domain::agent_runtime::TitleOrigin;
use model::summary::{SummaryFuture, SummaryRequest};
use serde_json::json;

use super::*;
use crate::summary::SummaryGenerator;

#[derive(Debug, Default)]
struct Generator {
    started: tokio::sync::Notify,
    release: tokio::sync::Notify,
}
impl SummaryGenerator for Generator {
    fn generate(&self, request: SummaryRequest) -> SummaryFuture<'_> {
        Box::pin(async move {
            assert_eq!(request.context, "First user prompt");
            self.started.notify_one();
            self.release.notified().await;
            Ok(json!({"title":"Generated session title"}))
        })
    }
    fn shutdown(&self) {}
}

#[tokio::test]
async fn generated_titles_are_nonblocking_and_respect_manual_same_value_edits() {
    for manual in [false, true] {
        let (mut manager, registry, _) = make_manager();
        let generator = Arc::new(Generator::default());
        let timeline = crate::storage::timeline::Timeline::memory().unwrap();
        let mut record = stored_record();
        record.title = Some("First user prompt".into());
        record.title_origin = Some(TitleOrigin::Prompt);
        registry.upsert(&record).unwrap();
        timeline
            .append(
                &record.id,
                "codex",
                &[crate::protocol::timeline::NativeItem {
                    key: "user-1".into(),
                    turn_id: None,
                    timestamp: record.created_at.clone(),
                    item: json!({"type":"user_message","text":"First user prompt"}),
                }],
            )
            .unwrap();
        manager = manager
            .with_timeline(timeline.clone())
            .with_summary_generation(generator.clone());
        tokio::time::timeout(
            std::time::Duration::from_millis(100),
            manager.poll_generated_titles(),
        )
        .await
        .unwrap()
        .unwrap();
        generator.started.notified().await;
        assert_eq!(
            registry.get(&record.id).unwrap().unwrap().title,
            record.title
        );
        if manual {
            registry
                .update(&record.id, &|before| {
                    let mut next = before.clone();
                    next.title_origin = None;
                    next
                })
                .unwrap();
        }
        generator.release.notify_one();
        for _ in 0..20 {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            manager.poll_generated_titles().await.unwrap();
        }
        let after = registry.get(&record.id).unwrap().unwrap();
        assert_eq!(
            after.title.as_deref(),
            Some(if manual {
                "First user prompt"
            } else {
                "Generated session title"
            })
        );
        assert_eq!(timeline.read(&record.id).unwrap().1.len(), 1);
        assert_eq!(registry.list().unwrap().len(), 1);
        manager.close_all().await.unwrap();
    }
}

#[tokio::test]
async fn shutdown_cancels_auxiliary_generation_without_requiring_model_completion() {
    let (mut manager, registry, _) = make_manager();
    let generator = Arc::new(Generator::default());
    let timeline = crate::storage::timeline::Timeline::memory().unwrap();
    let mut record = stored_record();
    record.title = Some("First user prompt".into());
    record.title_origin = Some(TitleOrigin::Prompt);
    registry.upsert(&record).unwrap();
    timeline
        .append(
            &record.id,
            "codex",
            &[crate::protocol::timeline::NativeItem {
                key: "user-1".into(),
                turn_id: None,
                timestamp: record.created_at.clone(),
                item: json!({"type":"user_message","text":"First user prompt"}),
            }],
        )
        .unwrap();
    manager = manager
        .with_timeline(timeline)
        .with_summary_generation(generator.clone());
    manager.poll_generated_titles().await.unwrap();
    generator.started.notified().await;
    tokio::time::timeout(std::time::Duration::from_millis(100), manager.close_all())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        registry.get(&record.id).unwrap().unwrap().title,
        record.title
    );
}
