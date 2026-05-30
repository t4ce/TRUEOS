extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use embassy_executor::task;
use embassy_time::{Duration as EmbassyDuration, Timer};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::r::net::{VNet, ports};
use v::vnet as api;

const MAX_CLIENT_RX_BYTES: usize = 64 * 1024;
const MAX_LINE_BYTES: usize = 16 * 1024;
const TICK_MS: u64 = 25;
const HEARTBEAT_TIMEOUT_TICKS: u64 = 60_000 / TICK_MS;
const STATE_BROADCAST_TICKS: u64 = 100 / TICK_MS;
const HEARTBEAT_LOG_TICKS: u64 = 10_000 / TICK_MS;
const POSITION_LOG_TICKS: u64 = 1_000 / TICK_MS;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GameStatus {
    Lobby,
    Running,
    Paused,
    Finished,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ClientInfo {
    pub id: u64,
    pub name: String,
    pub ping_ms: Option<u32>,
    pub latency_ms: Option<u32>,
    pub game_id: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct GameInfo {
    pub id: u64,
    pub name: String,
    pub game: String,
    pub host_id: u64,
    pub max_players: u16,
    pub status: GameStatus,
    pub players: Vec<ClientInfo>,
}

#[derive(Clone, Debug, Serialize)]
pub struct PlayerState {
    pub player_id: u64,
    pub name: String,
    pub state: Value,
}

#[derive(Debug)]
pub enum ClientMsg {
    Hello {
        name: String,
        ping_ms: Option<u32>,
        latency_ms: Option<u32>,
        game: Option<String>,
        session_id: Option<String>,
    },
    Heartbeat {
        ping_ms: Option<u32>,
        latency_ms: Option<u32>,
    },
    Chat {
        text: String,
    },
    CreateGame {
        name: String,
        game: String,
        max_players: Option<u16>,
        session_id: Option<String>,
    },
    FreeGame {
        game_id: u64,
        session_id: Option<String>,
    },
    JoinGame {
        game_id: u64,
    },
    LeaveGame {
        game_id: Option<u64>,
    },
    StartGame {
        game_id: u64,
        session_id: Option<String>,
    },
    PauseGame {
        game_id: u64,
        session_id: Option<String>,
    },
    ResumeGame {
        game_id: u64,
        session_id: Option<String>,
    },
    FinishGame {
        game_id: u64,
        session_id: Option<String>,
    },
    GameList,
    GameCommand {
        game_id: u64,
        seq: Option<u64>,
        command: Value,
    },
    Position {
        game_id: u64,
        state: Value,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    Welcome {
        client_id: u64,
        protocol: &'static str,
        heartbeat_ms: u64,
    },
    Ack {
        action: &'static str,
        game_id: Option<u64>,
    },
    Error {
        message: String,
    },
    Chat {
        from_id: u64,
        from: String,
        text: String,
    },
    GameList {
        games: Vec<GameInfo>,
    },
    GameCreated {
        game: GameInfo,
    },
    GameUpdated {
        game: GameInfo,
    },
    GameFreed {
        game_id: u64,
    },
    GameStarted {
        game: GameInfo,
    },
    GamePaused {
        game_id: u64,
    },
    GameResumed {
        game_id: u64,
    },
    GameFinished {
        game_id: u64,
    },
    GameCommand {
        game_id: u64,
        from_id: u64,
        seq: Option<u64>,
        command: Value,
    },
    State {
        game_id: u64,
        tick: u64,
        players: Vec<PlayerState>,
    },
}

struct ClientSession {
    id: u64,
    handle: api::NetHandle,
    name: String,
    ping_ms: Option<u32>,
    latency_ms: Option<u32>,
    session_id: Option<String>,
    game_id: Option<u64>,
    rx: Vec<u8>,
    last_seen_tick: u64,
}

impl ClientSession {
    fn info(&self) -> ClientInfo {
        ClientInfo {
            id: self.id,
            name: self.name.clone(),
            ping_ms: self.ping_ms,
            latency_ms: self.latency_ms,
            game_id: self.game_id,
        }
    }
}

struct GameSession {
    id: u64,
    name: String,
    game: String,
    host_id: u64,
    host_session_id: Option<String>,
    max_players: u16,
    status: GameStatus,
    players: Vec<u64>,
    player_state: BTreeMap<u64, Value>,
}

struct TacticsEndpoint {
    vnet: VNet,
    dev_idx: usize,
    listen_handle: Option<api::NetHandle>,
}

struct TacticsServer {
    clients: BTreeMap<u64, ClientSession>,
    handle_to_client: BTreeMap<u32, u64>,
    games: BTreeMap<u64, GameSession>,
    next_client_id: u64,
    next_game_id: u64,
    tick: u64,
}

impl TacticsServer {
    fn new() -> Self {
        Self {
            clients: BTreeMap::new(),
            handle_to_client: BTreeMap::new(),
            games: BTreeMap::new(),
            next_client_id: 1,
            next_game_id: 1,
            tick: 0,
        }
    }

    fn add_client(&mut self, handle: api::NetHandle) -> u64 {
        let id = self.next_client_id;
        self.next_client_id = self.next_client_id.saturating_add(1);
        self.handle_to_client.insert(handle.0, id);
        self.clients.insert(
            id,
            ClientSession {
                id,
                handle,
                name: alloc::format!("player-{}", id),
                ping_ms: None,
                latency_ms: None,
                session_id: None,
                game_id: None,
                rx: Vec::new(),
                last_seen_tick: self.tick,
            },
        );
        id
    }

    fn remove_handle(&mut self, handle: api::NetHandle) -> Vec<(u64, Vec<api::NetHandle>)> {
        let Some(client_id) = self.handle_to_client.remove(&handle.0) else {
            return Vec::new();
        };
        let old_game = self
            .clients
            .remove(&client_id)
            .and_then(|client| client.game_id);
        let mut updates = Vec::new();
        if let Some(game_id) = old_game {
            let should_remove_player = self
                .games
                .get(&game_id)
                .map(|game| game.status != GameStatus::Lobby)
                .unwrap_or(false);
            if should_remove_player {
                let recipients = self.remove_player_from_game(game_id, client_id);
                updates.push((game_id, recipients));
            }
        }
        updates
    }

    fn remove_player_from_game(&mut self, game_id: u64, client_id: u64) -> Vec<api::NetHandle> {
        if let Some(game) = self.games.get_mut(&game_id) {
            game.players.retain(|id| *id != client_id);
            game.player_state.remove(&client_id);
        }

        if self
            .games
            .get(&game_id)
            .map(|game| game.players.is_empty())
            .unwrap_or(false)
        {
            self.games.remove(&game_id);
            return self.all_handles();
        }

        self.game_handles(game_id)
    }

    fn all_handles(&self) -> Vec<api::NetHandle> {
        self.clients.values().map(|client| client.handle).collect()
    }

    fn game_handles(&self, game_id: u64) -> Vec<api::NetHandle> {
        let Some(game) = self.games.get(&game_id) else {
            return Vec::new();
        };
        game.players
            .iter()
            .filter_map(|id| self.clients.get(id).map(|client| client.handle))
            .collect()
    }

    fn game_info(&self, game: &GameSession) -> GameInfo {
        GameInfo {
            id: game.id,
            name: game.name.clone(),
            game: game.game.clone(),
            host_id: game.host_id,
            max_players: game.max_players,
            status: game.status,
            players: game
                .players
                .iter()
                .filter_map(|id| self.clients.get(id).map(ClientSession::info))
                .collect(),
        }
    }

    fn game_list(&self) -> Vec<GameInfo> {
        self.games
            .values()
            .map(|game| self.game_info(game))
            .collect()
    }

    fn host_allowed(
        &self,
        game_id: u64,
        client_id: u64,
        session_id: Option<&str>,
    ) -> Result<bool, ()> {
        let Some(game) = self.games.get(&game_id) else {
            return Err(());
        };
        if game.host_id == client_id {
            return Ok(true);
        }
        Ok(match (game.host_session_id.as_deref(), session_id) {
            (Some(expected), Some(actual)) => expected == actual,
            (None, _) => game.status == GameStatus::Lobby,
            _ => false,
        })
    }

    fn state_for_game(&self, game_id: u64) -> Option<ServerMsg> {
        let game = self.games.get(&game_id)?;
        if game.status != GameStatus::Running {
            return None;
        }
        let players = game
            .players
            .iter()
            .filter_map(|id| {
                let client = self.clients.get(id)?;
                let state = game.player_state.get(id).cloned().unwrap_or(Value::Null);
                Some(PlayerState {
                    player_id: *id,
                    name: client.name.clone(),
                    state,
                })
            })
            .collect();
        Some(ServerMsg::State {
            game_id,
            tick: self.tick,
            players,
        })
    }

    fn client_lines(&mut self, handle: api::NetHandle, data: &[u8]) -> Vec<Vec<u8>> {
        let Some(client_id) = self.handle_to_client.get(&handle.0).copied() else {
            return Vec::new();
        };
        let Some(client) = self.clients.get_mut(&client_id) else {
            return Vec::new();
        };
        client.last_seen_tick = self.tick;
        client.rx.extend_from_slice(data);
        if client.rx.len() > MAX_CLIENT_RX_BYTES {
            client.rx.clear();
            return Vec::new();
        }

        let mut lines = Vec::new();
        while let Some(pos) = client.rx.iter().position(|b| *b == b'\n') {
            let mut line: Vec<u8> = client.rx.drain(..=pos).collect();
            while matches!(line.last(), Some(b'\n' | b'\r')) {
                line.pop();
            }
            if !line.is_empty() && line.len() <= MAX_LINE_BYTES {
                lines.push(line);
            }
        }

        while client.rx.len() <= MAX_LINE_BYTES {
            let Some(frame_len) = json_object_frame_len(client.rx.as_slice()) else {
                break;
            };
            let mut line: Vec<u8> = client.rx.drain(..frame_len).collect();
            trim_ascii_edges(&mut line);
            if !line.is_empty() && line.len() <= MAX_LINE_BYTES {
                lines.push(line);
            }
        }
        lines
    }

    fn take_pending_line_on_close(&mut self, handle: api::NetHandle) -> Option<(u64, Vec<u8>)> {
        let client_id = self.handle_to_client.get(&handle.0).copied()?;
        let client = self.clients.get_mut(&client_id)?;
        if client.rx.is_empty() || client.rx.len() > MAX_LINE_BYTES {
            return None;
        }

        let mut line = core::mem::take(&mut client.rx);
        while matches!(line.last(), Some(b'\n' | b'\r')) {
            line.pop();
        }
        if !looks_like_complete_json_object(line.as_slice())
            && looks_like_unclosed_json_object(line.as_slice())
        {
            crate::log!(
                "tactics-srv: repairing eof json handle={} client={} bytes={}\n",
                handle.0,
                client_id,
                line.len()
            );
            line.push(b'}');
        }
        Some((client_id, line))
    }
}

fn trim_ascii_edges(bytes: &mut Vec<u8>) {
    let leading = bytes
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    if leading != 0 {
        bytes.drain(..leading);
    }
    while matches!(bytes.last(), Some(b) if b.is_ascii_whitespace()) {
        bytes.pop();
    }
}

fn json_object_frame_len(bytes: &[u8]) -> Option<usize> {
    let start = bytes.iter().position(|b| !b.is_ascii_whitespace())?;
    if bytes.get(start).copied() != Some(b'{') {
        return None;
    }

    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, &byte) in bytes[start..].iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }

        match byte {
            b'"' => in_string = true,
            b'{' => depth = depth.saturating_add(1),
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(start + offset + 1);
                }
            }
            _ => {}
        }
    }
    None
}

fn looks_like_complete_json_object(bytes: &[u8]) -> bool {
    let mut trimmed = bytes.to_vec();
    trim_ascii_edges(&mut trimmed);
    json_object_frame_len(trimmed.as_slice()) == Some(trimmed.len())
}

fn looks_like_unclosed_json_object(bytes: &[u8]) -> bool {
    let mut start = 0;
    let mut end = bytes.len();
    while start < end && bytes[start].is_ascii_whitespace() {
        start += 1;
    }
    while end > start && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    start < end && bytes[start] == b'{' && bytes[start..end].windows(6).any(|w| w == b"\"type\"")
}

fn log_json_parse_error(handle: api::NetHandle, client_id: u64, line: &[u8], err: &str) {
    let preview_len = line.len().min(96);
    crate::log!(
        "tactics-srv: invalid json handle={} client={} bytes={} err={} preview={:?}\n",
        handle.0,
        client_id,
        line.len(),
        err,
        &line[..preview_len]
    );
}

fn value_field<'a>(obj: &'a serde_json::Map<String, Value>, key: &str) -> Option<&'a Value> {
    obj.get(key)
}

fn string_field(obj: &serde_json::Map<String, Value>, key: &str) -> Result<String, String> {
    value_field(obj, key)
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .ok_or_else(|| alloc::format!("missing string field `{}`", key))
}

fn opt_string_field(
    obj: &serde_json::Map<String, Value>,
    key: &str,
) -> Result<Option<String>, String> {
    match value_field(obj, key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_str()
            .map(|s| Some(s.to_string()))
            .ok_or_else(|| alloc::format!("field `{}` must be string", key)),
    }
}

fn opt_session_id(obj: &serde_json::Map<String, Value>) -> Result<Option<String>, String> {
    for key in ["session_id", "client_key", "token"] {
        if let Some(value) = value_field(obj, key) {
            return match value {
                Value::Null => Ok(None),
                Value::String(value) if value.len() <= 128 => Ok(Some(value.clone())),
                Value::String(_) => Err(alloc::format!("field `{}` is too long", key)),
                _ => Err(alloc::format!("field `{}` must be string", key)),
            };
        }
    }
    Ok(None)
}

fn u64_field(obj: &serde_json::Map<String, Value>, key: &str) -> Result<u64, String> {
    value_field(obj, key)
        .and_then(Value::as_u64)
        .ok_or_else(|| alloc::format!("missing integer field `{}`", key))
}

fn opt_u32_field(obj: &serde_json::Map<String, Value>, key: &str) -> Result<Option<u32>, String> {
    match value_field(obj, key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .map(Some)
            .ok_or_else(|| alloc::format!("field `{}` must be u32", key)),
    }
}

fn opt_u16_field(obj: &serde_json::Map<String, Value>, key: &str) -> Result<Option<u16>, String> {
    match value_field(obj, key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .and_then(|v| u16::try_from(v).ok())
            .map(Some)
            .ok_or_else(|| alloc::format!("field `{}` must be u16", key)),
    }
}

fn opt_u64_field(obj: &serde_json::Map<String, Value>, key: &str) -> Result<Option<u64>, String> {
    match value_field(obj, key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or_else(|| alloc::format!("field `{}` must be u64", key)),
    }
}

fn json_value_field(obj: &serde_json::Map<String, Value>, key: &str) -> Result<Value, String> {
    value_field(obj, key)
        .cloned()
        .ok_or_else(|| alloc::format!("missing json field `{}`", key))
}

fn parse_client_msg(line: &[u8]) -> Result<ClientMsg, String> {
    let value = serde_json::from_slice::<Value>(line)
        .map_err(|err| alloc::format!("json parse failed: {}", err))?;
    let obj = value
        .as_object()
        .ok_or_else(|| "command must be a json object".to_string())?;
    let ty = value_field(obj, "type")
        .and_then(Value::as_str)
        .ok_or_else(|| "missing string field `type`".to_string())?;

    match ty {
        "hello" => Ok(ClientMsg::Hello {
            name: string_field(obj, "name")?,
            ping_ms: opt_u32_field(obj, "ping_ms")?,
            latency_ms: opt_u32_field(obj, "latency_ms")?,
            game: opt_string_field(obj, "game")?,
            session_id: opt_session_id(obj)?,
        }),
        "heartbeat" => Ok(ClientMsg::Heartbeat {
            ping_ms: opt_u32_field(obj, "ping_ms")?,
            latency_ms: opt_u32_field(obj, "latency_ms")?,
        }),
        "chat" => Ok(ClientMsg::Chat {
            text: string_field(obj, "text")?,
        }),
        "create_game" => Ok(ClientMsg::CreateGame {
            name: string_field(obj, "name")?,
            game: string_field(obj, "game")?,
            max_players: opt_u16_field(obj, "max_players")?,
            session_id: opt_session_id(obj)?,
        }),
        "free_game" => Ok(ClientMsg::FreeGame {
            game_id: u64_field(obj, "game_id")?,
            session_id: opt_session_id(obj)?,
        }),
        "join_game" => Ok(ClientMsg::JoinGame {
            game_id: u64_field(obj, "game_id")?,
        }),
        "leave_game" => Ok(ClientMsg::LeaveGame {
            game_id: opt_u64_field(obj, "game_id")?,
        }),
        "start_game" => Ok(ClientMsg::StartGame {
            game_id: u64_field(obj, "game_id")?,
            session_id: opt_session_id(obj)?,
        }),
        "pause_game" => Ok(ClientMsg::PauseGame {
            game_id: u64_field(obj, "game_id")?,
            session_id: opt_session_id(obj)?,
        }),
        "resume_game" => Ok(ClientMsg::ResumeGame {
            game_id: u64_field(obj, "game_id")?,
            session_id: opt_session_id(obj)?,
        }),
        "finish_game" => Ok(ClientMsg::FinishGame {
            game_id: u64_field(obj, "game_id")?,
            session_id: opt_session_id(obj)?,
        }),
        "game_list" => Ok(ClientMsg::GameList),
        "game_command" => Ok(ClientMsg::GameCommand {
            game_id: u64_field(obj, "game_id")?,
            seq: opt_u64_field(obj, "seq")?,
            command: json_value_field(obj, "command")?,
        }),
        "position" => Ok(ClientMsg::Position {
            game_id: u64_field(obj, "game_id")?,
            state: json_value_field(obj, "state")?,
        }),
        _ => Err(alloc::format!("unknown command type `{}`", ty)),
    }
}

fn client_msg_label(msg: &ClientMsg) -> &'static str {
    match msg {
        ClientMsg::Hello { .. } => "hello",
        ClientMsg::Heartbeat { .. } => "heartbeat",
        ClientMsg::Chat { .. } => "chat",
        ClientMsg::CreateGame { .. } => "create_game",
        ClientMsg::FreeGame { .. } => "free_game",
        ClientMsg::JoinGame { .. } => "join_game",
        ClientMsg::LeaveGame { .. } => "leave_game",
        ClientMsg::StartGame { .. } => "start_game",
        ClientMsg::PauseGame { .. } => "pause_game",
        ClientMsg::ResumeGame { .. } => "resume_game",
        ClientMsg::FinishGame { .. } => "finish_game",
        ClientMsg::GameList => "game_list",
        ClientMsg::GameCommand { .. } => "game_command",
        ClientMsg::Position { .. } => "position",
    }
}

fn send_msg<T: Serialize>(vnet: &VNet, handle: api::NetHandle, msg: &T) {
    if let Ok(mut data) = serde_json::to_vec(msg) {
        data.push(b'\n');
        if vnet.send_tcp_all(handle, data.as_slice()).is_err() {
            crate::log!("tactics-srv: send failed handle={} bytes={}\n", handle.0, data.len());
        }
    }
}

fn broadcast<T: Serialize>(vnet: &VNet, handles: &[api::NetHandle], msg: &T) {
    for &handle in handles {
        send_msg(vnet, handle, msg);
    }
}

fn send_error(vnet: &VNet, handle: api::NetHandle, message: &str) {
    crate::log!("tactics-srv: send error handle={} msg={}\n", handle.0, message);
    send_msg(
        vnet,
        handle,
        &ServerMsg::Error {
            message: message.to_string(),
        },
    );
}

fn open_tactics_listener(endpoint: &mut TacticsEndpoint) -> bool {
    if endpoint
        .vnet
        .submit(api::Command::OpenTcpListen {
            port: ports::GAMESERVER_TACTICS_TCP_PORT,
        })
        .is_err()
    {
        crate::log!(
            "tactics-srv: reopen listen failed dev={} owner={}\n",
            endpoint.dev_idx,
            endpoint.vnet.owner()
        );
        return false;
    }

    let ip = crate::net::adapter::ipv4_at(endpoint.dev_idx);
    let ip_mode = match crate::net::adapter::dhcp_has_lease_at(endpoint.dev_idx) {
        Some(true) => "dhcp",
        Some(false) => "fallback",
        None => "unknown",
    };
    match ip {
        Some([a, b, c, d]) => crate::log!(
            "tactics-srv: listening tcp {} dev={} owner={} ip={}.{}.{}.{} mode={}\n",
            ports::GAMESERVER_TACTICS_TCP_PORT,
            endpoint.dev_idx,
            endpoint.vnet.owner(),
            a,
            b,
            c,
            d,
            ip_mode
        ),
        None => crate::log!(
            "tactics-srv: listening tcp {} dev={} owner={} ip=none\n",
            ports::GAMESERVER_TACTICS_TCP_PORT,
            endpoint.dev_idx,
            endpoint.vnet.owner()
        ),
    }
    true
}

fn add_endpoints(endpoints: &mut Vec<TacticsEndpoint>) -> usize {
    let mut added = 0;
    for dev_idx in 0..crate::net::device_count() {
        if endpoints.iter().any(|endpoint| endpoint.dev_idx == dev_idx) {
            continue;
        }
        let usable = crate::net::adapter::ipv4_at(dev_idx).is_some()
            || crate::net::link_state_at(dev_idx)
                .map(|state| state.up)
                .unwrap_or(false);
        if !usable {
            continue;
        }
        let Some(vnet) = VNet::open(dev_idx) else {
            continue;
        };
        let mut endpoint = TacticsEndpoint {
            vnet,
            dev_idx,
            listen_handle: None,
        };
        if !open_tactics_listener(&mut endpoint) {
            continue;
        }
        endpoints.push(endpoint);
        added += 1;
    }
    added
}

fn handle_client_msg(
    server: &mut TacticsServer,
    vnet: &VNet,
    handle: api::NetHandle,
    client_id: u64,
    msg: ClientMsg,
) {
    match msg {
        ClientMsg::Hello {
            name,
            ping_ms,
            latency_ms,
            game,
            session_id,
        } => {
            if let Some(client) = server.clients.get_mut(&client_id) {
                crate::log!(
                    "tactics-srv: hello client={} handle={} name={} game={:?} session={} ping_ms={:?} latency_ms={:?}\n",
                    client_id,
                    handle.0,
                    name,
                    game,
                    session_id.as_deref().unwrap_or("none"),
                    ping_ms,
                    latency_ms
                );
                client.name = name;
                client.ping_ms = ping_ms;
                client.latency_ms = latency_ms;
                client.session_id = session_id;
            }
            send_msg(
                vnet,
                handle,
                &ServerMsg::Welcome {
                    client_id,
                    protocol: "trueos.tactics.v1",
                    heartbeat_ms: 1_000,
                },
            );
            send_msg(
                vnet,
                handle,
                &ServerMsg::GameList {
                    games: server.game_list(),
                },
            );
        }
        ClientMsg::Heartbeat {
            ping_ms,
            latency_ms,
        } => {
            if let Some(client) = server.clients.get_mut(&client_id) {
                if server.tick.is_multiple_of(HEARTBEAT_LOG_TICKS) {
                    crate::log!(
                        "tactics-srv: heartbeat client={} name={} ping_ms={:?} latency_ms={:?}\n",
                        client_id,
                        client.name,
                        ping_ms,
                        latency_ms
                    );
                }
                client.ping_ms = ping_ms;
                client.latency_ms = latency_ms;
                client.last_seen_tick = server.tick;
            }
            send_msg(
                vnet,
                handle,
                &ServerMsg::Ack {
                    action: "heartbeat",
                    game_id: None,
                },
            );
        }
        ClientMsg::Chat { text } => {
            let Some(from) = server
                .clients
                .get(&client_id)
                .map(|client| client.name.clone())
            else {
                return;
            };
            let handles = server
                .clients
                .get(&client_id)
                .and_then(|client| client.game_id)
                .map(|game_id| server.game_handles(game_id))
                .unwrap_or_else(|| server.all_handles());
            crate::log!(
                "tactics-srv: chat client={} name={} recipients={} bytes={}\n",
                client_id,
                from,
                handles.len(),
                text.len()
            );
            broadcast(
                vnet,
                handles.as_slice(),
                &ServerMsg::Chat {
                    from_id: client_id,
                    from,
                    text,
                },
            );
        }
        ClientMsg::CreateGame {
            name,
            game,
            max_players,
            session_id,
        } => {
            let game_id = server.next_game_id;
            server.next_game_id = server.next_game_id.saturating_add(1);
            if let Some(old_game) = server.clients.get(&client_id).and_then(|c| c.game_id) {
                crate::log!(
                    "tactics-srv: create_game client={} leaving old_game={}\n",
                    client_id,
                    old_game
                );
                server.remove_player_from_game(old_game, client_id);
            }
            if let Some(client) = server.clients.get_mut(&client_id) {
                client.game_id = Some(game_id);
                if session_id.is_some() {
                    client.session_id = session_id.clone();
                }
            }
            let max_players = max_players.unwrap_or(8).max(1);
            crate::log!(
                "tactics-srv: create_game id={} host={} session={} name={} game={} max_players={}\n",
                game_id,
                client_id,
                session_id.as_deref().unwrap_or("none"),
                name,
                game,
                max_players
            );
            server.games.insert(
                game_id,
                GameSession {
                    id: game_id,
                    name,
                    game,
                    host_id: client_id,
                    host_session_id: session_id,
                    max_players,
                    status: GameStatus::Lobby,
                    players: alloc::vec![client_id],
                    player_state: BTreeMap::new(),
                },
            );
            send_msg(
                vnet,
                handle,
                &ServerMsg::Ack {
                    action: "create_game",
                    game_id: Some(game_id),
                },
            );
            if let Some(game) = server.games.get(&game_id) {
                broadcast(
                    vnet,
                    server.all_handles().as_slice(),
                    &ServerMsg::GameCreated {
                        game: server.game_info(game),
                    },
                );
            }
        }
        ClientMsg::FreeGame {
            game_id,
            session_id,
        } => {
            if session_id.is_some()
                && let Some(client) = server.clients.get_mut(&client_id)
            {
                client.session_id = session_id.clone();
            }
            let allowed = server
                .host_allowed(game_id, client_id, session_id.as_deref())
                .unwrap_or(false);
            if !allowed {
                send_error(vnet, handle, "only the host can free this game");
                return;
            }
            if let Some(game) = server.games.remove(&game_id) {
                crate::log!(
                    "tactics-srv: free_game id={} host={} session={} players={}\n",
                    game_id,
                    client_id,
                    session_id.as_deref().unwrap_or("none"),
                    game.players.len()
                );
                for player_id in game.players {
                    if let Some(client) = server.clients.get_mut(&player_id) {
                        client.game_id = None;
                    }
                }
            }
            send_msg(
                vnet,
                handle,
                &ServerMsg::Ack {
                    action: "free_game",
                    game_id: Some(game_id),
                },
            );
            broadcast(vnet, server.all_handles().as_slice(), &ServerMsg::GameFreed { game_id });
        }
        ClientMsg::JoinGame { game_id } => {
            if !server.games.contains_key(&game_id) {
                send_error(vnet, handle, "game not found");
                return;
            }
            let full = server
                .games
                .get(&game_id)
                .map(|game| game.players.len() >= game.max_players as usize)
                .unwrap_or(true);
            if full {
                send_error(vnet, handle, "game is full");
                return;
            }
            if let Some(old_game) = server.clients.get(&client_id).and_then(|c| c.game_id) {
                crate::log!(
                    "tactics-srv: join_game client={} leaving old_game={}\n",
                    client_id,
                    old_game
                );
                server.remove_player_from_game(old_game, client_id);
            }
            if let Some(client) = server.clients.get_mut(&client_id) {
                client.game_id = Some(game_id);
            }
            if let Some(game) = server.games.get_mut(&game_id)
                && !game.players.contains(&client_id)
            {
                game.players.push(client_id);
            }
            if let Some(game) = server.games.get(&game_id) {
                send_msg(
                    vnet,
                    handle,
                    &ServerMsg::Ack {
                        action: "join_game",
                        game_id: Some(game_id),
                    },
                );
                crate::log!(
                    "tactics-srv: join_game client={} game_id={} players={}/{}\n",
                    client_id,
                    game_id,
                    game.players.len(),
                    game.max_players
                );
                broadcast(
                    vnet,
                    server.all_handles().as_slice(),
                    &ServerMsg::GameUpdated {
                        game: server.game_info(game),
                    },
                );
            }
        }
        ClientMsg::LeaveGame { game_id } => {
            let game_id =
                game_id.or_else(|| server.clients.get(&client_id).and_then(|c| c.game_id));
            if let Some(game_id) = game_id {
                crate::log!("tactics-srv: leave_game client={} game_id={}\n", client_id, game_id);
                if let Some(client) = server.clients.get_mut(&client_id) {
                    client.game_id = None;
                }
                let recipients = server.remove_player_from_game(game_id, client_id);
                send_msg(
                    vnet,
                    handle,
                    &ServerMsg::Ack {
                        action: "leave_game",
                        game_id: Some(game_id),
                    },
                );
                if let Some(game) = server.games.get(&game_id) {
                    broadcast(
                        vnet,
                        recipients.as_slice(),
                        &ServerMsg::GameUpdated {
                            game: server.game_info(game),
                        },
                    );
                } else {
                    broadcast(
                        vnet,
                        server.all_handles().as_slice(),
                        &ServerMsg::GameFreed { game_id },
                    );
                }
            }
        }
        ClientMsg::StartGame {
            game_id,
            session_id,
        } => {
            let allowed = server
                .host_allowed(game_id, client_id, session_id.as_deref())
                .unwrap_or(false);
            if !allowed {
                send_error(vnet, handle, "only the host can start this game");
                return;
            }
            if let Some(client) = server.clients.get_mut(&client_id) {
                client.game_id = Some(game_id);
                if session_id.is_some() {
                    client.session_id = session_id.clone();
                }
            }
            if let Some(game) = server.games.get_mut(&game_id) {
                if !game.players.contains(&client_id) {
                    game.players.push(client_id);
                }
                game.status = GameStatus::Running;
            }
            if let Some(game) = server.games.get(&game_id) {
                send_msg(
                    vnet,
                    handle,
                    &ServerMsg::Ack {
                        action: "start_game",
                        game_id: Some(game_id),
                    },
                );
                crate::log!(
                    "tactics-srv: start_game id={} host={} session={} players={}\n",
                    game_id,
                    client_id,
                    session_id.as_deref().unwrap_or("none"),
                    game.players.len()
                );
                broadcast(
                    vnet,
                    server.game_handles(game_id).as_slice(),
                    &ServerMsg::GameStarted {
                        game: server.game_info(game),
                    },
                );
            }
        }
        ClientMsg::PauseGame {
            game_id,
            session_id,
        } => {
            let allowed = server
                .host_allowed(game_id, client_id, session_id.as_deref())
                .unwrap_or(false);
            if allowed {
                if let Some(game) = server.games.get_mut(&game_id) {
                    game.status = GameStatus::Paused;
                }
                crate::log!(
                    "tactics-srv: pause_game id={} host={} session={}\n",
                    game_id,
                    client_id,
                    session_id.as_deref().unwrap_or("none")
                );
                send_msg(
                    vnet,
                    handle,
                    &ServerMsg::Ack {
                        action: "pause_game",
                        game_id: Some(game_id),
                    },
                );
                broadcast(
                    vnet,
                    server.game_handles(game_id).as_slice(),
                    &ServerMsg::GamePaused { game_id },
                );
            } else {
                send_error(vnet, handle, "only the host can pause this game");
            }
        }
        ClientMsg::ResumeGame {
            game_id,
            session_id,
        } => {
            let allowed = server
                .host_allowed(game_id, client_id, session_id.as_deref())
                .unwrap_or(false);
            if allowed {
                if let Some(game) = server.games.get_mut(&game_id) {
                    game.status = GameStatus::Running;
                }
                crate::log!(
                    "tactics-srv: resume_game id={} host={} session={}\n",
                    game_id,
                    client_id,
                    session_id.as_deref().unwrap_or("none")
                );
                send_msg(
                    vnet,
                    handle,
                    &ServerMsg::Ack {
                        action: "resume_game",
                        game_id: Some(game_id),
                    },
                );
                broadcast(
                    vnet,
                    server.game_handles(game_id).as_slice(),
                    &ServerMsg::GameResumed { game_id },
                );
            } else {
                send_error(vnet, handle, "only the host can resume this game");
            }
        }
        ClientMsg::FinishGame {
            game_id,
            session_id,
        } => {
            let allowed = server
                .host_allowed(game_id, client_id, session_id.as_deref())
                .unwrap_or(false);
            if allowed {
                if let Some(game) = server.games.get_mut(&game_id) {
                    game.status = GameStatus::Finished;
                }
                crate::log!(
                    "tactics-srv: finish_game id={} host={} session={}\n",
                    game_id,
                    client_id,
                    session_id.as_deref().unwrap_or("none")
                );
                send_msg(
                    vnet,
                    handle,
                    &ServerMsg::Ack {
                        action: "finish_game",
                        game_id: Some(game_id),
                    },
                );
                broadcast(
                    vnet,
                    server.game_handles(game_id).as_slice(),
                    &ServerMsg::GameFinished { game_id },
                );
            } else {
                send_error(vnet, handle, "only the host can finish this game");
            }
        }
        ClientMsg::GameList => {
            crate::log!(
                "tactics-srv: game_list client={} games={}\n",
                client_id,
                server.games.len()
            );
            send_msg(
                vnet,
                handle,
                &ServerMsg::GameList {
                    games: server.game_list(),
                },
            );
        }
        ClientMsg::GameCommand {
            game_id,
            seq,
            command,
        } => {
            let in_game = server
                .games
                .get(&game_id)
                .map(|game| game.players.contains(&client_id))
                .unwrap_or(false);
            if !in_game {
                send_error(vnet, handle, "client is not in that game");
                return;
            }
            crate::log!(
                "tactics-srv: game_command game_id={} from={} seq={:?}\n",
                game_id,
                client_id,
                seq
            );
            broadcast(
                vnet,
                server.game_handles(game_id).as_slice(),
                &ServerMsg::GameCommand {
                    game_id,
                    from_id: client_id,
                    seq,
                    command,
                },
            );
        }
        ClientMsg::Position { game_id, state } => {
            let running = server
                .games
                .get(&game_id)
                .map(|game| game.status == GameStatus::Running && game.players.contains(&client_id))
                .unwrap_or(false);
            if !running {
                send_error(vnet, handle, "game is not running for this client");
                return;
            }
            if let Some(game) = server.games.get_mut(&game_id) {
                game.player_state.insert(client_id, state);
            }
            if server.tick.is_multiple_of(POSITION_LOG_TICKS) {
                crate::log!(
                    "tactics-srv: position game_id={} client={} tick={}\n",
                    game_id,
                    client_id,
                    server.tick
                );
            }
            if let Some(state) = server.state_for_game(game_id) {
                broadcast(vnet, server.game_handles(game_id).as_slice(), &state);
            }
        }
    }
}

fn drain_endpoint(endpoint: &mut TacticsEndpoint, server: &mut TacticsServer) {
    for _ in 0..64 {
        let Some(ev) = endpoint.vnet.pop_event() else {
            break;
        };
        match ev {
            api::Event::Opened { handle, .. } => {
                endpoint.listen_handle = Some(handle);
                crate::log!("tactics-srv: opened handle={}\n", handle.0);
            }
            api::Event::Error { msg } => {
                crate::log!("tactics-srv: net error msg={}\n", msg);
            }
            api::Event::TcpEstablished { handle, .. } => {
                if endpoint.listen_handle == Some(handle) {
                    endpoint.listen_handle = None;
                    crate::log!(
                        "tactics-srv: accepted handle={} dev={} reopening listener\n",
                        handle.0,
                        endpoint.dev_idx
                    );
                    let _ = open_tactics_listener(endpoint);
                }
                let id = match server.handle_to_client.get(&handle.0).copied() {
                    Some(id) => id,
                    None => server.add_client(handle),
                };
                crate::log!("tactics-srv: tcp established handle={} client={}\n", handle.0, id);
            }
            api::Event::TcpData { handle, data } => {
                if endpoint.listen_handle == Some(handle) {
                    endpoint.listen_handle = None;
                    crate::log!(
                        "tactics-srv: accepted-data handle={} dev={} reopening listener\n",
                        handle.0,
                        endpoint.dev_idx
                    );
                    let _ = open_tactics_listener(endpoint);
                }
                let client_id = match server.handle_to_client.get(&handle.0).copied() {
                    Some(client_id) => client_id,
                    None => server.add_client(handle),
                };
                crate::log!(
                    "tactics-srv: tcp data handle={} client={} bytes={}\n",
                    handle.0,
                    client_id,
                    data.len()
                );
                for line in server.client_lines(handle, data.as_slice()) {
                    match parse_client_msg(line.as_slice()) {
                        Ok(msg) => {
                            crate::log!(
                                "tactics-srv: command handle={} client={} type={}\n",
                                handle.0,
                                client_id,
                                client_msg_label(&msg)
                            );
                            handle_client_msg(server, &endpoint.vnet, handle, client_id, msg);
                        }
                        Err(err) => {
                            log_json_parse_error(handle, client_id, line.as_slice(), err.as_str());
                            send_error(&endpoint.vnet, handle, "invalid json command");
                        }
                    }
                }
            }
            api::Event::Closed { handle } => {
                let was_listener = endpoint.listen_handle == Some(handle);
                if was_listener {
                    endpoint.listen_handle = None;
                }
                if let Some((client_id, line)) = server.take_pending_line_on_close(handle) {
                    crate::log!(
                        "tactics-srv: flushing eof command handle={} client={} bytes={}\n",
                        handle.0,
                        client_id,
                        line.len()
                    );
                    match parse_client_msg(line.as_slice()) {
                        Ok(msg) => {
                            crate::log!(
                                "tactics-srv: eof command handle={} client={} type={}\n",
                                handle.0,
                                client_id,
                                client_msg_label(&msg)
                            );
                            handle_client_msg(server, &endpoint.vnet, handle, client_id, msg);
                        }
                        Err(err) => {
                            log_json_parse_error(handle, client_id, line.as_slice(), err.as_str());
                            send_error(&endpoint.vnet, handle, "invalid json command");
                        }
                    }
                }
                if let Some(client_id) = server.handle_to_client.get(&handle.0).copied() {
                    if let Some(client) = server.clients.get(&client_id) {
                        crate::log!(
                            "tactics-srv: tcp closed handle={} client={} name={} game_id={:?}\n",
                            handle.0,
                            client_id,
                            client.name,
                            client.game_id
                        );
                    }
                }
                for (game_id, recipients) in server.remove_handle(handle) {
                    if let Some(game) = server.games.get(&game_id) {
                        broadcast(
                            &endpoint.vnet,
                            recipients.as_slice(),
                            &ServerMsg::GameUpdated {
                                game: server.game_info(game),
                            },
                        );
                    } else {
                        broadcast(
                            &endpoint.vnet,
                            server.all_handles().as_slice(),
                            &ServerMsg::GameFreed { game_id },
                        );
                    }
                }
                if was_listener {
                    crate::log!(
                        "tactics-srv: listener closed handle={} dev={} reopening\n",
                        handle.0,
                        endpoint.dev_idx
                    );
                    let _ = open_tactics_listener(endpoint);
                }
            }
            api::Event::TcpSent { .. }
            | api::Event::UdpPacket { .. }
            | api::Event::UdpPacketV6 { .. }
            | api::Event::IcmpReply { .. }
            | api::Event::IcmpReplyV6 { .. } => {}
        }
    }
}

#[task]
pub async fn tactics_srv_task() {
    crate::r::readiness::wait_for(crate::r::readiness::NET_ANY_CONFIGURED).await;

    let mut endpoints = Vec::new();
    let mut server = TacticsServer::new();
    add_endpoints(&mut endpoints);

    loop {
        if server.tick.is_multiple_of(200) {
            add_endpoints(&mut endpoints);
        }

        for endpoint in &mut endpoints {
            drain_endpoint(endpoint, &mut server);
        }

        if server.tick.is_multiple_of(STATE_BROADCAST_TICKS) {
            let running_games: Vec<u64> = server
                .games
                .values()
                .filter(|game| game.status == GameStatus::Running)
                .map(|game| game.id)
                .collect();
            for game_id in running_games {
                if let Some(state) = server.state_for_game(game_id) {
                    for endpoint in &endpoints {
                        broadcast(&endpoint.vnet, server.game_handles(game_id).as_slice(), &state);
                    }
                }
            }
        }

        let stale: Vec<api::NetHandle> = server
            .clients
            .values()
            .filter(|client| {
                server.tick.saturating_sub(client.last_seen_tick) > HEARTBEAT_TIMEOUT_TICKS
            })
            .map(|client| client.handle)
            .collect();
        for handle in stale {
            if let Some(client_id) = server.handle_to_client.get(&handle.0).copied()
                && let Some(client) = server.clients.get(&client_id)
            {
                crate::log!(
                    "tactics-srv: heartbeat timeout handle={} client={} name={} game_id={:?}\n",
                    handle.0,
                    client_id,
                    client.name,
                    client.game_id
                );
            }
            for endpoint in &endpoints {
                let _ = endpoint.vnet.submit(api::Command::Close { handle });
            }
            server.remove_handle(handle);
        }

        server.tick = server.tick.wrapping_add(1);
        Timer::after(EmbassyDuration::from_millis(TICK_MS)).await;
    }
}

/*
Tiny client sketch (JSON lines over TCP port 1337):

use serde_json::json;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

fn send(stream: &mut TcpStream, value: serde_json::Value) -> std::io::Result<()> {
    writeln!(stream, "{}", value)
}

fn main() -> std::io::Result<()> {
    let mut stream = TcpStream::connect("TRUEOS_IP:1337")?;
    stream.set_nodelay(true)?;

    let session_id = "local-dev-session";
    let started = Instant::now();
    send(&mut stream, json!({
        "type": "hello",
        "name": "Ada",
        "ping_ms": 0,
        "latency_ms": 0,
        "game": "tactics",
        "session_id": session_id
    }))?;
    send(&mut stream, json!({"type": "game_list"}))?;
    send(&mut stream, json!({
        "type": "create_game",
        "name": "Friday lobby",
        "game": "tactics",
        "max_players": 4,
        "session_id": session_id
    }))?;
    send(&mut stream, json!({"type": "start_game", "game_id": 1, "session_id": session_id}))?;

    let mut read = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    loop {
        if started.elapsed() > Duration::from_secs(1) {
            send(&mut stream, json!({"type": "heartbeat", "ping_ms": 12, "latency_ms": 6}))?;
            send(&mut stream, json!({
                "type": "position",
                "game_id": 1,
                "state": {"x": 12.0, "y": 4.0, "facing": "east"}
            }))?;
        }

        line.clear();
        if read.read_line(&mut line)? == 0 {
            break;
        }
        println!("server: {}", line.trim_end());
    }
    Ok(())
}

Useful commands:
{"type":"chat","text":"hello"}
{"type":"join_game","game_id":1}
{"type":"pause_game","game_id":1,"session_id":"local-dev-session"}
{"type":"resume_game","game_id":1,"session_id":"local-dev-session"}
{"type":"game_command","game_id":1,"seq":42,"command":{"move":{"dx":1,"dy":0}}}
{"type":"free_game","game_id":1,"session_id":"local-dev-session"}
*/
