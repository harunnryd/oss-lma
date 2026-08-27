use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::sync::{broadcast, mpsc, oneshot};
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::{protocol::ControlMessage, reconnect::ReconnectBuffer, StereoChunk};

type Socket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

#[derive(Debug, thiserror::Error)]
pub enum LinkError {
    #[error("link client is unavailable")]
    Unavailable,
    #[error("sampling rate must be at least 8000 Hz, got {0}")]
    InvalidSamplingRate(usize),
}

pub type Result<T> = std::result::Result<T, LinkError>;

#[derive(Clone)]
pub struct LinkClient {
    commands: mpsc::UnboundedSender<Command>,
    events: broadcast::Sender<crate::LinkEvent>,
}

enum Command {
    Start(Session, oneshot::Sender<Result<()>>),
    Chunk(StereoChunk),
    Pause,
    Resume,
    End,
}

#[derive(Clone, PartialEq, Eq)]
struct Session {
    call_id: String,
    port: u16,
    token: String,
    rate: usize,
    diarization: bool,
}

struct FrameAssembler {
    frame_bytes: usize,
    pcm: Vec<u8>,
}

impl FrameAssembler {
    fn new(rate: usize) -> Self {
        Self {
            frame_bytes: StereoChunk::byte_len(rate).max(4),
            pcm: Vec::new(),
        }
    }

    fn push(&mut self, chunk: StereoChunk) -> Vec<StereoChunk> {
        if chunk.pcm.len() != chunk.frames * 4 || !chunk.pcm.len().is_multiple_of(4) {
            return Vec::new();
        }
        self.pcm.extend(chunk.pcm);
        let mut frames = Vec::new();
        while self.pcm.len() >= self.frame_bytes {
            let pcm = self.pcm.drain(..self.frame_bytes).collect();
            frames.push(StereoChunk {
                pcm,
                frames: self.frame_bytes / 4,
            });
        }
        frames
    }
}

impl LinkClient {
    pub fn new() -> Self {
        let (commands, receiver) = mpsc::unbounded_channel();
        let (events, _) = broadcast::channel(16);
        tokio::spawn(run(receiver, events.clone()));
        Self { commands, events }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<crate::LinkEvent> {
        self.events.subscribe()
    }

    pub async fn start(
        &self,
        call_id: impl Into<String>,
        port: u16,
        token: impl Into<String>,
        rate: usize,
        diarization: bool,
    ) -> Result<()> {
        if rate < 8_000 {
            return Err(LinkError::InvalidSamplingRate(rate));
        }
        let (reply, result) = oneshot::channel();
        self.commands
            .send(Command::Start(
                Session {
                    call_id: call_id.into(),
                    port,
                    token: token.into(),
                    rate,
                    diarization,
                },
                reply,
            ))
            .map_err(|_| LinkError::Unavailable)?;
        result.await.map_err(|_| LinkError::Unavailable)?
    }

    pub fn send_chunk(&self, chunk: StereoChunk) -> Result<()> {
        self.commands
            .send(Command::Chunk(chunk))
            .map_err(|_| LinkError::Unavailable)
    }

    pub fn pause(&self) -> Result<()> {
        self.commands
            .send(Command::Pause)
            .map_err(|_| LinkError::Unavailable)
    }

    pub fn resume(&self) -> Result<()> {
        self.commands
            .send(Command::Resume)
            .map_err(|_| LinkError::Unavailable)
    }

    pub fn end(&self) -> Result<()> {
        self.commands
            .send(Command::End)
            .map_err(|_| LinkError::Unavailable)
    }
}

impl Default for LinkClient {
    fn default() -> Self {
        Self::new()
    }
}

async fn run(
    mut commands: mpsc::UnboundedReceiver<Command>,
    events: broadcast::Sender<crate::LinkEvent>,
) {
    let mut session = None;
    let mut buffer = None;
    let mut assembler = None;
    let mut retry_delay = Duration::from_millis(500);

    loop {
        if session.is_none() {
            let Some(command) = commands.recv().await else {
                return;
            };
            match command {
                Command::Start(next, reply) => {
                    buffer = Some(ReconnectBuffer::new(next.rate));
                    assembler = Some(FrameAssembler::new(next.rate));
                    session = Some(next);
                    retry_delay = Duration::from_millis(500);
                    let _ = reply.send(Ok(()));
                }
                Command::Chunk(_) | Command::Pause | Command::Resume | Command::End => {}
            }
            continue;
        }

        let active = session.clone().expect("active session exists");
        let mut socket = match connect(&active).await {
            Ok(socket) => socket,
            Err(_) => {
                wait_for_retry(
                    &mut commands,
                    retry_delay,
                    &mut session,
                    &mut buffer,
                    &mut assembler,
                    &events,
                )
                .await;
                retry_delay = (retry_delay * 2).min(Duration::from_secs(10));
                continue;
            }
        };
        if send_control(
            &mut socket,
            ControlMessage::start(&active.call_id, active.rate, active.diarization),
        )
        .await
        .is_err()
        {
            wait_for_retry(
                &mut commands,
                retry_delay,
                &mut session,
                &mut buffer,
                &mut assembler,
                &events,
            )
            .await;
            retry_delay = (retry_delay * 2).min(Duration::from_secs(10));
            continue;
        }
        let _ = events.send(crate::LinkEvent::Connected);
        retry_delay = Duration::from_millis(500);

        if flush(
            &mut socket,
            buffer.as_mut().expect("buffer follows session"),
        )
        .await
        .is_err()
        {
            let _ = events.send(crate::LinkEvent::Disconnected);
            wait_for_retry(
                &mut commands,
                retry_delay,
                &mut session,
                &mut buffer,
                &mut assembler,
                &events,
            )
            .await;
            retry_delay = (retry_delay * 2).min(Duration::from_secs(10));
            continue;
        }

        loop {
            tokio::select! {
                command = commands.recv() => match command {
                    Some(Command::Start(next, reply)) if session.as_ref() == Some(&next) => {
                        let _ = reply.send(Ok(()));
                    }
                    Some(Command::Start(next, reply)) => {
                        buffer = Some(ReconnectBuffer::new(next.rate));
                        assembler = Some(FrameAssembler::new(next.rate));
                        session = Some(next);
                        let _ = reply.send(Ok(()));
                        break;
                    }
                    Some(Command::Chunk(chunk)) => {
                        let frames = assembler.as_mut().expect("assembler follows session").push(chunk);
                        if send_frames(&mut socket, frames, &mut buffer, &events).await.is_err() {
                            break;
                        }
                    }
                    Some(Command::Pause) => {
                        if send_control(&mut socket, ControlMessage::pause(&active.call_id)).await.is_err() { break; }
                    }
                    Some(Command::Resume) => {
                        if send_control(&mut socket, ControlMessage::resume(&active.call_id)).await.is_err() { break; }
                    }
                    Some(Command::End) => {
                        let _ = send_control(&mut socket, ControlMessage::end(&active.call_id)).await;
                        session = None;
                        buffer = None;
                        assembler = None;
                        break;
                    }
                    None => return,
                },
                message = socket.next() => match message {
                    Some(Ok(Message::Ping(payload))) => {
                        if socket.send(Message::Pong(payload)).await.is_err() { break; }
                    }
                    Some(Ok(Message::Text(text))) => {
                        if let Some((call_id, code, context)) = crate::protocol::parse_error(&text) {
                            let _ = events.send(crate::LinkEvent::Error { call_id, code, context });
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(_)) | None => break,
                },
            }
        }

        if session.is_some() {
            let _ = events.send(crate::LinkEvent::Disconnected);
            wait_for_retry(
                &mut commands,
                retry_delay,
                &mut session,
                &mut buffer,
                &mut assembler,
                &events,
            )
            .await;
            retry_delay = (retry_delay * 2).min(Duration::from_secs(10));
        }
    }
}

async fn wait_for_retry(
    commands: &mut mpsc::UnboundedReceiver<Command>,
    delay: Duration,
    session: &mut Option<Session>,
    buffer: &mut Option<ReconnectBuffer>,
    assembler: &mut Option<FrameAssembler>,
    events: &broadcast::Sender<crate::LinkEvent>,
) {
    let sleep = tokio::time::sleep(delay);
    tokio::pin!(sleep);
    loop {
        tokio::select! {
            _ = &mut sleep => break,
            command = commands.recv() => match command {
                None => return,
                Some(command) => {
            match command {
                Command::Start(next, reply) => {
                    if session.as_ref() != Some(&next) {
                        *buffer = Some(ReconnectBuffer::new(next.rate));
                        *assembler = Some(FrameAssembler::new(next.rate));
                        *session = Some(next);
                    }
                    let _ = reply.send(Ok(()));
                }
                Command::Chunk(chunk) => {
                    let frames = assembler.as_mut().expect("assembler follows session").push(chunk);
                    for frame in frames { push_buffer(buffer, frame, events); }
                }
                Command::End => { *session = None; *buffer = None; *assembler = None; },
                Command::Pause | Command::Resume => {}
            }
                }
            }
        }
    }
}

fn push_buffer(
    buffer: &mut Option<ReconnectBuffer>,
    chunk: StereoChunk,
    events: &broadcast::Sender<crate::LinkEvent>,
) {
    if let Some(buffer) = buffer {
        let dropped_frames = buffer.dropped_frames();
        buffer.push(chunk);
        if buffer.dropped_frames() > dropped_frames {
            let _ = events.send(crate::LinkEvent::BufferDropped);
        }
    }
}

async fn connect(
    session: &Session,
) -> std::result::Result<Socket, tokio_tungstenite::tungstenite::Error> {
    let endpoint = format!(
        "ws://127.0.0.1:{}/ws?token={}",
        session.port,
        urlencoding::encode(&session.token)
    );
    connect_async(endpoint).await.map(|(socket, _)| socket)
}

async fn send_control(
    socket: &mut Socket,
    message: ControlMessage,
) -> std::result::Result<(), tokio_tungstenite::tungstenite::Error> {
    socket
        .send(Message::Text(message.to_json().to_string().into()))
        .await
}

async fn flush(
    socket: &mut Socket,
    buffer: &mut ReconnectBuffer,
) -> std::result::Result<(), tokio_tungstenite::tungstenite::Error> {
    while let Some(chunk) = buffer.front().cloned() {
        socket.send(Message::Binary(chunk.pcm.into())).await?;
        buffer.pop_front();
    }
    Ok(())
}

async fn send_frames(
    socket: &mut Socket,
    frames: Vec<StereoChunk>,
    buffer: &mut Option<ReconnectBuffer>,
    events: &broadcast::Sender<crate::LinkEvent>,
) -> std::result::Result<(), tokio_tungstenite::tungstenite::Error> {
    let mut frames = frames.into_iter();
    while let Some(chunk) = frames.next() {
        if let Err(error) = socket.send(Message::Binary(chunk.pcm.clone().into())).await {
            push_buffer(buffer, chunk, events);
            for chunk in frames {
                push_buffer(buffer, chunk, events);
            }
            return Err(error);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use futures_util::StreamExt;
    use lma_capture::StereoChunk;
    use tokio::{
        net::TcpListener,
        sync::Mutex,
        time::{sleep, timeout, Duration, Instant},
    };
    use tokio_tungstenite::{accept_async, tungstenite::Message};

    use crate::LinkEvent;

    use super::LinkClient;

    #[test]
    fn reframes_partial_and_oversized_pcm_into_exact_hundred_millisecond_chunks() {
        let mut framer = super::FrameAssembler::new(100);
        let first = framer.push(StereoChunk {
            pcm: vec![1; 24],
            frames: 6,
        });
        let second = framer.push(StereoChunk {
            pcm: vec![2; 56],
            frames: 14,
        });

        assert!(first.is_empty());
        assert_eq!(second.len(), 2);
        assert!(second
            .iter()
            .all(|chunk| chunk.pcm.len() == 40 && chunk.frames == 10));
        assert_eq!(
            second[0].pcm,
            [1; 24].into_iter().chain([2; 16]).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn rejects_sampling_rates_below_the_wire_contract_minimum() {
        let client = LinkClient::new();
        for rate in [0, 7_999] {
            let error = client
                .start(
                    "123e4567-e89b-12d3-a456-426614174000",
                    1,
                    "token",
                    rate,
                    false,
                )
                .await
                .expect_err("invalid rate is rejected before link setup");
            assert!(matches!(error, super::LinkError::InvalidSamplingRate(value) if value == rate));
        }
    }

    #[tokio::test]
    async fn reports_when_the_reconnect_buffer_drops_audio() {
        let client = LinkClient::new();
        let mut events = client.subscribe();
        client
            .start(
                "123e4567-e89b-12d3-a456-426614174000",
                1,
                "token",
                8_000,
                false,
            )
            .await
            .expect("start is queued");
        for _ in 0..4 {
            client
                .send_chunk(StereoChunk {
                    pcm: vec![0; 32_000],
                    frames: 8_000,
                })
                .expect("chunk is queued");
        }
        let event = timeout(Duration::from_secs(3), events.recv())
            .await
            .expect("drop telemetry arrives");
        assert_eq!(event.expect("event delivery"), LinkEvent::BufferDropped);
    }

    #[tokio::test]
    async fn reconnects_with_the_same_call_id_and_only_one_connection_attempt() {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("listener binds");
        let port = listener.local_addr().expect("listener address").port();
        let starts = Arc::new(Mutex::new(Vec::new()));
        let server_starts = Arc::clone(&starts);
        let server = tokio::spawn(async move {
            for attempt in 0..2 {
                let (stream, _) = listener.accept().await.expect("client connects");
                let mut socket = accept_async(stream).await.expect("websocket upgrades");
                let start = socket
                    .next()
                    .await
                    .expect("start frame")
                    .expect("valid frame");
                let Message::Text(start) = start else {
                    panic!("START must be text")
                };
                server_starts
                    .lock()
                    .await
                    .push((Instant::now(), start.to_string()));
                if attempt == 0 {
                    socket
                        .close(None)
                        .await
                        .expect("server closes first connection");
                }
            }
        });
        let client = LinkClient::new();
        let first = client.start(
            "123e4567-e89b-12d3-a456-426614174000",
            port,
            "token",
            48_000,
            true,
        );
        let second = client.start(
            "123e4567-e89b-12d3-a456-426614174000",
            port,
            "token",
            48_000,
            true,
        );
        let (first, second) = tokio::join!(first, second);
        first.expect("first start succeeds");
        second.expect("second start joins in-flight attempt");
        timeout(Duration::from_secs(3), async {
            loop {
                if starts.lock().await.len() == 2 {
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("client reconnects");
        server.await.expect("server task completes");
        let starts = starts.lock().await;
        assert_eq!(starts.len(), 2);
        assert!(starts[1].0.duration_since(starts[0].0) >= Duration::from_millis(450));
        assert_eq!(starts[0].1, starts[1].1);
        assert!(starts[0]
            .1
            .contains("\"CallId\":\"123e4567-e89b-12d3-a456-426614174000\""));
    }
}
