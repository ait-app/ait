use serde_json::json;

use super::*;

/// OMP 18.18.8's `get_session_stats` data after one turn (probe `b8probe/r1`).
fn session_stats() -> Value {
    json!({"sessionFile":"/s.jsonl","sessionId":"s","userMessages":1,"assistantMessages":1,
        "toolCalls":0,"toolResults":0,"totalMessages":2,
        "tokens":{"input":100,"output":7,"reasoning":0,"cacheRead":20,"cacheWrite":0,"total":127},
        "cost":0.00109,"premiumRequests":0,
        "contextUsage":{"tokens":120,"contextWindow":32768,"percent":0.37}})
}

/// A poller whose read `id` of `source` is in flight.
fn reading(poller: &mut Poller, id: &str, source: Source) {
    poller.read = Some(Read {
        id: id.to_owned(),
        source,
        written: None,
        deadline: Instant::now() + Duration::from_secs(60),
    });
}

fn response(id: &str, success: bool, data: &Value) -> Value {
    json!({"type":"response","id":id,"command":"get_session_stats","success":success,"data":data})
}

#[test]
fn session_stats_map_as_paseo_maps_them() {
    assert_eq!(
        stats(&session_stats()),
        Some(AgentUsage {
            input_tokens: Some(100),
            cached_input_tokens: Some(20),
            output_tokens: Some(7),
            total_cost_usd: Some(0.00109),
            context_window_max_tokens: Some(32768),
            context_window_used_tokens: Some(120),
        })
    );
    // A fresh session reports its context before any tokens.
    let fresh = stats(
        &json!({"tokens":{"input":0,"output":0,"cacheRead":0},"cost":0,
        "contextUsage":{"tokens":8586,"contextWindow":32768}}),
    )
    .unwrap();
    assert_eq!(fresh.input_tokens, Some(0));
    assert_eq!(fresh.context_window_used_tokens, Some(8586));
    assert_eq!(
        stats(&json!({"tokens":{"input":0,"output":0,"cacheRead":0},"cost":0})),
        None
    );
}

#[test]
fn the_state_fallback_reports_context_with_message_totals() {
    let totals = AgentUsage {
        input_tokens: Some(10),
        output_tokens: Some(5),
        cached_input_tokens: Some(2),
        total_cost_usd: Some(0.001),
        context_window_used_tokens: Some(17),
        ..AgentUsage::default()
    };
    let usage = state(
        &json!({"contextUsage":{"tokens":120,"contextWindow":32768}}),
        &totals,
    )
    .unwrap();
    assert_eq!(usage.input_tokens, Some(10));
    assert_eq!(usage.total_cost_usd, Some(0.001));
    assert_eq!(usage.context_window_used_tokens, Some(120));
    assert_eq!(usage.context_window_max_tokens, Some(32768));
    // Without contextUsage the model's window and the last message's total are used.
    let usage = state(&json!({"model":{"contextWindow":65536}}), &totals).unwrap();
    assert_eq!(usage.context_window_max_tokens, Some(65536));
    assert_eq!(usage.context_window_used_tokens, Some(17));
    assert_eq!(state(&json!({}), &AgentUsage::default()), None);
}

#[test]
fn only_changed_valid_snapshots_are_published() {
    let mut poller = Poller::default();
    let totals = AgentUsage::default();
    reading(&mut poller, "r1", Source::Stats);
    assert!(poller.owns(&response("r1", true, &session_stats())));
    assert!(!poller.owns(&response("r2", true, &session_stats())));
    assert!(
        poller
            .answer(&response("r1", true, &session_stats()), &totals)
            .is_some()
    );
    assert!(!poller.owns(&response("r1", true, &session_stats())));

    reading(&mut poller, "r2", Source::Stats);
    assert_eq!(
        poller.answer(&response("r2", true, &session_stats()), &totals),
        None
    );
    reading(&mut poller, "r3", Source::Stats);
    let mut negative = session_stats();
    negative["cost"] = json!(-1.0);
    assert_eq!(
        poller.answer(&response("r3", true, &negative), &totals),
        None
    );
}

#[test]
fn a_refused_stats_read_switches_to_the_state_fallback_at_once() {
    let mut poller = Poller::default();
    poller.complete_turn();
    reading(&mut poller, "r1", Source::Stats);

    let refused = json!({"type":"response","id":"r1","command":"get_session_stats",
        "success":false,"error":"Unknown command: get_session_stats"});
    assert_eq!(poller.answer(&refused, &AgentUsage::default()), None);

    assert!(poller.stats_refused);
    assert!(poller.wanted);
    // The turn's outcome still waits for the fallback read.
    assert!(poller.holds_outcome());
    reading(&mut poller, "r2", Source::State);
    let usage = poller.answer(
        &json!({"type":"response","id":"r2","command":"get_state","success":true,
            "data":{"contextUsage":{"tokens":5,"contextWindow":10}}}),
        &AgentUsage::default(),
    );
    assert_eq!(usage.unwrap().context_window_max_tokens, Some(10));
    assert!(!poller.holds_outcome());
}

#[test]
fn an_empty_stats_answer_also_falls_back() {
    let mut poller = Poller::default();
    reading(&mut poller, "r1", Source::Stats);
    assert_eq!(
        poller.answer(&response("r1", true, &json!({})), &AgentUsage::default()),
        None
    );
    assert!(poller.stats_refused);
}

#[test]
fn turns_schedule_reads_and_hold_their_outcome_until_the_final_read() {
    let mut poller = Poller::default();
    poller.start_turn();
    assert!(poller.next.is_some());
    assert!(!poller.holds_outcome());

    poller.complete_turn();
    assert!(poller.next.is_none());
    assert!(poller.wanted);
    assert!(poller.holds_outcome());

    poller.stop();
    assert!(!poller.wanted);
    assert!(!poller.holds_outcome());

    reading(&mut poller, "r1", Source::Stats);
    poller.reset();
    assert!(poller.read.is_none());
    poller.refresh();
    assert!(poller.wanted);
}
