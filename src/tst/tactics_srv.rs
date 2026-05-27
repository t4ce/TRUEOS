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

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    Hello {
        name: String,
        ping_ms: Option<u32>,
        latency_ms: Option<u32>,
        game: Option<String>,
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
    },
    FreeGame {
        game_id: u64,
    },
    JoinGame {
        game_id: u64,
    },
    LeaveGame {
        game_id: Option<u64>,
    },
    StartGame {
        game_id: u64,
    },
    PauseGame {
        game_id: u64,
    },
    ResumeGame {
        game_id: u64,
    },
    FinishGame {
        game_id: u64,
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
    max_players: u16,
    status: GameStatus,
    players: Vec<u64>,
    player_state: BTreeMap<u64, Value>,
}

struct TacticsEndpoint {
    vnet: VNet,
    dev_idx: usize,
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
            let recipients = self.remove_player_from_game(game_id, client_id);
            updates.push((game_id, recipients));
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
        lines
    }
}

fn send_msg<T: Serialize>(vnet: &VNet, handle: api::NetHandle, msg: &T) {
    if let Ok(mut data) = serde_json::to_vec(msg) {
        data.push(b'\n');
        let _ = vnet.send_tcp_all(handle, data.as_slice());
    }
}

fn broadcast<T: Serialize>(vnet: &VNet, handles: &[api::NetHandle], msg: &T) {
    for &handle in handles {
        send_msg(vnet, handle, msg);
    }
}

fn send_error(vnet: &VNet, handle: api::NetHandle, message: &str) {
    send_msg(
        vnet,
        handle,
        &ServerMsg::Error {
            message: message.to_string(),
        },
    );
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
        if vnet
            .submit(api::Command::OpenTcpListen {
                port: ports::GAMESERVER_TACTICS_TCP_PORT,
            })
            .is_err()
        {
            continue;
        }

        let ip = crate::net::adapter::ipv4_at(dev_idx);
        match ip {
            Some([a, b, c, d]) => crate::log!(
                "tactics-srv: listening tcp {} dev={} owner={} ip={}.{}.{}.{}\n",
                ports::GAMESERVER_TACTICS_TCP_PORT,
                dev_idx,
                vnet.owner(),
                a,
                b,
                c,
                d
            ),
            None => crate::log!(
                "tactics-srv: listening tcp {} dev={} owner={} ip=none\n",
                ports::GAMESERVER_TACTICS_TCP_PORT,
                dev_idx,
                vnet.owner()
            ),
        }
        endpoints.push(TacticsEndpoint { vnet, dev_idx });
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
            game: _,
        } => {
            if let Some(client) = server.clients.get_mut(&client_id) {
                client.name = name;
                client.ping_ms = ping_ms;
                client.latency_ms = latency_ms;
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
        } => {
            let game_id = server.next_game_id;
            server.next_game_id = server.next_game_id.saturating_add(1);
            if let Some(old_game) = server.clients.get(&client_id).and_then(|c| c.game_id) {
                server.remove_player_from_game(old_game, client_id);
            }
            if let Some(client) = server.clients.get_mut(&client_id) {
                client.game_id = Some(game_id);
            }
            server.games.insert(
                game_id,
                GameSession {
                    id: game_id,
                    name,
                    game,
                    host_id: client_id,
                    max_players: max_players.unwrap_or(8).max(1),
                    status: GameStatus::Lobby,
                    players: alloc::vec![client_id],
                    player_state: BTreeMap::new(),
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
        ClientMsg::FreeGame { game_id } => {
            let allowed = server
                .games
                .get(&game_id)
                .map(|game| game.host_id == client_id)
                .unwrap_or(false);
            if !allowed {
                send_error(vnet, handle, "only the host can free this game");
                return;
            }
            if let Some(game) = server.games.remove(&game_id) {
                for player_id in game.players {
                    if let Some(client) = server.clients.get_mut(&player_id) {
                        client.game_id = None;
                    }
                }
            }
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
                if let Some(client) = server.clients.get_mut(&client_id) {
                    client.game_id = None;
                }
                let recipients = server.remove_player_from_game(game_id, client_id);
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
        ClientMsg::StartGame { game_id } => {
            let allowed = server
                .games
                .get(&game_id)
                .map(|game| game.host_id == client_id)
                .unwrap_or(false);
            if !allowed {
                send_error(vnet, handle, "only the host can start this game");
                return;
            }
            if let Some(game) = server.games.get_mut(&game_id) {
                game.status = GameStatus::Running;
            }
            if let Some(game) = server.games.get(&game_id) {
                broadcast(
                    vnet,
                    server.game_handles(game_id).as_slice(),
                    &ServerMsg::GameStarted {
                        game: server.game_info(game),
                    },
                );
            }
        }
        ClientMsg::PauseGame { game_id } => {
            if let Some(game) = server.games.get_mut(&game_id)
                && game.host_id == client_id
            {
                game.status = GameStatus::Paused;
                broadcast(
                    vnet,
                    server.game_handles(game_id).as_slice(),
                    &ServerMsg::GamePaused { game_id },
                );
            }
        }
        ClientMsg::ResumeGame { game_id } => {
            if let Some(game) = server.games.get_mut(&game_id)
                && game.host_id == client_id
            {
                game.status = GameStatus::Running;
                broadcast(
                    vnet,
                    server.game_handles(game_id).as_slice(),
                    &ServerMsg::GameResumed { game_id },
                );
            }
        }
        ClientMsg::FinishGame { game_id } => {
            if let Some(game) = server.games.get_mut(&game_id)
                && game.host_id == client_id
            {
                game.status = GameStatus::Finished;
                broadcast(
                    vnet,
                    server.game_handles(game_id).as_slice(),
                    &ServerMsg::GameFinished { game_id },
                );
            }
        }
        ClientMsg::GameList => {
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
            api::Event::Opened { .. } => {}
            api::Event::TcpEstablished { handle, .. } => {
                let id = server.add_client(handle);
                send_msg(
                    &endpoint.vnet,
                    handle,
                    &ServerMsg::Welcome {
                        client_id: id,
                        protocol: "trueos.tactics.v1",
                        heartbeat_ms: 1_000,
                    },
                );
            }
            api::Event::TcpData { handle, data } => {
                let client_id = match server.handle_to_client.get(&handle.0).copied() {
                    Some(client_id) => client_id,
                    None => server.add_client(handle),
                };
                for line in server.client_lines(handle, data.as_slice()) {
                    match serde_json::from_slice::<ClientMsg>(line.as_slice()) {
                        Ok(msg) => {
                            handle_client_msg(server, &endpoint.vnet, handle, client_id, msg);
                        }
                        Err(_) => send_error(&endpoint.vnet, handle, "invalid json command"),
                    }
                }
            }
            api::Event::Closed { handle } => {
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
            }
            api::Event::Error { msg } => {
                if msg != "bad handle" {
                    crate::log!("tactics-srv: net error {}\n", msg);
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

    let started = Instant::now();
    send(&mut stream, json!({
        "type": "hello",
        "name": "Ada",
        "ping_ms": 0,
        "latency_ms": 0,
        "game": "tactics"
    }))?;
    send(&mut stream, json!({"type": "game_list"}))?;
    send(&mut stream, json!({
        "type": "create_game",
        "name": "Friday lobby",
        "game": "tactics",
        "max_players": 4
    }))?;
    send(&mut stream, json!({"type": "start_game", "game_id": 1}))?;

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
{"type":"pause_game","game_id":1}
{"type":"resume_game","game_id":1}
{"type":"game_command","game_id":1,"seq":42,"command":{"move":{"dx":1,"dy":0}}}
{"type":"free_game","game_id":1}
*/
