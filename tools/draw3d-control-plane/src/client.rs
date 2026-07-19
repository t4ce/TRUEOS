use std::fmt;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use trueos_draw3d::{Command, FrameDecoder, Response, SceneStats, encode_command};

pub const HOST: &str = "192.168.178.94";
pub const PORT: u16 = 4246;
pub const ENDPOINT: &str = "192.168.178.94:4246";

#[derive(Clone, Debug)]
pub enum ConnectionState {
    Disconnected { reason: Option<String> },
    Connecting,
    Connected { round_trip: Duration },
}

impl ConnectionState {
    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected { .. })
    }
}

#[derive(Debug)]
pub enum ClientCommand {
    Connect,
    Disconnect,
    Request {
        label: &'static str,
        command: Command,
    },
    Shutdown,
}

#[derive(Debug)]
pub enum ClientEvent {
    Connection(ConnectionState),
    Applied {
        label: &'static str,
        affected: u32,
        stats: SceneStats,
    },
    Stats(SceneStats),
    Pong(Duration),
    Error {
        label: &'static str,
        message: String,
    },
}

pub struct NetworkHandle {
    commands: Sender<ClientCommand>,
    events: Receiver<ClientEvent>,
}

impl NetworkHandle {
    pub fn spawn() -> Self {
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, event_rx) = mpsc::channel();
        thread::Builder::new()
            .name("draw3d-control-network".to_owned())
            .spawn(move || network_worker(command_rx, event_tx))
            .expect("failed to spawn draw3d network worker");
        Self {
            commands: command_tx,
            events: event_rx,
        }
    }

    pub fn send(&self, command: ClientCommand) {
        let _ = self.commands.send(command);
    }

    pub fn drain_events(&self) -> Vec<ClientEvent> {
        self.events.try_iter().collect()
    }
}

impl Drop for NetworkHandle {
    fn drop(&mut self) {
        let _ = self.commands.send(ClientCommand::Shutdown);
    }
}

#[derive(Debug)]
enum CallError {
    Transport(String),
    Protocol(String),
}

impl CallError {
    fn is_transport(&self) -> bool {
        matches!(self, Self::Transport(_))
    }
}

impl fmt::Display for CallError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(message) | Self::Protocol(message) => formatter.write_str(message),
        }
    }
}

struct ProtocolClient {
    stream: TcpStream,
    decoder: FrameDecoder,
    next_request_id: u32,
}

impl ProtocolClient {
    fn connect() -> Result<Self, CallError> {
        let address = SocketAddr::from(([192, 168, 178, 94], PORT));
        let stream = TcpStream::connect_timeout(&address, Duration::from_secs(3))
            .map_err(|error| CallError::Transport(format!("connect failed: {error}")))?;
        stream.set_nodelay(true).map_err(|error| {
            CallError::Transport(format!("could not enable TCP_NODELAY: {error}"))
        })?;
        stream
            .set_read_timeout(Some(Duration::from_secs(4)))
            .map_err(|error| {
                CallError::Transport(format!("could not set read timeout: {error}"))
            })?;
        stream
            .set_write_timeout(Some(Duration::from_secs(4)))
            .map_err(|error| {
                CallError::Transport(format!("could not set write timeout: {error}"))
            })?;
        Ok(Self {
            stream,
            decoder: FrameDecoder::new(),
            next_request_id: 1,
        })
    }

    fn call(&mut self, command: &Command) -> Result<Response, CallError> {
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
        let opcode = command.opcode();
        let bytes = encode_command(request_id, command)
            .map_err(|error| CallError::Protocol(format!("command encoding failed: {error:?}")))?;
        self.stream
            .write_all(&bytes)
            .map_err(|error| CallError::Transport(format!("send failed: {error}")))?;

        loop {
            if let Some(decoded) = self
                .decoder
                .next_response()
                .map_err(|error| CallError::Protocol(format!("reply framing failed: {error:?}")))?
            {
                if decoded.request_id != request_id {
                    return Err(CallError::Protocol(format!(
                        "reply request ID {} did not match {request_id}",
                        decoded.request_id
                    )));
                }
                if decoded.opcode != opcode {
                    return Err(CallError::Protocol(format!(
                        "reply opcode {:?} did not match {:?}",
                        decoded.opcode, opcode
                    )));
                }
                let response = decoded.response.map_err(|error| {
                    CallError::Protocol(format!("reply decode failed: {error:?}"))
                })?;
                return match response {
                    Response::Error(error) => {
                        Err(CallError::Protocol(format!("server rejected command: {error:?}")))
                    }
                    response => Ok(response),
                };
            }

            let mut buffer = [0_u8; 16 * 1024];
            let received = self
                .stream
                .read(&mut buffer)
                .map_err(|error| CallError::Transport(format!("receive failed: {error}")))?;
            if received == 0 {
                return Err(CallError::Transport("connection closed by draw3d service".to_owned()));
            }
            self.decoder
                .push(&buffer[..received])
                .map_err(|error| CallError::Protocol(format!("reply buffer failed: {error:?}")))?;
        }
    }
}

fn ping_nonce() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

fn send_event(events: &Sender<ClientEvent>, event: ClientEvent) {
    let _ = events.send(event);
}

fn connect(events: &Sender<ClientEvent>) -> Option<ProtocolClient> {
    send_event(events, ClientEvent::Connection(ConnectionState::Connecting));
    let mut client = match ProtocolClient::connect() {
        Ok(client) => client,
        Err(error) => {
            send_event(
                events,
                ClientEvent::Connection(ConnectionState::Disconnected {
                    reason: Some(error.to_string()),
                }),
            );
            return None;
        }
    };

    let nonce = ping_nonce();
    let started = Instant::now();
    match client.call(&Command::Ping { nonce }) {
        Ok(Response::Pong(reply)) if reply == nonce => {
            send_event(
                events,
                ClientEvent::Connection(ConnectionState::Connected {
                    round_trip: started.elapsed(),
                }),
            );
            Some(client)
        }
        Ok(response) => {
            send_event(
                events,
                ClientEvent::Connection(ConnectionState::Disconnected {
                    reason: Some(format!("unexpected handshake reply: {response:?}")),
                }),
            );
            None
        }
        Err(error) => {
            send_event(
                events,
                ClientEvent::Connection(ConnectionState::Disconnected {
                    reason: Some(error.to_string()),
                }),
            );
            None
        }
    }
}

fn network_worker(commands: Receiver<ClientCommand>, events: Sender<ClientEvent>) {
    let mut client: Option<ProtocolClient> = None;
    while let Ok(command) = commands.recv() {
        match command {
            ClientCommand::Connect => client = connect(&events),
            ClientCommand::Disconnect => {
                client = None;
                send_event(
                    &events,
                    ClientEvent::Connection(ConnectionState::Disconnected { reason: None }),
                );
            }
            ClientCommand::Shutdown => break,
            ClientCommand::Request { label, command } => {
                let Some(active) = client.as_mut() else {
                    send_event(
                        &events,
                        ClientEvent::Error {
                            label,
                            message: "not connected".to_owned(),
                        },
                    );
                    continue;
                };
                let started = Instant::now();
                match active.call(&command) {
                    Ok(Response::Applied(outcome)) => send_event(
                        &events,
                        ClientEvent::Applied {
                            label,
                            affected: outcome.affected,
                            stats: outcome.stats,
                        },
                    ),
                    Ok(Response::Stats(stats)) => {
                        send_event(&events, ClientEvent::Stats(stats));
                    }
                    Ok(Response::Pong(_)) => {
                        send_event(&events, ClientEvent::Pong(started.elapsed()));
                    }
                    Ok(response) => send_event(
                        &events,
                        ClientEvent::Error {
                            label,
                            message: format!("unexpected response: {response:?}"),
                        },
                    ),
                    Err(error) => {
                        let transport_error = error.is_transport();
                        send_event(
                            &events,
                            ClientEvent::Error {
                                label,
                                message: error.to_string(),
                            },
                        );
                        if transport_error {
                            client = None;
                            send_event(
                                &events,
                                ClientEvent::Connection(ConnectionState::Disconnected {
                                    reason: Some("transport connection lost".to_owned()),
                                }),
                            );
                        }
                    }
                }
            }
        }
    }
}

pub fn stats_request() -> ClientCommand {
    ClientCommand::Request {
        label: "refresh stats",
        command: Command::GetStats,
    }
}

pub fn ping_request() -> ClientCommand {
    ClientCommand::Request {
        label: "ping",
        command: Command::Ping {
            nonce: ping_nonce(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use trueos_draw3d::Opcode;

    #[test]
    fn endpoint_is_the_draw3d_service() {
        let parsed: SocketAddr = ENDPOINT.parse().unwrap();
        assert_eq!(parsed.ip().to_string(), HOST);
        assert_eq!(parsed.port(), PORT);
    }

    #[test]
    fn command_opcodes_used_by_control_plane_are_stable() {
        assert_eq!(Command::Clear.opcode(), Opcode::Clear);
        assert_eq!(Command::GetStats.opcode(), Opcode::GetStats);
    }
}
