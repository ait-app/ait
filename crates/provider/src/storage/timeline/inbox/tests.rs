use super::*;
use base64::Engine;

#[test]
fn claimed_foreground_input_keeps_completion_pending_until_admission_or_recovery() {
    let timeline = Timeline::memory().unwrap();
    let prompt = AgentPrompt::text("continue work");
    assert!(!timeline.has_pending_admission("agent").unwrap());
    timeline
        .reserve_input("agent", "first", &prompt, "interrupt")
        .unwrap();
    assert!(timeline.has_pending_admission("agent").unwrap());
    timeline.claim_input("agent", "first").unwrap();
    assert!(!timeline.has_queued_input("agent").unwrap());
    assert!(timeline.has_pending_admission("agent").unwrap());
    timeline
        .finish_input("agent", "first", Receipt::Accepted)
        .unwrap();
    assert!(!timeline.has_pending_admission("agent").unwrap());
    timeline
        .reserve_input("agent", "uncertain", &prompt, "interrupt")
        .unwrap();
    timeline.claim_input("agent", "uncertain").unwrap();
    timeline.recover_inputs().unwrap();
    assert!(!timeline.has_pending_admission("agent").unwrap());
    timeline.claim_answer("agent", "answer", &prompt).unwrap();
    assert!(!timeline.has_pending_admission("agent").unwrap());
}

#[test]
fn screenshot_input_survives_restart_and_retries_without_duplicate_delivery() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("timeline.sqlite3");
    let timeline = Timeline::open(&path).unwrap();
    let prompt = AgentPrompt {
        text: "Inspect this screenshot".to_owned(),
        images: vec![crate::protocol::prompt::PromptImage {
            data: base64::engine::general_purpose::STANDARD.encode(vec![0; 3_886_681]),
            mime_type: "image/png".to_owned(),
        }],
        client_message_id: Some("screenshot".to_owned()),
        ..AgentPrompt::default()
    };
    prompt.validate().unwrap();
    assert!(serde_json::to_vec(&prompt).unwrap().len() > 4 * 1024 * 1024);
    assert_eq!(
        timeline.reserve_input("agent", "screenshot", &prompt, "interrupt"),
        Ok(Receipt::New)
    );
    drop(timeline);

    let timeline = Timeline::open(&path).unwrap();
    timeline.recover_inputs().unwrap();
    assert_eq!(
        timeline.reserve_input("agent", "screenshot", &prompt, "interrupt"),
        Ok(Receipt::Accepted)
    );
    let queued = timeline.queued_inputs().unwrap();
    assert_eq!(queued.len(), 1);
    assert_eq!(queued[0].prompt, prompt);
    timeline.claim_input("agent", "screenshot").unwrap();
    timeline
        .finish_input("agent", "screenshot", Receipt::Accepted)
        .unwrap();
    assert!(!timeline.has_queued_input("agent").unwrap());
    assert_eq!(
        timeline.reserve_input("agent", "screenshot", &prompt, "interrupt"),
        Ok(Receipt::Accepted)
    );
}

#[test]
fn maximum_valid_input_fits_and_queue_byte_budgets_are_released_after_delivery() {
    let timeline = Timeline::memory().unwrap();
    let mut prompt = AgentPrompt {
        text: "Inspect".to_owned(),
        images: vec![
            crate::protocol::prompt::PromptImage {
                data: "AAAA".repeat(8 * 1024 * 1024),
                mime_type: "image/png".to_owned(),
            },
            crate::protocol::prompt::PromptImage {
                data: "AAAA".repeat(7 * 1024 * 1024),
                mime_type: "image/png".to_owned(),
            },
        ],
        output_schema: Some(serde_json::json!({"padding":""})),
        ..AgentPrompt::default()
    };
    let encoded_size = serde_json::to_vec(&prompt).unwrap().len();
    prompt.output_schema = Some(serde_json::json!({
        "padding":"x".repeat(MAX_PROMPT_BYTES - encoded_size)
    }));
    assert_eq!(serde_json::to_vec(&prompt).unwrap().len(), MAX_PROMPT_BYTES);
    prompt.validate().unwrap();
    assert_eq!(
        timeline.reserve_input("one", "large", &prompt, "interrupt"),
        Ok(Receipt::New)
    );
    // Reuse the admitted payload to fill the global budget without repeatedly
    // decoding and serializing a maximum-size rich input during fixture setup.
    {
        let database = timeline.database.lock().unwrap();
        for agent in ["two", "three", "four"] {
            database
                .execute(
                    "INSERT INTO input_receipts(agent,message,fingerprint,prompt,state) \
                     SELECT ?,message,fingerprint,prompt,state FROM input_receipts WHERE agent='one'",
                    [agent],
                )
                .unwrap();
        }
    }
    // Duplicates do not consume capacity even when both byte budgets are full.
    assert_eq!(
        timeline.reserve_input("one", "large", &prompt, "interrupt"),
        Ok(Receipt::Accepted)
    );
    let small = AgentPrompt::text("next");
    for agent in ["one", "five"] {
        assert_eq!(
            timeline.reserve_input(agent, "small", &small, "interrupt"),
            Err(ErrorCode::ResourceExhausted)
        );
        assert_eq!(
            timeline.input_receipt(agent, "small", &small, "interrupt"),
            Ok(None)
        );
    }
    timeline.claim_input("one", "large").unwrap();
    timeline
        .finish_input("one", "large", Receipt::Accepted)
        .unwrap();
    for agent in ["one", "five"] {
        assert_eq!(
            timeline.reserve_input(agent, "small", &small, "interrupt"),
            Ok(Receipt::New)
        );
    }
}

#[test]
fn input_byte_budget_counts_utf8_bytes_in_existing_prompts() {
    let timeline = Timeline::memory().unwrap();
    let prompt = AgentPrompt::text("\u{754c}");
    let encoded = serde_json::to_string(&prompt).unwrap();
    // The next prompt exceeds the byte limit by one byte. SQLite's text length()
    // would undercount the multibyte body and incorrectly admit it.
    let small_size = encoded.len();
    let mut large = AgentPrompt {
        output_schema: Some(serde_json::json!({"padding":""})),
        ..AgentPrompt::text("\u{754c}")
    };
    let overhead = serde_json::to_vec(&large).unwrap().len();
    large.output_schema = Some(serde_json::json!({
        "padding":"x".repeat(MAX_PROMPT_BYTES - small_size - overhead + 1)
    }));
    timeline
        .reserve_input("agent", "large", &large, "interrupt")
        .unwrap();
    assert_eq!(
        timeline.reserve_input("agent", "small", &prompt, "interrupt"),
        Err(ErrorCode::ResourceExhausted)
    );
    timeline.cancel_inputs("agent").unwrap();
    assert_eq!(
        timeline.reserve_input("agent", "small", &prompt, "interrupt"),
        Ok(Receipt::New)
    );
}

#[test]
fn owner_queries_read_only_its_first_fifo_input_and_scheduler_reads_only_ids() {
    let timeline = Timeline::memory().unwrap();
    for (agent, message) in [("a", "one"), ("b", "other"), ("a", "two")] {
        timeline
            .reserve_input(agent, message, &AgentPrompt::text(message), "interrupt")
            .unwrap();
    }
    assert_eq!(timeline.queued_agents().unwrap(), vec!["a", "b"]);
    assert_eq!(
        timeline.queued_for_agent("a").unwrap().unwrap().message,
        "one"
    );
    timeline.claim_input("a", "one").unwrap();
    assert_eq!(
        timeline.queued_for_agent("a").unwrap().unwrap().message,
        "two"
    );
    // An unrelated corrupt body must not prevent owner b or the scheduler from reading.
    timeline
        .database
        .lock()
        .unwrap()
        .execute(
            "UPDATE input_receipts SET prompt='invalid-json' WHERE agent='a' AND message='two'",
            [],
        )
        .unwrap();
    assert_eq!(
        timeline.queued_for_agent("b").unwrap().unwrap().prompt.text,
        "other"
    );
    assert_eq!(timeline.queued_agents().unwrap(), vec!["a", "b"]);
    assert!(timeline.queued_for_agent("a").is_err());
    assert!(timeline.queued_for_agent("missing").unwrap().is_none());
}

#[test]
fn retries_match_payload_and_policy_and_claimed_inputs_never_replay_after_restart() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("timeline.sqlite3");
    let timeline = Timeline::open(&path).unwrap();
    let prompt = AgentPrompt::text("hello");
    assert_eq!(
        timeline.reserve_input("agent", "one", &prompt, "interrupt"),
        Ok(Receipt::New)
    );
    assert_eq!(
        timeline.reserve_input("agent", "one", &prompt, "interrupt"),
        Ok(Receipt::Accepted)
    );
    assert_eq!(
        timeline.reserve_input("agent", "one", &prompt, "steer"),
        Err(ErrorCode::IdempotencyConflict)
    );
    assert_eq!(
        timeline.reserve_input("agent", "one", &AgentPrompt::text("different"), "interrupt"),
        Err(ErrorCode::IdempotencyConflict)
    );
    timeline.claim_input("agent", "one").unwrap();
    assert_eq!(
        timeline.reserve_input("agent", "one", &prompt, "interrupt"),
        Ok(Receipt::Uncertain)
    );
    timeline
        .reserve_input("agent", "two", &prompt, "interrupt")
        .unwrap();
    drop(timeline);
    let timeline = Timeline::open(&path).unwrap();
    timeline.recover_inputs().unwrap();
    assert_eq!(
        timeline.reserve_input("agent", "one", &prompt, "interrupt"),
        Ok(Receipt::Uncertain)
    );
    let inputs = timeline.queued_inputs().unwrap();
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].message, "two");
    assert!(timeline.claim_input("agent", "one").is_err());
    timeline.claim_input("agent", "two").unwrap();
    timeline
        .finish_input("agent", "two", Receipt::Accepted)
        .unwrap();
    assert_eq!(
        timeline.reserve_input("agent", "two", &prompt, "interrupt"),
        Ok(Receipt::Accepted)
    );
    assert!(!timeline.has_queued_input("agent").unwrap());
}

#[test]
fn rejection_fallback_is_explicit_and_cancellation_never_replays_inputs() {
    let timeline = Timeline::memory().unwrap();
    let prompt = AgentPrompt::text("hello");
    timeline
        .reserve_input("agent", "one", &prompt, "steer")
        .unwrap();
    assert!(timeline.requeue_input("agent", "one").is_err());
    timeline.claim_input("agent", "one").unwrap();
    timeline.requeue_input("agent", "one").unwrap();
    assert!(timeline.has_queued_input("agent").unwrap());
    timeline.cancel_inputs("agent").unwrap();
    assert!(!timeline.has_queued_input("agent").unwrap());
    assert_eq!(
        timeline.reserve_input("agent", "one", &prompt, "steer"),
        Ok(Receipt::Rejected)
    );
    assert!(
        timeline
            .finish_input("agent", "one", Receipt::Accepted)
            .is_err()
    );
    assert!(
        timeline
            .finish_input("agent", "missing", Receipt::New)
            .is_err()
    );
}

#[test]
fn bounded_fifo_preserves_distinct_agents_and_rejects_excess_before_admission() {
    let timeline = Timeline::memory().unwrap();
    let prompt = AgentPrompt::text("hello");
    for index in 0..32 {
        timeline
            .reserve_input("agent", &index.to_string(), &prompt, "interrupt")
            .unwrap();
    }
    assert_eq!(
        timeline.reserve_input("agent", "overflow", &prompt, "interrupt"),
        Err(ErrorCode::ResourceExhausted)
    );
    timeline
        .reserve_input("other", "0", &prompt, "interrupt")
        .unwrap();
    let inputs = timeline.queued_inputs().unwrap();
    assert_eq!(inputs.len(), 33);
    assert_eq!(inputs[31].message, "31");
    assert_eq!(inputs[32].agent, "other");
    timeline.cancel_inputs("agent").unwrap();
    assert_eq!(timeline.queued_inputs().unwrap().len(), 1);
}
