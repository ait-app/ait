//! Shared fixtures: a local destination and an in-process local API double.

use std::net::SocketAddr;
use std::sync::Arc;

use model::share::Visitor;
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
