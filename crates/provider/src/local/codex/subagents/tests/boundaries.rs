use super::*;

#[test]
fn child_restore_rejects_root_aliases_and_bounds_out_of_order_native_output() {
    let mut live = Live::default();
    let images = ImageStore::default();
    live.observe(
        "item/completed",
        &spawn("root", "child"),
        ("root", "/tmp"),
        &images,
    )
    .unwrap();
    let mut children = live.children();
    children[0].id = "root".into();
    assert!(matches!(
        Live::restore(children, "root"),
        Err(AgentSessionError::Failed)
    ));
    let mut pending = spawn("root", "child");
    pending["item"]["receiverThreadIds"] = json!([]);
    live.observe("item/started", &pending, ("root", "/tmp"), &images)
        .unwrap();
    let unknown = json!({"threadId":"unknown","turnId":"t","item":{"type":"agentMessage","id":"answer","text":"buffered"}});
    for _ in 0..256 {
        assert!(
            live.observe("item/completed", &unknown, ("root", "/tmp"), &images)
                .unwrap()
                .is_empty()
        );
    }
    assert!(matches!(
        live.observe("item/completed", &unknown, ("root", "/tmp"), &images),
        Err(AgentSessionError::Failed)
    ));
}

#[test]
fn child_turns_reject_overlap_and_map_interruption_or_failure_without_ending_the_root() {
    let images = ImageStore::default();
    for (native, expected) in [("interrupted", "canceled"), ("failed", "failed")] {
        let mut live = Live::default();
        live.observe(
            "item/completed",
            &spawn("root", "child"),
            ("root", "/tmp"),
            &images,
        )
        .unwrap();
        live.observe(
            "turn/started",
            &json!({"threadId":"child","turnId":"first"}),
            ("root", "/tmp"),
            &images,
        )
        .unwrap();
        assert!(matches!(
            live.observe(
                "turn/started",
                &json!({"threadId":"child","turnId":"second"}),
                ("root", "/tmp"),
                &images
            ),
            Err(AgentSessionError::Failed)
        ));
        live.observe(
            "turn/completed",
            &json!({"threadId":"child","turn":{"id":"first","status":native}}),
            ("root", "/tmp"),
            &images,
        )
        .unwrap();
        assert_eq!(live.children()[0].descriptor["status"], expected);
        assert!(
            live.observe(
                "item/completed",
                &json!({"threadId":"child"}),
                ("root", "/tmp"),
                &images
            )
            .unwrap()
            .is_empty()
        );
    }
}
