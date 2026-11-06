//! Shared fixtures: a local destination and an in-process local API double.

use std::net::SocketAddr;
use std::sync::Arc;

use model::share::Visitor;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{OnceCell, Semaphore, mpsc};

use crate::Local;
use crate::hub::bridge::{Connection, Frame, LocalApi, Refused};
use crate::transport::Dialer;

/// A local destination at `address` serving sessions with `api`, if any.
pub(crate) fn local(address: SocketAddr, api: Option<Arc<FakeApi>>) -> Arc<Local> {
    let local = Local {
        visits: Arc::new(Semaphore::new(16)),
        dialer: Dialer::default(),
        url: format!("ws://{address}/v1/ws"),
        server_id: "server".to_owned(),
        instance_id: "instance".to_owned(),
        downloads: OnceCell::new(),
        api: std::sync::OnceLock::new(),
    };
    if let Some(api) = api {
        let _ = local.api.set(api);
    }
    Arc::new(local)
}

/// Body length of a file [`local_file`] serves.
pub(crate) const FILE_SIZE: usize = 128 * 1024 + 17;

/// Serve one local download request; returns the request head it received.
pub(crate) async fn local_file(status: u16) -> (SocketAddr, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("address");
    let task = tokio::spawn(async move {
        let (mut tcp, _) = listener.accept().await.expect("accept");
        let mut request = Vec::new();
        while !request.ends_with(b"\r\n\r\n") {
            request.push(tcp.read_u8().await.expect("request"));
        }
        let body = if status == 200 {
            vec![42_u8; FILE_SIZE]
        } else {
            Vec::new()
        };
        let head = format!(
            "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nContent-Type: application/octet-stream\r\nConnection: close\r\n\r\n",
            body.len()
        );
        tcp.write_all(head.as_bytes()).await.expect("head");
        tcp.write_all(&body).await.expect("body");
        String::from_utf8(request).expect("ascii request")
    });
    (address, task)
}

/// The test's end of one in-process connection.
#[derive(Debug)]
pub(crate) struct Accepted {
    /// The visitor a guest connection was admitted as.
    pub(crate) guest: Option<Visitor>,
    /// Messages the session sent to the API.
    pub(crate) received: mpsc::Receiver<Frame>,
    /// Messages for the session, as if from the API.
    pub(crate) replies: mpsc::Sender<Frame>,
}

impl Accepted {
    /// Next message from the session, or `None` once it closed the connection.
    pub(crate) async fn next(&mut self) -> Option<Frame> {
        tokio::time::timeout(std::time::Duration::from_secs(3), self.received.recv())
            .await
            .expect("session message in time")
    }

    /// Send a text message as the API.
    pub(crate) async fn reply(&self, text: &'static str) {
        self.replies
            .send(Frame::Text(text.into()))
            .await
            .expect("session reads replies");
    }
}

/// Hands every admitted connection to the test; refuses all when `refuse` is set.
#[derive(Debug)]
pub(crate) struct FakeApi {
    connections: mpsc::UnboundedSender<Accepted>,
    refuse: bool,
}

impl FakeApi {
    /// An API double and the stream of connections it admits.
    pub(crate) fn new(refuse: bool) -> (Arc<Self>, mpsc::UnboundedReceiver<Accepted>) {
        let (connections, accepted) = mpsc::unbounded_channel();
        (
            Arc::new(Self {
                connections,
                refuse,
            }),
            accepted,
        )
    }
}

#[async_trait::async_trait]
impl LocalApi for FakeApi {
    async fn connect(&self, guest: Option<&Visitor>) -> Result<Connection, Refused> {
        if self.refuse {
            return Err(Refused);
        }
        let (to_api, from_session) = mpsc::channel(64);
        let (to_session, from_api) = mpsc::channel(64);
        self.connections
            .send(Accepted {
                guest: guest.cloned(),
                received: from_session,
                replies: to_session,
            })
            .map_err(|_| Refused)?;
        Ok(Connection {
            sender: to_api,
            receiver: from_api,
        })
    }
}
