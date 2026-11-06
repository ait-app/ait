use super::*;

const DECLARED: [&str; 5] = ["lease", "hosts", "state", "credit", "download"];

fn declared() -> Vec<String> {
    DECLARED.map(str::to_owned).to_vec()
}

fn binary(seq: u64, s: Uuid, length: usize) -> Message {
    frame(
        &json!({"t":"msg","seq":seq,"s":s,"pt":"binary"}),
        &vec![0; length],
    )
}

#[tokio::test]
async fn declared_features_are_offered_and_the_welcome_enables_the_lease() {
    let (_relay, hub, center, _local) = connector_with(Some(declared())).await;
    let (mut socket, _) = accept(&center).await;
    assert_eq!(
        next(&mut socket).await.0,
        json!({"t":"hello","v":1,"resume":null,"f":["lease","credit","download"]})
    );
    let mut header = welcome(Uuid::new_v4(), false, 0);
    header["f"] = json!(["lease", "credit"]);
    header["win"] = json!(262_144);
    socket.send(frame(&header, b"")).await.expect("welcome");

    let status = next_status(&hub, |status| status.online).await;
    assert_eq!(status.features, ["lease", "credit"]);
    assert!(status.holds_lease());

    // The lease is held while the link resumes, and released with the link.
    drop(socket);
    let resuming = next_status(&hub, |status| !status.online).await;
    assert!(resuming.holds_lease());
    hub.stop().await;
    assert!(!hub.status().await.holds_lease());
}

#[tokio::test]
async fn an_empty_declaration_offers_no_features_and_old_welcomes_enable_none() {
    let (_relay, hub, center, _local) = connector_with(Some(vec!["hosts".to_owned()])).await;
    let (mut socket, _) = accept(&center).await;
    assert_eq!(
        next(&mut socket).await.0,
        json!({"t":"hello","v":1,"resume":null,"f":[]})
    );
    socket
        .send(frame(&welcome(Uuid::new_v4(), false, 0), b""))
        .await
        .expect("welcome");
    let status = next_status(&hub, |status| status.online).await;
    assert!(status.features.is_empty());
    assert!(!status.holds_lease());
}

#[tokio::test]
async fn credit_sessions_honor_both_windows_and_repeat_credit_after_resume() {
    let (_relay, hub, center, mut local) = connector_with(Some(declared())).await;
    let (mut first, _) = accept(&center).await;
    next(&mut first).await;
    let link = Uuid::new_v4();
    let mut header = welcome(link, false, 0);
    header["f"] = json!(["lease", "credit", "download"]);
    first.send(frame(&header, b"")).await.expect("welcome");
    let s = Uuid::new_v4();
    first
        .send(frame(
            &json!({"t":"open","seq":1,"s":s,"from":Uuid::new_v4(),"w":65_536}),
            HELLO.as_bytes(),
        ))
        .await
        .expect("open");
    let mut api = local.recv().await.expect("session connects");
    api.next().await.expect("hello");
    api.reply("server info").await;
    assert_eq!(
        next(&mut first).await.0,
        json!({"t":"accept","seq":1,"s":s,"w":credit::WINDOW})
    );

    // The visitor's 64 KiB window admits the first 40 KiB only.
    for _ in 0..2 {
        api.replies
            .send(Frame::Binary(vec![1; 40 * 1024].into()))
            .await
            .expect("reply");
    }
    assert_eq!(next(&mut first).await.0["seq"], 2);
    first
        .send(frame(&json!({"t":"credit","s":s,"c":40_960}), b""))
        .await
        .expect("credit");
    assert_eq!(next(&mut first).await.0["seq"], 3);

    // Half of this Host's window delivered locally is credited back.
    for seq in 2..10 {
        first
            .send(binary(seq, s, 64 * 1024))
            .await
            .expect("visitor msg");
    }
    for _ in 2..10 {
        api.next().await.expect("delivered");
    }
    assert_eq!(
        next(&mut first).await,
        (json!({"t":"credit","s":s,"c":524_288}), Vec::new())
    );

    first
        .send(frame(&json!({"t":"ack","a":3}), b""))
        .await
        .expect("ack");
    settle(&mut first).await;
    drop(first);
    let (mut second, _) = accept(&center).await;
    assert_eq!(
        next(&mut second).await.0,
        json!({"t":"hello","v":1,"resume":{"link":link,"recv":9},
            "f":["lease","credit","download"]})
    );
    let mut header = welcome(link, true, 3);
    header["f"] = json!(["lease", "credit", "download"]);
    second.send(frame(&header, b"")).await.expect("resume");
    assert_eq!(
        next(&mut second).await,
        (json!({"t":"credit","s":s,"c":524_288}), Vec::new())
    );
    hub.stop().await;
}

#[tokio::test]
async fn sessions_without_a_window_keep_the_message_bound() {
    let (_relay, hub, center, local) = connector_with(Some(declared())).await;
    silent_daemon(local);
    let mut socket = online(&hub, &center, 120).await;
    let s = Uuid::new_v4();
    socket
        .send(open(1, s, HELLO.as_bytes()))
        .await
        .expect("open");
    for seq in 2..2 + 65 {
        socket.send(binary(seq, s, 1)).await.expect("msg");
    }
    assert_eq!(
        next(&mut socket).await.0,
        json!({"t":"close","seq":1,"s":s,"code":"overflow"})
    );
}

#[tokio::test]
async fn download_and_unknown_session_kinds_are_routed_by_k() {
    let (_relay, hub, center, _local) = connector_with(Some(declared())).await;
    let mut socket = online(&hub, &center, 120).await;
    let (download, unknown) = (Uuid::new_v4(), Uuid::new_v4());
    // Nothing serves files at the fixture's local address.
    socket
        .send(frame(
            &json!({"t":"open","seq":1,"s":download,"from":Uuid::new_v4(),"k":"download","w":65_536}),
            br#"{"token":"scoped-token"}"#,
        ))
        .await
        .expect("download");
    socket
        .send(frame(
            &json!({"t":"open","seq":2,"s":unknown,"from":Uuid::new_v4(),"k":"tunnel"}),
            b"",
        ))
        .await
        .expect("unknown");
    let mut closed = [next(&mut socket).await.0, next(&mut socket).await.0];
    closed.sort_by_key(|header| header["seq"].as_u64());
    assert_eq!(closed[0]["s"], json!(unknown));
    assert_eq!(closed[0]["code"], "unsupported");
    assert_eq!(closed[1]["s"], json!(download));
    assert_eq!(closed[1]["code"], "unavailable");
}

#[tokio::test]
async fn flow_and_revoked_errors_end_the_link() {
    for code in ["flow", "revoked"] {
        let (_relay, hub, center, _local) = connector_with(Some(declared())).await;
        let mut socket = online(&hub, &center, 120).await;
        socket
            .send(frame(&json!({"t":"error","code":code}), b""))
            .await
            .expect("error");
        let status = next_status(&hub, |status| !status.online && !status.connecting).await;
        assert_eq!(status.rejected.as_deref(), Some(code));
        assert!(!status.holds_lease());
    }
}
