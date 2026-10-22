//! One local daemon socket per Hub session, retained across remote reconnects.

use std::{sync::Arc, time::Duration};

use futures_util::{SinkExt, StreamExt};
use secrecy::ExposeSecret;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc};
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::{Error, Local, transport};

pub(super) struct Packet {
    pub message: Message,
    _permit: OwnedSemaphorePermit,
}

impl Packet {
    pub(super) fn new(message: Message, budget: &Arc<Semaphore>) -> Result<Self, Error> {
        let length = u32::try_from(message.len().max(1)).map_err(|_| Error::Protocol)?;
        let permit = budget
            .clone()
            .try_acquire_many_owned(length)
            .map_err(|_| Error::Transport)?;
        Ok(Self {
            message,
            _permit: permit,
        })
    }
}

pub(super) enum Output {
    Accept(Packet),
    Message(Packet),
    Closed,
}

pub(super) struct Event {
    pub id: Uuid,
    pub generation: Uuid,
    pub output: Output,
}

pub(super) struct Session {
    pub generation: Uuid,
    pub incoming: mpsc::Sender<Packet>,
    pub cancel: CancellationToken,
}

pub(super) struct Visit {
    pub id: Uuid,
    pub generation: Uuid,
    pub local: Arc<Local>,
    pub output: mpsc::Sender<Event>,
    pub budget: Arc<Semaphore>,
}

impl Visit {
    pub(super) async fn run(
        &self,
        hello: Message,
        mut incoming: mpsc::Receiver<Packet>,
    ) -> Result<(), Error> {
        let mut socket = self
            .local
            .dialer
            .connect(
                &self.local.url,
                self.local.token.expose_secret(),
                transport::MAX_MESSAGE,
            )
            .await?;
        transport::send(&mut socket, hello).await?;
        let info = timeout(Duration::from_secs(10), transport::text(&mut socket))
            .await
            .map_err(|_| Error::Transport)??;
        let value: serde_json::Value = serde_json::from_str(&info).map_err(|_| Error::Protocol)?;
        if value["type"] != "server_info" {
            return Err(Error::Protocol);
        }
        self.emit(Output::Accept(Packet::new(
            Message::Text(info),
            &self.budget,
        )?))
        .await?;
        let (mut sink, mut stream) = socket.split();
        let (pong_tx, mut pong_rx) = mpsc::channel(1);
        let reader = async {
            while let Some(frame) = stream.next().await {
                match frame.map_err(|_| Error::Transport)? {
                    message @ (Message::Text(_) | Message::Binary(_)) => {
                        self.emit(Output::Message(Packet::new(message, &self.budget)?))
                            .await?;
                    }
                    Message::Ping(bytes) => {
                        pong_tx.try_send(bytes).map_err(|_| Error::Transport)?;
                    }
                    Message::Pong(_) => {}
                    Message::Close(_) => return Ok(()),
                    Message::Frame(_) => return Err(Error::Protocol),
                }
            }
            Ok(())
        };
        let writer = async {
            loop {
                let packet = tokio::select! {
                    packet = incoming.recv() => {
                        let Some(packet) = packet else { return Ok(()) };
                        packet
                    }
                    Some(pong) = pong_rx.recv() => {
                        timeout(Duration::from_secs(5), sink.send(Message::Pong(pong)))
                            .await.map_err(|_| Error::Transport)?.map_err(|_| Error::Transport)?;
                        continue;
                    }
                };
                timeout(Duration::from_secs(5), sink.send(packet.message.clone()))
                    .await
                    .map_err(|_| Error::Transport)?
                    .map_err(|_| Error::Transport)?;
            }
        };
        tokio::select! { result = reader => result, result = writer => result }
    }

    pub(super) async fn emit(&self, output: Output) -> Result<(), Error> {
        timeout(
            Duration::from_secs(5),
            self.output.send(Event {
                id: self.id,
                generation: self.generation,
                output,
            }),
        )
        .await
        .map_err(|_| Error::Transport)?
        .map_err(|_| Error::Transport)
    }
}
