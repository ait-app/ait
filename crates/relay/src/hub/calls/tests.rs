use super::*;

fn call_request(sender: &Sender) -> (Request, oneshot::Receiver<Result<Value, CallError>>) {
    let (reply, receiver) = oneshot::channel();
    (
        Request {
            id: Uuid::new_v4(),
            method: "hub.events.publish",
            params: Bytes::from_static(b"{}"),
            reply,
            _slot: sender.slots.clone().try_acquire_owned().unwrap(),
            _bytes: sender.bytes.clone().try_acquire_many_owned(2).unwrap(),
        },
        receiver,
    )
}

#[tokio::test]
async fn acknowledgement_does_not_complete_a_call_and_reply_does() {
    let (sender, mut calls) = Calls::channel();
    let mut link = Link::default();
    let (request, mut reply) = call_request(&sender);
    let id = request.id;
    calls.output(request, &mut link).unwrap().unwrap();
    link.acked(1).unwrap();
    assert!(matches!(
        reply.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));
    calls
        .reply(id, br#"{"result":{"cursor":4,"inserted":true}}"#)
        .unwrap();
    assert_eq!(reply.await.unwrap().unwrap()["cursor"], 4);
    assert_eq!(sender.slots.available_permits(), MAX_CALLS);
    assert_eq!(sender.bytes.available_permits(), MAX_BYTES);
}

#[tokio::test]
async fn correlated_business_errors_do_not_break_the_link_or_disclose_the_message() {
    let (sender, mut calls) = Calls::channel();
    let mut link = Link::default();
    let (request, reply) = call_request(&sender);
    let id = request.id;
    calls.output(request, &mut link).unwrap();
    calls
        .reply(
            id,
            br#"{"error":{"code":"push_capacity","message":"secret"}}"#,
        )
        .unwrap();
    assert_eq!(
        reply.await.unwrap(),
        Err(CallError::Rejected("push_capacity".to_owned()))
    );
    assert!(calls.reply(Uuid::new_v4(), br#"{"result":{}}"#).is_ok());
    assert!(
        calls
            .reply(Uuid::new_v4(), br#"{"unexpected":{}}"#)
            .is_err()
    );
}

#[tokio::test]
async fn fresh_links_and_cancelled_callers_release_admission() {
    let (sender, mut calls) = Calls::channel();
    let mut link = Link::default();
    let (request, reply) = call_request(&sender);
    calls.output(request, &mut link).unwrap();
    calls.fresh();
    assert!(reply.await.is_err());
    let (request, reply) = call_request(&sender);
    drop(reply);
    assert!(calls.output(request, &mut link).unwrap().is_none());
    let (request, reply) = call_request(&sender);
    calls.output(request, &mut link).unwrap();
    drop(reply);
    calls.prune();
    assert!(calls.pending.is_empty());
    assert_eq!(sender.slots.available_permits(), MAX_CALLS);
}
