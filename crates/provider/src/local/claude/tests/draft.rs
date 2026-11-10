use super::*;

#[tokio::test]
async fn failed_or_empty_advisor_discovery_keeps_features_and_backs_off() {
    for marker in ["fail-init", "empty-models", "timeout-init"] {
        let (root, mut client, spec) = fixture();
        client.deadline = Duration::from_secs(2);
        std::fs::write(root.path().join(marker), "").unwrap();
        let (first, second) =
            tokio::join!(client.draft_features(&spec), client.draft_features(&spec));
        let first = first.unwrap();
        assert!(!first.is_empty());
        assert_eq!(first, second.unwrap());
        let calls = root.path().join("initialize-calls.jsonl");
        assert_eq!(std::fs::read_to_string(&calls).unwrap().lines().count(), 1);
        assert_eq!(client.draft_features(&spec).await.unwrap(), first);
        assert_eq!(std::fs::read_to_string(&calls).unwrap().lines().count(), 1);
        std::fs::remove_file(root.path().join(marker)).unwrap();
        client.draft_probes.lock().await.insert(
            spec.cwd.clone(),
            std::time::Instant::now()
                .checked_sub(Duration::from_secs(31))
                .unwrap(),
        );
        client.deadline = Duration::from_secs(2);
        assert!(!client.draft_features(&spec).await.unwrap().is_empty());
        assert!(!client.catalog().is_empty());
        assert_eq!(std::fs::read_to_string(&calls).unwrap().lines().count(), 2);
    }
}
