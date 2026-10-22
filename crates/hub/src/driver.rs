//! Owns routing and replay state while physical connections come and go.
use std::{collections::HashMap, sync::Arc, time::Duration};

use futures_util::{SinkExt, StreamExt};
use secrecy::{ExposeSecret, SecretString};
use tokio::sync::{Semaphore, mpsc};
use tokio::task::JoinSet;
use tokio::time::{Instant, timeout};
use tokio_tungstenite::tungstenite::{Bytes, Message};
use tokio_util::{sync::CancellationToken, task::AbortOnDropHandle};
use uuid::Uuid;

use crate::{Connector, Error, Local, link, session, transport, wire};
use session::{Event, Output, Packet, Session, Visit};
use wire::{Inbound, Outbound};

pub(super) struct Driver {
    local: Arc<Local>,
    link: link::Link,
    sessions: HashMap<Uuid, Session>,
    tasks: JoinSet<()>,
    events: mpsc::Receiver<Event>,
    output: mpsc::Sender<Event>,
    incoming_budget: Arc<Semaphore>,
    outgoing_budget: Arc<Semaphore>,
    ack_due: bool,
}

impl Driver {
    pub(super) fn new(local: Arc<Local>) -> Self {
        let (output, events) = mpsc::channel(16);
        Self {
            local,
            link: link::Link::default(),
            sessions: HashMap::new(),
            tasks: JoinSet::new(),
            events,
            output,
            incoming_budget: Arc::new(Semaphore::new(link::BUFFER_BYTES)),
            outgoing_budget: Arc::new(Semaphore::new(link::BUFFER_BYTES)),
            ack_due: false,
        }
    }

    pub(super) async fn run(
        &mut self,
        connector: &Connector,
        generation: Uuid,
        url: &str,
        ticket: SecretString,
    ) -> Result<(), &'static str> {
        let mut grace = Duration::from_secs(30);
        let mut deadline = Instant::now() + grace;
        let mut delay = Duration::from_millis(500);
        loop {
            let credential = self.link.token.as_ref().unwrap_or(&ticket).clone();
            let attempt = establish(self.local.clone(), url, &credential, self.link.resume);
            tokio::pin!(attempt);
            let connected = loop {
                tokio::select! {
                    result = tokio::time::timeout_at(deadline, &mut attempt) =>
                        break result.unwrap_or(Err(Error::Transport)),
                    Some(event) = self.events.recv() => self.event(event)?,
                    Some(_) = self.tasks.join_next(), if !self.tasks.is_empty() => {}
                }
            };
            match connected {
                Ok((socket, welcome)) => {
                    grace = Duration::from_secs(
                        self.link.welcome(welcome).map_err(|_| "invalid_resume")?,
                    );
                    connector
                        .connected(generation, true, self.link.resume.map(|r| r.link))
                        .await;
                    let result = self.pump(socket).await;
                    connector
                        .connected(generation, false, self.link.resume.map(|r| r.link))
                        .await;
                    result?;
                    deadline = Instant::now() + grace;
                    delay = Duration::from_millis(500);
                }
                Err(Error::Unauthorized) => return Err("ticket_required"),
                Err(Error::InvalidGrant | Error::Protocol) => return Err("invalid_frame"),
                Err(Error::Transport) => {}
            }
            if Instant::now() >= deadline {
                return Err("resume_expired");
            }
            let jitter = Duration::from_millis(u64::from(Uuid::new_v4().as_bytes()[0]));
            let pause = tokio::time::sleep_until((Instant::now() + delay + jitter).min(deadline));
            tokio::pin!(pause);
            loop {
                tokio::select! {
                    () = &mut pause => break,
                    Some(event) = self.events.recv() => self.event(event)?,
                    Some(_) = self.tasks.join_next(), if !self.tasks.is_empty() => {}
                }
            }
            delay = (delay * 2).min(Duration::from_secs(10));
        }
    }

    async fn pump(&mut self, socket: transport::Socket) -> Result<(), &'static str> {
        let (mut sink, mut incoming) = socket.split();
        let (sending, mut outgoing) = mpsc::channel(2);
        let mut writer = AbortOnDropHandle::new(tokio::spawn(async move {
            while let Some(message) = outgoing.recv().await {
                if !matches!(
                    timeout(Duration::from_secs(5), sink.send(message)).await,
                    Ok(Ok(()))
                ) {
                    break;
                }
            }
        }));
        let mut pong = None;
        let idle = tokio::time::sleep(Duration::from_secs(65));
        tokio::pin!(idle);
        loop {
            let next = self.link.next();
            let has_output = pong.is_some() || self.ack_due || next.is_some();
            tokio::select! {
                _ = &mut writer => return Ok(()),
                () = &mut idle => return Ok(()),
                frame = incoming.next() => {
                    idle.as_mut().reset(Instant::now() + Duration::from_secs(65));
                    match frame {
                        Some(Ok(Message::Binary(bytes))) => self.receive(&bytes)?,
                        Some(Ok(Message::Ping(bytes))) => pong = Some(Message::Pong(bytes)),
                        Some(Ok(Message::Pong(_))) => {}
                        Some(Ok(Message::Text(_) | Message::Frame(_))) => return Err("invalid_frame"),
                        _ => return Ok(()),
                    }
                }
                permit = sending.reserve(), if has_output => {
                    let Ok(permit) = permit else { return Ok(()) };
                    if let Some(pong) = pong.take() {
                        permit.send(pong);
                    } else if self.ack_due {
                        let recv = self.link.resume.ok_or("invalid_resume")?.recv;
                        let bytes = wire::encode(&Outbound::Ack { a: recv }, &[]).map_err(|_| "invalid_frame")?;
                        permit.send(Message::Binary(bytes));
                        self.ack_due = false;
                    } else if let Some((seq, bytes)) = next {
                        permit.send(Message::Binary(bytes));
                        self.link.sent(seq);
                    }
                }
                Some(event) = self.events.recv() => self.event(event)?,
                Some(_) = self.tasks.join_next(), if !self.tasks.is_empty() => {}
            }
        }
    }

    fn receive(&mut self, bytes: &Bytes) -> Result<(), &'static str> {
        let (header, payload) = wire::decode(bytes).map_err(|_| "invalid_frame")?;
        let (seq, id) = match &header {
            Inbound::Ack { a } => return self.link.ack(*a).map_err(|_| "invalid_ack"),
            Inbound::Error { code } => {
                return Err(match code.as_str() {
                    "unauthorized" => "unauthorized",
                    "replaced" => "replaced",
                    "resume_expired" => "resume_expired",
                    _ => "hub_rejected",
                });
            }
            Inbound::Open { seq, s, .. }
            | Inbound::Msg { seq, s, .. }
            | Inbound::Close { seq, s, .. } => (*seq, *s),
            Inbound::Welcome(_) => return Err("unexpected_frame"),
        };
        self.ack_due = true;
        if !self.link.receive(seq).map_err(|_| "invalid_frame")? {
            return Ok(());
        }
        match header {
            Inbound::Open { from, .. } if !from.is_nil() => self.open(id, payload),
            Inbound::Msg { pt, .. } => {
                if let Some(handle) = self.sessions.get(&id) {
                    let message = wire::message(pt, payload).map_err(|_| "invalid_frame")?;
                    if Packet::new(message, &self.incoming_budget)
                        .and_then(|packet| {
                            handle
                                .incoming
                                .try_send(packet)
                                .map_err(|_| Error::Transport)
                        })
                        .is_err()
                    {
                        self.close(id, "overflow")?;
                    }
                }
                Ok(())
            }
            Inbound::Close { .. } => {
                // Dropping the sender drains messages received before the peer's close.
                self.sessions.remove(&id);
                Ok(())
            }
            _ => Err("unexpected_frame"),
        }
    }

    fn open(&mut self, id: Uuid, hello: Bytes) -> Result<(), &'static str> {
        if id.is_nil() || self.sessions.contains_key(&id) {
            return Err("duplicate_session");
        }
        let valid = serde_json::from_slice::<serde_json::Value>(&hello)
            .is_ok_and(|value| value["type"] == "hello");
        let permit = self.local.visits.clone().try_acquire_owned();
        if !valid || self.sessions.len() >= 16 || permit.is_err() {
            return self
                .link
                .close(id, if valid { "capacity" } else { "invalid_hello" })
                .map_err(|_| "overflow");
        }
        let hello = wire::message(wire::PayloadKind::Text, hello).map_err(|_| "invalid_frame")?;
        let (incoming, receiver) = mpsc::channel(2);
        let generation = Uuid::new_v4();
        let cancel = CancellationToken::new();
        let stopping = cancel.clone();
        self.sessions.insert(
            id,
            Session {
                generation,
                incoming,
                cancel,
            },
        );
        let visit = Visit {
            id,
            generation,
            local: self.local.clone(),
            output: self.output.clone(),
            budget: self.outgoing_budget.clone(),
        };
        self.tasks.spawn(async move {
            let _permit = permit;
            tokio::select! {
                () = stopping.cancelled() => {}
                _ = visit.run(hello, receiver) => {}
            }
            let _ = visit.emit(Output::Closed).await;
        });
        Ok(())
    }

    fn close(&mut self, id: Uuid, code: &'static str) -> Result<(), &'static str> {
        if let Some(session) = self.sessions.remove(&id) {
            session.cancel.cancel();
        }
        self.link.close(id, code).map_err(|_| "overflow")
    }

    fn event(&mut self, event: Event) -> Result<(), &'static str> {
        if self
            .sessions
            .get(&event.id)
            .is_none_or(|s| s.generation != event.generation)
        {
            return Ok(());
        }
        let id = event.id;
        let result = match event.output {
            Output::Accept(packet) => {
                self.link
                    .push(id, &packet.message.into_data(), |seq| Outbound::Accept {
                        seq,
                        s: id,
                    })
            }
            Output::Message(packet) => {
                let kind = if packet.message.is_text() {
                    wire::PayloadKind::Text
                } else {
                    wire::PayloadKind::Binary
                };
                self.link
                    .push(id, &packet.message.into_data(), |seq| Outbound::Msg {
                        seq,
                        s: id,
                        pt: kind,
                    })
            }
            Output::Closed => {
                self.sessions.remove(&id);
                // Keep the session's final queued response before its normal close.
                return self
                    .link
                    .push(id, &[], |seq| Outbound::Close {
                        seq,
                        s: id,
                        code: "closed",
                    })
                    .map_err(|_| "overflow");
            }
        };
        if result.is_err() {
            self.close(id, "overflow")?;
        }
        Ok(())
    }
}

async fn establish(
    local: Arc<Local>,
    url: &str,
    ticket: &SecretString,
    resume: Option<wire::Resume>,
) -> Result<(transport::Socket, wire::Welcome), Error> {
    let mut socket = local
        .dialer
        .connect(url, ticket.expose_secret(), wire::MAX_FRAME)
        .await?;
    let hello = wire::encode(&Outbound::Hello { v: 1, resume }, &[])?;
    transport::send(&mut socket, Message::Binary(hello)).await?;
    let welcome = timeout(Duration::from_secs(5), async {
        loop {
            match socket.next().await {
                Some(Ok(Message::Binary(bytes))) => {
                    let (header, _) = wire::decode(&bytes)?;
                    return match header {
                        Inbound::Welcome(welcome) => Ok(welcome),
                        Inbound::Error { .. } => Err(Error::Unauthorized),
                        _ => Err(Error::Protocol),
                    };
                }
                Some(Ok(Message::Ping(bytes))) => {
                    transport::send(&mut socket, Message::Pong(bytes)).await?;
                }
                Some(Ok(Message::Pong(_))) => {}
                _ => return Err(Error::Transport),
            }
        }
    })
    .await
    .map_err(|_| Error::Transport)??;
    Ok((socket, welcome))
}
