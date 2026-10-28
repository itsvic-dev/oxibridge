use std::{collections::VecDeque, sync::Arc, time::Duration};

use futures_util::{SinkExt, StreamExt};
use log::{debug, warn};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::{
    net::TcpStream,
    time::{Instant, Interval, interval_at, sleep},
};
use tokio_tungstenite::{
    Connector, MaybeTlsStream, WebSocketStream, connect_async_tls_with_config,
    tungstenite::{self, Message as WsMessage, protocol::CloseFrame},
};

use crate::{
    Client, Error,
    model::{Bot, Id, Message},
};

const SEEN_EVENTS: usize = 1024;
const MAX_BACKOFF_SECS: u64 = 60;

type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// A group of gateway events that a bot asks for when it connects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    Guilds,
    Members,
    Messages,
    Reactions,
    Typing,
    Presence,
}

/// An event from the gateway, with the bot that caused it.
#[derive(Debug, Clone)]
pub struct Dispatch {
    pub event: Event,
    /// The bot that caused the event, also when it acted as one of its personas.
    pub origin_bot_id: Option<Id>,
}

#[derive(Debug, Clone)]
pub enum Event {
    /// A new session started. Events from before it are lost.
    Ready(Ready),
    /// A session continued after a reconnect, and all missed events came before this.
    Resumed,
    MessageCreate(Box<Message>),
    MessageUpdate(Box<Message>),
    MessageDelete(MessageDelete),
    MessageDeleteBulk(MessageDeleteBulk),
    ReactionAdd(ReactionEvent),
    ReactionRemove(ReactionEvent),
    ReactionRemoveAll(ReactionRemoveAll),
    /// An event that this crate does not know, or whose payload it cannot read.
    Other {
        name: String,
        data: Value,
    },
}

#[derive(Debug, Clone, Deserialize)]
pub struct Ready {
    pub session_id: String,
    /// The bot of the token. [`None`] for a user token.
    pub bot: Option<Bot>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MessageDelete {
    pub id: Id,
    pub channel_id: Id,
    pub guild_id: Option<Id>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MessageDeleteBulk {
    pub ids: Vec<Id>,
    pub channel_id: Id,
    pub guild_id: Option<Id>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReactionEvent {
    pub message_id: Id,
    pub channel_id: Id,
    pub guild_id: Option<Id>,
    pub actor_id: Id,
    pub emoji: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReactionRemoveAll {
    pub message_id: Id,
    pub channel_id: Id,
    pub guild_id: Option<Id>,
}

#[derive(Deserialize)]
struct Frame {
    op: String,
    #[serde(default)]
    d: Value,
    t: Option<String>,
    s: Option<u64>,
    id: Option<String>,
    origin_bot_id: Option<Id>,
}

#[derive(Deserialize)]
struct Hello {
    heartbeat_interval_ms: u64,
}

struct Session {
    id: String,
    seq: Option<u64>,
}

struct Connection {
    socket: Socket,
    heartbeat: Interval,
    acked: bool,
}

enum Step {
    Heartbeat,
    Received(Option<Result<WsMessage, tungstenite::Error>>),
}

/// A connection to the gateway that reconnects and resumes by itself.
///
/// Call [`Self::next`] in a loop to get events. Events replayed after a resume come only once.
pub struct Gateway {
    url: String,
    token: String,
    intents: Vec<Intent>,
    session: Option<Session>,
    connection: Option<Connection>,
    seen: VecDeque<String>,
    failures: u32,
}

impl Gateway {
    /// Prepares a connection to the gateway of `client`'s server. Nothing connects until [`Self::next`].
    ///
    /// `intents` is used for bot tokens only.
    #[must_use]
    pub fn new(client: &Client, intents: Vec<Intent>) -> Self {
        Self {
            url: client.gateway_url(),
            token: client.token().to_owned(),
            intents,
            session: None,
            connection: None,
            seen: VecDeque::with_capacity(SEEN_EVENTS),
            failures: 0,
        }
    }

    /// Waits for the next event. Connects first, and reconnects after network errors.
    ///
    /// # Errors
    /// Returns [`Error::GatewayClosed`] when the server closes for a reason that a reconnect cannot fix,
    /// like `4004` for a token that is not valid.
    pub async fn next(&mut self) -> Result<Dispatch, Error> {
        loop {
            let Some(connection) = &mut self.connection else {
                self.connect().await?;
                continue;
            };
            let step = tokio::select! {
                _ = connection.heartbeat.tick() => Step::Heartbeat,
                message = connection.socket.next() => Step::Received(message),
            };
            match step {
                Step::Heartbeat => self.heartbeat().await,
                Step::Received(Some(Ok(WsMessage::Text(text)))) => {
                    if let Some(dispatch) = self.handle(&text) {
                        return Ok(dispatch);
                    }
                }
                Step::Received(Some(Ok(WsMessage::Close(frame)))) => {
                    self.closed(frame.as_ref())?;
                }
                Step::Received(Some(Ok(_))) => {}
                Step::Received(Some(Err(e))) => {
                    warn!("gateway connection failed: {e}");
                    self.disconnect();
                }
                Step::Received(None) => {
                    warn!("gateway connection ended");
                    self.disconnect();
                }
            }
        }
    }

    async fn heartbeat(&mut self) {
        let seq = self.session.as_ref().and_then(|session| session.seq);
        let Some(connection) = &mut self.connection else {
            return;
        };
        if !connection.acked {
            warn!("gateway missed a heartbeat ack, reconnecting");
            self.disconnect();
            return;
        }
        connection.acked = false;
        let frame = json!({ "op": "heartbeat", "d": { "seq": seq } });
        if let Err(e) = send(&mut connection.socket, &frame).await {
            warn!("gateway heartbeat failed: {e}");
            self.disconnect();
        }
    }

    fn handle(&mut self, text: &str) -> Option<Dispatch> {
        let frame: Frame = match serde_json::from_str(text) {
            Ok(frame) => frame,
            Err(e) => {
                warn!("gateway sent a frame that is not valid: {e}");
                return None;
            }
        };
        let dispatch = |event| {
            Some(Dispatch {
                event,
                origin_bot_id: None,
            })
        };
        match frame.op.as_str() {
            "heartbeat_ack" => {
                if let Some(connection) = &mut self.connection {
                    connection.acked = true;
                }
                None
            }
            "ready" => match serde_json::from_value::<Ready>(frame.d) {
                Ok(ready) => {
                    self.session = Some(Session {
                        id: ready.session_id.clone(),
                        seq: None,
                    });
                    self.failures = 0;
                    dispatch(Event::Ready(ready))
                }
                Err(e) => {
                    warn!("gateway sent a ready that is not valid: {e}");
                    None
                }
            },
            "resumed" => {
                self.failures = 0;
                dispatch(Event::Resumed)
            }
            "dispatch" => self.dispatch(frame),
            "reconnect" => {
                debug!("gateway asked for a reconnect");
                self.disconnect();
                None
            }
            "invalid_session" => {
                debug!("gateway session is no longer valid");
                self.session = None;
                self.disconnect();
                None
            }
            "rate_limited" => {
                warn!("gateway dropped a command: {}", frame.d);
                None
            }
            _ => None,
        }
    }

    fn dispatch(&mut self, frame: Frame) -> Option<Dispatch> {
        if let (Some(session), Some(seq)) = (&mut self.session, frame.s) {
            session.seq = Some(seq);
        }
        if let Some(id) = frame.id {
            if self.seen.contains(&id) {
                return None;
            }
            if self.seen.len() >= SEEN_EVENTS {
                self.seen.pop_front();
            }
            self.seen.push_back(id);
        }
        let name = frame.t.unwrap_or_default();
        Some(Dispatch {
            event: parse_event(name, frame.d),
            origin_bot_id: frame.origin_bot_id,
        })
    }

    fn closed(&mut self, frame: Option<&CloseFrame>) -> Result<(), Error> {
        let code = frame.map(|frame| u16::from(frame.code));
        self.disconnect();
        match code {
            Some(code @ (4001 | 4002 | 4004 | 4005 | 4010..=4013)) => {
                Err(Error::GatewayClosed(code))
            }
            Some(1000 | 4003) => {
                self.session = None;
                Ok(())
            }
            code => {
                warn!("gateway closed with {code:?}, resuming");
                Ok(())
            }
        }
    }

    fn disconnect(&mut self) {
        self.connection = None;
        self.failures = self.failures.saturating_add(1);
    }

    async fn connect(&mut self) -> Result<(), Error> {
        loop {
            if self.failures > 0 {
                let exponent = self.failures.saturating_sub(1).min(6);
                let delay = 2_u64.saturating_pow(exponent).min(MAX_BACKOFF_SECS);
                sleep(Duration::from_secs(delay)).await;
            }
            match self.open().await {
                Ok(connection) => {
                    self.connection = Some(connection);
                    return Ok(());
                }
                Err(e @ Error::GatewayClosed(_)) => return Err(e),
                Err(e) => {
                    warn!("gateway connection failed: {e}");
                    self.failures = self.failures.saturating_add(1);
                }
            }
        }
    }

    async fn open(&mut self) -> Result<Connection, Error> {
        let connector = Connector::Rustls(tls_config()?);
        let (mut socket, _) =
            connect_async_tls_with_config(&self.url, None, false, Some(connector)).await?;
        let hello = loop {
            match socket.next().await {
                Some(Ok(WsMessage::Text(text))) => {
                    let frame: Frame = serde_json::from_str(&text)?;
                    if frame.op == "hello" {
                        break serde_json::from_value::<Hello>(frame.d)?;
                    }
                }
                Some(Ok(WsMessage::Close(frame))) => {
                    self.closed(frame.as_ref())?;
                    return Err(tungstenite::Error::ConnectionClosed.into());
                }
                Some(Ok(_)) => {}
                Some(Err(e)) => return Err(e.into()),
                None => return Err(tungstenite::Error::ConnectionClosed.into()),
            }
        };

        let start = match &self.session {
            Some(session) => json!({
                "op": "resume",
                "d": { "token": self.token, "session_id": session.id, "seq": session.seq },
            }),
            None if self.token.starts_with("bot_") => json!({
                "op": "identify",
                "d": { "token": self.token, "intents": self.intents },
            }),
            None => json!({
                "op": "identify",
                "d": { "token": self.token, "client_type": "desktop" },
            }),
        };
        send(&mut socket, &start).await?;

        let period = Duration::from_millis(hello.heartbeat_interval_ms.max(1));
        let first = Instant::now()
            .checked_add(jitter(period))
            .unwrap_or_else(Instant::now);
        Ok(Connection {
            socket,
            heartbeat: interval_at(first, period),
            acked: true,
        })
    }
}

fn parse_event(name: String, data: Value) -> Event {
    fn read<T: serde::de::DeserializeOwned>(data: &Value) -> Option<T> {
        serde_json::from_value(data.clone()).ok()
    }
    let event = match name.as_str() {
        "MESSAGE_CREATE" => read(&data).map(Event::MessageCreate),
        "MESSAGE_UPDATE" => read(&data).map(Event::MessageUpdate),
        "MESSAGE_DELETE" => read(&data).map(Event::MessageDelete),
        "MESSAGE_DELETE_BULK" => read(&data).map(Event::MessageDeleteBulk),
        "REACTION_ADD" => read(&data).map(Event::ReactionAdd),
        "REACTION_REMOVE" => read(&data).map(Event::ReactionRemove),
        "REACTION_REMOVE_ALL" => read(&data).map(Event::ReactionRemoveAll),
        _ => None,
    };
    event.unwrap_or(Event::Other { name, data })
}

async fn send(socket: &mut Socket, frame: &Value) -> Result<(), Error> {
    socket.send(WsMessage::text(frame.to_string())).await?;
    Ok(())
}

// the docs ask for a random wait before the first heartbeat, so that clients do not beat in step
fn jitter(period: Duration) -> Duration {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |time| time.subsec_nanos());
    period.mul_f64(f64::from(nanos) / 1e9)
}

fn tls_config() -> Result<Arc<rustls::ClientConfig>, Error> {
    let roots = rustls::RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    // more than one crypto provider can be in the build, so pick one instead of the process default
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots)
        .with_no_client_auth();
    Ok(Arc::new(config))
}

#[cfg(test)]
mod tests {
    use std::pin::Pin;

    use futures_util::{SinkExt, StreamExt};
    use serde_json::{Value, json};
    use tokio::net::TcpListener;
    use tokio_tungstenite::{
        WebSocketStream, accept_async,
        tungstenite::{
            Message as WsMessage,
            protocol::{CloseFrame, frame::coding::CloseCode},
        },
    };

    use super::{Event, Gateway, Intent};
    use crate::{Client, Error};

    type Server = WebSocketStream<tokio::net::TcpStream>;
    type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;

    async fn send(socket: &mut Server, frame: Value) -> TestResult {
        socket.send(WsMessage::text(frame.to_string())).await?;
        Ok(())
    }

    async fn receive(
        socket: &mut Server,
    ) -> Result<Value, Box<dyn std::error::Error + Send + Sync>> {
        loop {
            if let Some(WsMessage::Text(text)) = socket.next().await.transpose()? {
                return Ok(serde_json::from_str(&text)?);
            }
        }
    }

    async fn close(socket: &mut Server, code: u16) -> TestResult {
        let frame = CloseFrame {
            code: CloseCode::from(code),
            reason: "".into(),
        };
        socket.close(Some(frame)).await?;
        Ok(())
    }

    fn message(id: &str) -> Value {
        json!({
            "id": id, "channel_id": "2", "content": "hi", "pinned": false,
            "author": { "id": "3", "kind": "user", "username": "vic" },
            "mentions": [], "mention_roles": [], "mention_everyone": false, "attachments": []
        })
    }

    fn dispatch(name: &str, seq: u64, id: &str, data: &Value) -> Value {
        json!({ "op": "dispatch", "t": name, "s": seq, "id": id, "d": data, "origin_bot_id": "9" })
    }

    type Handler =
        Box<dyn FnOnce(Server) -> Pin<Box<dyn Future<Output = TestResult> + Send>> + Send>;

    fn handler<F, Fut>(f: F) -> Handler
    where
        F: FnOnce(Server) -> Fut + Send + 'static,
        Fut: Future<Output = TestResult> + Send + 'static,
    {
        Box::new(move |socket| Box::pin(f(socket)))
    }

    // accepts one connection per handler, after sending it a hello
    async fn server(handlers: Vec<Handler>) -> std::io::Result<Gateway> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let url = format!("http://{}", listener.local_addr()?);
        tokio::spawn(async move {
            for handler in handlers {
                let (stream, _) = listener.accept().await?;
                let mut socket = accept_async(stream).await?;
                let hello = json!({ "op": "hello", "d": { "heartbeat_interval_ms": 60_000 } });
                send(&mut socket, hello).await?;
                handler(socket).await?;
            }
            TestResult::Ok(())
        });
        let client = Client::with_base_url("bot_token", &url);
        Ok(Gateway::new(&client, vec![Intent::Messages]))
    }

    #[tokio::test]
    async fn identifies_and_drops_repeated_events() -> TestResult {
        let mut gateway = server(vec![handler(|mut socket: Server| async move {
            let identify = receive(&mut socket).await?;
            assert_eq!(identify.pointer("/d/intents"), Some(&json!(["messages"])));
            send(
                &mut socket,
                json!({ "op": "ready", "d": { "session_id": "s" } }),
            )
            .await?;
            for _ in 0..2 {
                send(
                    &mut socket,
                    dispatch("MESSAGE_CREATE", 1, "e1", &message("1")),
                )
                .await?;
            }
            send(
                &mut socket,
                dispatch("MESSAGE_CREATE", 2, "e2", &message("2")),
            )
            .await?;
            receive(&mut socket).await?;
            Ok(())
        })])
        .await?;

        assert!(matches!(gateway.next().await?.event, Event::Ready(_)));
        let first = gateway.next().await?;
        assert!(matches!(&first.event, Event::MessageCreate(m) if m.id.as_str() == "1"));
        assert_eq!(first.origin_bot_id.map(|id| id.0).as_deref(), Some("9"));
        let second = gateway.next().await?.event;
        assert!(matches!(&second, Event::MessageCreate(m) if m.id.as_str() == "2"));
        Ok(())
    }

    #[tokio::test]
    async fn resumes_after_the_connection_closes() -> TestResult {
        let mut gateway = server(vec![
            handler(|mut socket: Server| async move {
                receive(&mut socket).await?;
                send(
                    &mut socket,
                    json!({ "op": "ready", "d": { "session_id": "s" } }),
                )
                .await?;
                let delete = json!({ "id": "1", "channel_id": "2" });
                send(&mut socket, dispatch("MESSAGE_DELETE", 7, "e1", &delete)).await?;
                close(&mut socket, 4000).await
            }),
            handler(|mut socket: Server| async move {
                let resume = receive(&mut socket).await?;
                assert_eq!(resume.pointer("/op"), Some(&json!("resume")));
                assert_eq!(resume.pointer("/d/seq"), Some(&json!(7)));
                send(&mut socket, json!({ "op": "resumed" })).await?;
                receive(&mut socket).await?;
                Ok(())
            }),
        ])
        .await?;

        assert!(matches!(gateway.next().await?.event, Event::Ready(_)));
        assert!(matches!(
            gateway.next().await?.event,
            Event::MessageDelete(_)
        ));
        assert!(matches!(gateway.next().await?.event, Event::Resumed));
        Ok(())
    }

    #[tokio::test]
    async fn stops_when_the_token_is_not_valid() -> TestResult {
        let mut gateway = server(vec![handler(|mut socket: Server| async move {
            receive(&mut socket).await?;
            close(&mut socket, 4004).await
        })])
        .await?;

        assert!(matches!(
            gateway.next().await,
            Err(Error::GatewayClosed(4004))
        ));
        Ok(())
    }

    #[tokio::test]
    async fn keeps_unknown_events() -> TestResult {
        let mut gateway = server(vec![handler(|mut socket: Server| async move {
            receive(&mut socket).await?;
            send(
                &mut socket,
                dispatch("SOMETHING_NEW", 1, "e1", &json!({ "x": 1 })),
            )
            .await?;
            receive(&mut socket).await?;
            Ok(())
        })])
        .await?;

        let event = gateway.next().await?.event;
        assert!(matches!(event, Event::Other { name, .. } if name == "SOMETHING_NEW"));
        Ok(())
    }
}
