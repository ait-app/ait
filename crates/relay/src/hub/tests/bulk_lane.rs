use tokio::sync::mpsc::error::TryRecvError;

use super::*;
use crate::test_support::{FILE_SIZE, local_file};

const BULK_TICKET: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const DECLARED: [&str; 6] = ["lease", "hosts", "state", "credit", "download", "bulk"];
const MAIN_FEATURES: [&str; 4] = ["lease", "credit", "download", "bulk"];

fn declared(features: &[&str]) -> Vec<String> {
    features.iter().map(|&feature| feature.to_owned()).collect()
}

/// Bulk tickets from the fixture center; each request is reported on `requests`.
#[derive(Debug)]
struct Tickets {
    requests: mpsc::UnboundedSender<()>,
}

#[async_trait::async_trait]
impl bulk::Tickets for Tickets {
    async fn bulk(&self) -> Option<bulk::Ticket> {
        self.requests.send(()).expect("test observes requests");
        Some(bulk::Ticket {
            hub_ticket: SecretString::from(BULK_TICKET),
            hub_features: Some(declared(&DECLARED)),
        })
    }
}

/// An online main link that declared `bulk`, and the bulk ticket requests it makes.
struct Fixture {
    _relay: crate::Connector,
    hub: Connector,
    center: TcpListener,
    main: Center,
    main_link: Uuid,
    requests: mpsc::UnboundedReceiver<()>,
}

/// Bring the main link online for a center declaring `bulk`, serving files at `local`.
async fn fixture(local: std::net::SocketAddr) -> Fixture {
    let (requests_tx, requests) = mpsc::unbounded_channel();
    let tickets: Arc<dyn bulk::Tickets> = Arc::new(Tickets {
        requests: requests_tx,
    });
    let (relay, hub, center, _) = serving(local, Some(declared(&DECLARED)), Some(tickets)).await;
    let (mut main, credential) = accept(&center).await;
    assert_eq!(credential, format!("Bearer {TICKET}"));
    assert_eq!(
        next(&mut main).await.0,
        json!({"t":"hello","v":1,"resume":null,"f":MAIN_FEATURES})
    );
    let main_link = Uuid::new_v4();
    let mut header = welcome(main_link, false, 0);
    header["f"] = json!(MAIN_FEATURES);
    main.send(frame(&header, b"")).await.expect("welcome");
    next_status(&hub, |status| status.online).await;
    Fixture {
        _relay: relay,
        hub,
        center,
        main,
        main_link,
        requests,
    }
}

async fn hint(center: &mut Center, lane: &str) {
    center
        .send(frame(&json!({"t":"lane","lane":lane}), b""))
        .await
        .expect("lane");
}

/// Accept the bulk link's first connection and welcome it as `link`.
async fn bulk_link(center: &TcpListener, link: Uuid) -> Center {
    let (mut bulk, credential) = accept(center).await;
    assert_eq!(credential, format!("Bearer {BULK_TICKET}"));
    assert_eq!(
        next(&mut bulk).await.0,
        json!({"t":"hello","v":1,"resume":null,"f":["credit","download"]})
    );
    let mut header = welcome(link, false, 0);
    header["f"] = json!(["credit", "download"]);
    header["win"] = json!(262_144);
    bulk.send(frame(&header, b"")).await.expect("welcome");
    bulk
}

/// Wait until the connection is closed by the Host.
async fn closed(center: &mut Center) {
    timeout(Duration::from_secs(3), async {
        loop {
            match center.next().await {
                Some(Ok(Message::Close(_)) | Err(_)) | None => return,
                Some(Ok(_)) => {}
            }
        }
    })
    .await
    .expect("closed in time");
}

/// Hint until the Host requests a new bulk ticket; the previous bulk task may
/// still be finishing when its socket closes.
async fn hint_until_requested(fixture: &mut Fixture) {
    timeout(Duration::from_secs(3), async {
        loop {
            hint(&mut fixture.main, "bulk").await;
            settle(&mut fixture.main).await;
            if fixture.requests.try_recv().is_ok() {
                return;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("bulk ticket requested");
}

#[tokio::test]
async fn lane_hints_open_one_download_only_bulk_link() {
    let mut fixture = fixture("127.0.0.1:9".parse().expect("address")).await;
    // Duplicate hints while the bulk link connects and unknown lanes open nothing more.
    hint(&mut fixture.main, "bulk").await;
    hint(&mut fixture.main, "future").await;
    hint(&mut fixture.main, "bulk").await;
    let mut bulk = bulk_link(&fixture.center, Uuid::new_v4()).await;
    hint(&mut fixture.main, "bulk").await;
    settle(&mut fixture.main).await;
    settle(&mut bulk).await;
    assert_eq!(fixture.requests.try_recv(), Ok(()));
    assert_eq!(fixture.requests.try_recv(), Err(TryRecvError::Empty));
    assert!(
        timeout(Duration::from_millis(200), fixture.center.accept())
            .await
            .is_err(),
        "no second bulk link"
    );

    // The bulk link refuses everything but downloads and leaves the main status alone.
    let rpc = Uuid::new_v4();
    bulk.send(open(1, rpc, HELLO.as_bytes()))
        .await
        .expect("open");
    assert_eq!(
        next(&mut bulk).await.0,
        json!({"t":"close","seq":1,"s":rpc,"code":"unsupported"})
    );
    let status = fixture.hub.status().await;
    assert_eq!(status.link, Some(fixture.main_link));
    assert_eq!(status.features, MAIN_FEATURES);
    assert!(status.holds_lease());

    // Stopping the connector closes the bulk link with the main one.
    fixture.hub.stop().await;
    closed(&mut bulk).await;
    closed(&mut fixture.main).await;
}

#[tokio::test]
async fn downloads_are_served_over_the_bulk_link() {
    let (local, request) = local_file(200).await;
    let mut fixture = fixture(local).await;
    hint(&mut fixture.main, "bulk").await;
    let mut bulk = bulk_link(&fixture.center, Uuid::new_v4()).await;

    let s = Uuid::new_v4();
    bulk.send(frame(
        &json!({"t":"open","seq":1,"s":s,"from":Uuid::new_v4(),"k":"download","w":65_536}),
        br#"{"token":"scoped-token"}"#,
    ))
    .await
    .expect("open");
    assert_eq!(
        next(&mut bulk).await.0,
        json!({"t":"accept","seq":1,"s":s,"w":credit::WINDOW})
    );
    let mut received = 0_usize;
    let mut body = 0_usize;
    loop {
        let (header, payload) = next(&mut bulk).await;
        received += payload.len();
        bulk.send(frame(&json!({"t":"credit","s":s,"c":received}), b""))
            .await
            .expect("credit");
        if header["pt"] == "binary" {
            body += payload.len();
            continue;
        }
        let message: Value = serde_json::from_slice(&payload).expect("json");
        if message["type"] == "download.end" {
            break;
        }
        assert_eq!(message["type"], "download.headers");
    }
    assert_eq!(body, FILE_SIZE);
    bulk.send(frame(
        &json!({"t":"msg","seq":2,"s":s,"pt":"text"}),
        br#"{"type":"download.complete"}"#,
    ))
    .await
    .expect("complete");
    assert_eq!(next(&mut bulk).await.0["code"], "done");
    let request = request.await.expect("local");
    assert!(request.starts_with("GET /api/files/download?token=scoped-token HTTP/1.1\r\n"));
}

#[tokio::test]
async fn the_center_closes_an_idle_bulk_link_and_a_later_hint_opens_another() {
    let mut fixture = fixture("127.0.0.1:9".parse().expect("address")).await;
    hint(&mut fixture.main, "bulk").await;
    let mut bulk = bulk_link(&fixture.center, Uuid::new_v4()).await;
    settle(&mut bulk).await;
    fixture.requests.try_recv().expect("first ticket");

    // The Host never closes an unused bulk link itself: a download routed to it
    // meanwhile would be lost. The center decides, under its routing lock.
    bulk.send(frame(&json!({"t":"error","code":"idle"}), b""))
        .await
        .expect("idle");
    closed(&mut bulk).await;
    let status = fixture.hub.status().await;
    assert!(status.online);
    assert_eq!(status.rejected, None);
    assert!(!fixture.hub.yielding().await);

    hint_until_requested(&mut fixture).await;
    let _second = bulk_link(&fixture.center, Uuid::new_v4()).await;
}

#[tokio::test]
async fn a_bulk_link_resumes_and_a_replaced_one_ends_without_yielding() {
    let mut fixture = fixture("127.0.0.1:9".parse().expect("address")).await;
    hint(&mut fixture.main, "bulk").await;
    let link = Uuid::new_v4();
    let first = bulk_link(&fixture.center, link).await;
    drop(first);

    let (mut resumed, credential) = accept(&fixture.center).await;
    assert_eq!(credential, format!("Bearer {RESUME}"));
    assert_eq!(
        next(&mut resumed).await.0,
        json!({"t":"hello","v":1,"resume":{"link":link,"recv":0},"f":["credit","download"]})
    );
    resumed
        .send(frame(&welcome(link, true, 0), b""))
        .await
        .expect("resume");
    resumed
        .send(frame(&json!({"t":"error","code":"replaced"}), b""))
        .await
        .expect("replaced");
    closed(&mut resumed).await;

    let status = fixture.hub.status().await;
    assert!(status.online);
    assert_eq!(status.rejected, None);
    assert!(!fixture.hub.yielding().await);
    fixture.requests.try_recv().expect("first ticket");
    hint_until_requested(&mut fixture).await;
}

#[tokio::test]
async fn centers_without_bulk_get_no_declaration_and_hints_are_ignored() {
    let (requests_tx, mut requests) = mpsc::unbounded_channel();
    let tickets: Arc<dyn bulk::Tickets> = Arc::new(Tickets {
        requests: requests_tx,
    });
    let (_relay, hub, center, _) = serving(
        "127.0.0.1:9".parse().expect("address"),
        Some(declared(&["lease", "credit", "download"])),
        Some(tickets),
    )
    .await;
    let (mut main, _) = accept(&center).await;
    assert_eq!(
        next(&mut main).await.0,
        json!({"t":"hello","v":1,"resume":null,"f":["lease","credit","download"]})
    );
    main.send(frame(&welcome(Uuid::new_v4(), false, 0), b""))
        .await
        .expect("welcome");
    next_status(&hub, |status| status.online).await;
    hint(&mut main, "bulk").await;
    settle(&mut main).await;
    // The ticket source was never used, only dropped.
    assert_eq!(requests.try_recv(), Err(TryRecvError::Disconnected));

    // Without a ticket source the main link never declares `bulk` either.
    let (_relay, _hub, center, _) = connector_with(Some(declared(&DECLARED))).await;
    let (mut main, _) = accept(&center).await;
    assert_eq!(
        next(&mut main).await.0["f"],
        json!(["lease", "credit", "download"])
    );
}
