#![no_std]

extern crate alloc;

use alloc::{collections::VecDeque, format, string::String, vec::Vec};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChatConfig {
    pub max_rooms: usize,
    pub max_messages_per_room: usize,
    pub max_name_len: usize,
    pub max_message_len: usize,
}

impl Default for ChatConfig {
    fn default() -> Self {
        Self {
            max_rooms: 32,
            max_messages_per_room: 128,
            max_name_len: 32,
            max_message_len: 1024,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChatMethod {
    Get,
    Post,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatRequest {
    pub method: ChatMethod,
    pub path: String,
    pub query: Option<String>,
    pub body: Vec<u8>,
    pub now_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatResponse {
    pub status: u16,
    pub content_type: &'static str,
    pub body: Vec<u8>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Message {
    id: u64,
    user: String,
    text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    statement: Option<String>,
    unix_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct Room {
    name: String,
    messages: VecDeque<Message>,
}

#[derive(Debug)]
pub struct ChatHub {
    config: ChatConfig,
    next_id: u64,
    rooms: Vec<Room>,
}

#[derive(Serialize)]
struct RoomsJson<'a> {
    rooms: Vec<RoomJson<'a>>,
}

#[derive(Serialize)]
struct ApiJson {
    ok: bool,
    transport: &'static str,
    endpoints: &'static [&'static str],
    post_body: &'static str,
}

#[derive(Serialize)]
struct RoomJson<'a> {
    room: &'a str,
    messages: usize,
}

#[derive(Serialize)]
struct MessagesJson<'a> {
    room: &'a str,
    messages: Vec<&'a Message>,
}

#[derive(Serialize)]
struct PostOkJson {
    ok: bool,
    id: u64,
    message: Message,
}

#[derive(Deserialize)]
struct PostMessageJson {
    user: String,
    text: String,
    #[serde(default)]
    statement: Option<String>,
}

#[derive(Serialize)]
struct ErrorJson<'a> {
    ok: bool,
    error: &'a str,
}

#[derive(Deserialize, Serialize)]
struct ChatSnapshot {
    next_id: u64,
    rooms: Vec<Room>,
}

impl ChatHub {
    pub fn new(config: ChatConfig) -> Self {
        Self {
            config,
            next_id: 1,
            rooms: Vec::new(),
        }
    }

    pub fn room_count(&self) -> usize {
        self.rooms.len()
    }

    pub fn message_count(&self, room: &str) -> usize {
        let Some(name) = sanitize_name(room, self.config.max_name_len) else {
            return 0;
        };
        self.rooms
            .iter()
            .find(|candidate| candidate.name == name)
            .map(|room| room.messages.len())
            .unwrap_or(0)
    }

    pub fn to_json_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(&ChatSnapshot {
            next_id: self.next_id,
            rooms: self.rooms.clone(),
        })
        .unwrap_or_else(|_| b"{\"next_id\":1,\"rooms\":[]}".to_vec())
    }

    pub fn from_json_bytes(config: ChatConfig, bytes: &[u8]) -> Result<Self, ()> {
        let snapshot: ChatSnapshot = serde_json::from_slice(bytes).map_err(|_| ())?;
        let mut rooms = Vec::new();
        let mut max_id = 0u64;

        for mut room in snapshot.rooms.into_iter() {
            if rooms.len() >= config.max_rooms {
                break;
            }
            let Some(name) = sanitize_name(room.name.as_str(), config.max_name_len) else {
                continue;
            };
            if rooms.iter().any(|candidate: &Room| candidate.name == name) {
                continue;
            }

            let mut messages = VecDeque::new();
            for mut message in room.messages.drain(..) {
                if message.id == 0 {
                    continue;
                }
                let Some(user) = sanitize_text(message.user.as_str(), config.max_name_len) else {
                    continue;
                };
                let Some(text) = sanitize_text(message.text.as_str(), config.max_message_len)
                else {
                    continue;
                };
                message.user = user;
                message.text = text;
                max_id = max_id.max(message.id);
                messages.push_back(message);
                while messages.len() > config.max_messages_per_room {
                    messages.pop_front();
                }
            }

            rooms.push(Room { name, messages });
        }

        Ok(Self {
            config,
            next_id: snapshot.next_id.max(max_id.saturating_add(1)).max(1),
            rooms,
        })
    }

    pub fn handle(&mut self, req: ChatRequest) -> ChatResponse {
        match (req.method, req.path.as_str()) {
            (ChatMethod::Get, "/") => html_response(index_html()),
            (ChatMethod::Get, "/api") => api_response(),
            (ChatMethod::Get, "/api/rooms") => self.rooms_response(),
            _ => {
                if let Some(room) = api_room_messages_path(&req.path) {
                    match req.method {
                        ChatMethod::Get => self.messages_response(room, req.query.as_deref()),
                        ChatMethod::Post => self.post_message_response(room, &req.body, req.now_ms),
                        ChatMethod::Other => error_response(405, "method not allowed"),
                    }
                } else {
                    error_response(404, "not found")
                }
            }
        }
    }

    fn rooms_response(&self) -> ChatResponse {
        let rooms = self
            .rooms
            .iter()
            .map(|room| RoomJson {
                room: room.name.as_str(),
                messages: room.messages.len(),
            })
            .collect();
        json_response(200, &RoomsJson { rooms })
    }

    fn messages_response(&self, room: &str, query: Option<&str>) -> ChatResponse {
        let Some(room_name) = sanitize_name(room, self.config.max_name_len) else {
            return error_response(400, "invalid room");
        };
        let since = query.and_then(parse_since).unwrap_or(0);
        let messages = self
            .rooms
            .iter()
            .find(|room| room.name == room_name)
            .map(|room| {
                room.messages
                    .iter()
                    .filter(|message| message.id > since)
                    .collect()
            })
            .unwrap_or_else(Vec::new);
        json_response(
            200,
            &MessagesJson {
                room: room_name.as_str(),
                messages,
            },
        )
    }

    fn post_message_response(&mut self, room: &str, body: &[u8], now_ms: u64) -> ChatResponse {
        let Some(room_name) = sanitize_name(room, self.config.max_name_len) else {
            return error_response(400, "invalid room");
        };
        let (user_raw, text_raw, statement_raw) = post_message_fields(body);
        let Some(user) = user_raw.and_then(|value| sanitize_text(&value, self.config.max_name_len))
        else {
            return error_response(400, "invalid user");
        };
        let Some(text) =
            text_raw.and_then(|value| sanitize_text(&value, self.config.max_message_len))
        else {
            return error_response(400, "invalid text");
        };
        let statement =
            statement_raw.and_then(|value| sanitize_statement(&value, self.config.max_name_len));

        if self.rooms.iter().all(|room| room.name != room_name) {
            if self.rooms.len() >= self.config.max_rooms {
                return error_response(507, "room limit reached");
            }
            self.rooms.push(Room {
                name: room_name.clone(),
                messages: VecDeque::new(),
            });
        }

        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1).max(1);
        let unix_ms = if now_ms == 0 { id } else { now_ms };

        let mut message = Message {
            id,
            user,
            text,
            statement: statement.clone(),
            unix_ms,
        };
        if let Some(room) = self.rooms.iter_mut().find(|room| room.name == room_name) {
            if let Some(statement) = statement.as_deref() {
                if let Some(existing) = room
                    .messages
                    .iter_mut()
                    .find(|message| message.statement.as_deref() == Some(statement))
                {
                    existing.id = id;
                    existing.unix_ms = unix_ms;
                    existing.user = message.user.clone();
                    existing.text = append_limited_text(
                        existing.text.as_str(),
                        message.text.as_str(),
                        self.config.max_message_len,
                    );
                    message = existing.clone();
                } else {
                    room.messages.push_back(message.clone());
                }
            } else {
                room.messages.push_back(message.clone());
            }
            while room.messages.len() > self.config.max_messages_per_room {
                room.messages.pop_front();
            }
        }

        json_response(
            200,
            &PostOkJson {
                ok: true,
                id,
                message,
            },
        )
    }
}

fn api_room_messages_path(path: &str) -> Option<&str> {
    let rest = path.strip_prefix("/api/rooms/")?;
    rest.strip_suffix("/messages")
}

fn parse_since(query: &str) -> Option<u64> {
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        if key == "since" {
            return value.parse().ok();
        }
    }
    None
}

fn post_message_fields(body: &[u8]) -> (Option<String>, Option<String>, Option<String>) {
    let trimmed = trim_ascii_ws(body);
    if trimmed.first() == Some(&b'{') {
        if let Ok(json) = serde_json::from_slice::<PostMessageJson>(trimmed) {
            return (Some(json.user), Some(json.text), json.statement);
        }
    }
    (
        form_value(body, "user"),
        form_value(body, "text"),
        form_value(body, "statement"),
    )
}

fn append_limited_text(existing: &str, delta: &str, max_len: usize) -> String {
    let mut out = String::new();
    for ch in existing.chars().chain(delta.chars()) {
        if out.len().saturating_add(ch.len_utf8()) > max_len {
            break;
        }
        out.push(ch);
    }
    out
}

fn trim_ascii_ws(mut bytes: &[u8]) -> &[u8] {
    while matches!(bytes.first(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
        bytes = &bytes[1..];
    }
    while matches!(bytes.last(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
        bytes = &bytes[..bytes.len().saturating_sub(1)];
    }
    bytes
}

fn sanitize_name(raw: &str, max_len: usize) -> Option<String> {
    let mut out = String::new();
    for ch in raw.trim().chars() {
        let mapped = match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' => ch.to_ascii_lowercase(),
            '-' | '_' => ch,
            ' ' => '-',
            _ => continue,
        };
        if out.len() < max_len {
            out.push(mapped);
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

fn sanitize_statement(raw: &str, max_len: usize) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut out = String::new();
    for ch in trimmed.chars() {
        if out.len().saturating_add(ch.len_utf8()) > max_len {
            break;
        }
        match ch {
            'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' | '.' | ':' => out.push(ch),
            _ => return None,
        }
    }
    if out.is_empty() { None } else { Some(out) }
}

fn sanitize_text(raw: &str, max_len: usize) -> Option<String> {
    let mut out = String::new();
    for ch in raw.trim().chars() {
        if ch == '\0' || ch == '\r' {
            continue;
        }
        if out.len() + ch.len_utf8() > max_len {
            break;
        }
        out.push(ch);
    }
    if out.trim().is_empty() {
        None
    } else {
        Some(out)
    }
}

fn form_value(body: &[u8], key: &str) -> Option<String> {
    let body = core::str::from_utf8(body).ok()?;
    for pair in body.split('&') {
        let (raw_key, raw_value) = pair.split_once('=').unwrap_or((pair, ""));
        if url_decode(raw_key, 64).as_deref() == Some(key) {
            return url_decode(raw_value, 8 * 1024);
        }
    }
    None
}

fn url_decode(raw: &str, max_len: usize) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut out = Vec::new();
    let mut idx = 0;
    while idx < bytes.len() {
        let byte = match bytes[idx] {
            b'+' => {
                idx += 1;
                b' '
            }
            b'%' if idx + 2 < bytes.len() => {
                let hi = hex(bytes[idx + 1])?;
                let lo = hex(bytes[idx + 2])?;
                idx += 3;
                (hi << 4) | lo
            }
            b'%' => return None,
            other => {
                idx += 1;
                other
            }
        };
        if out.len() >= max_len {
            break;
        }
        out.push(byte);
    }
    String::from_utf8(out).ok()
}

fn hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn html_escape(raw: &str) -> String {
    let mut out = String::new();
    for ch in raw.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

fn index_html() -> Vec<u8> {
    let title = html_escape("TRUEOS Chat");
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title}</title>
<style>
body{{font:16px system-ui,sans-serif;margin:0;background:#101820;color:#eef3f7}}
main{{max-width:760px;margin:0 auto;padding:24px}}
label{{display:block;margin:12px 0 4px;color:#b7c6d1}}
input,button{{font:inherit;border:1px solid #41515e;background:#17242e;color:#eef3f7;padding:10px;border-radius:6px}}
input{{box-sizing:border-box;width:100%}}
input:disabled{{color:#b7c6d1;background:#13202a;cursor:not-allowed;opacity:1}}
button{{cursor:pointer;background:#2d6cdf;border-color:#2d6cdf}}
button:disabled{{cursor:wait;opacity:.72}}
.settings{{display:grid;grid-template-columns:1fr 1fr;gap:12px;align-items:end}}
#messages{{list-style:none;padding:0;margin:20px 0;display:grid;gap:10px}}
#messages li{{background:#17242e;border:1px solid #2c3c48;border-radius:6px;padding:10px}}
.meta{{color:#91a7b8;font-size:13px;margin-bottom:4px}}
.status{{margin:14px 0 0;color:#b7c6d1;font-size:13px;min-height:18px}}
form{{display:grid;grid-template-columns:1fr auto;gap:10px;align-items:end}}
@media (max-width:560px){{.settings{{grid-template-columns:1fr}}}}
</style>
</head>
<body>
<main>
<h1>TRUEOS Chat</h1>
<div class="settings">
<div><label for="room">Room</label><input id="room" value="lobby" maxlength="32" disabled></div>
<div><label for="user">Display name</label><input id="user" value="guest" maxlength="32"></div>
</div>
<ul id="messages"></ul>
<div id="status" class="status" aria-live="polite"></div>
<form id="post"><input id="text" autocomplete="off" maxlength="1024"><button>Send</button></form>
</main>
<script>
let since=0;
const room='lobby';
const bubbles=new Map();
const status=document.querySelector('#status');
const textInput=document.querySelector('#text');
const sendButton=document.querySelector('#post button');
const esc=s=>s.replace(/[&<>"']/g,c=>({{'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}}[c]));
function setStatus(text){{status.textContent=text;}}
function showMessage(m,source){{
  since=Math.max(since,m.id);
  const key=m.statement?`statement:${{m.statement}}`:`message:${{m.id}}`;
  let li=bubbles.get(key);
  if(!li){{
    li=document.createElement('li');
    bubbles.set(key,li);
    document.querySelector('#messages').appendChild(li);
  }}
  const tag=m.statement?` statement ${{esc(m.statement)}}`:'';
  li.innerHTML=`<div class="meta">${{esc(m.user)}} #${{m.id}}${{tag}} - ${{source}}</div><div>${{esc(m.text)}}</div>`;
}}
async function poll(){{
  const encodedRoom=encodeURIComponent(room);
  const started=Date.now();
  try{{
    const r=await fetch(`/api/rooms/${{encodedRoom}}/messages?since=${{since}}`,{{cache:'no-store'}});
    const ms=Date.now()-started;
    if(!r.ok){{setStatus(`poll failed HTTP ${{r.status}} after ${{ms}}ms`);return;}}
    const data=await r.json();
    for(const m of data.messages)showMessage(m,'poll');
    setStatus(`poll ok ${{ms}}ms - since #${{since}}`);
  }}catch(err){{
    setStatus(`poll error: ${{err&&err.message?err.message:'network'}}`);
  }}
}}
document.querySelector('#post').addEventListener('submit',async e=>{{
  e.preventDefault();
  const text=textInput.value;
  if(!text.trim())return;
  const encodedRoom=encodeURIComponent(room);
  const body=JSON.stringify({{user:document.querySelector('#user').value,text}});
  const started=Date.now();
  sendButton.disabled=true;
  setStatus('sending...');
  try{{
    const r=await fetch(`/api/rooms/${{encodedRoom}}/messages`,{{method:'POST',headers:{{'content-type':'application/json'}},body}});
    const ms=Date.now()-started;
    if(!r.ok){{setStatus(`send failed HTTP ${{r.status}} after ${{ms}}ms`);return;}}
    const data=await r.json();
    if(data.message)showMessage(data.message,'ack');
    textInput.value='';
    setStatus(`send ack #${{data.id}} ${{ms}}ms`);
  }}catch(err){{
    setStatus(`send error: ${{err&&err.message?err.message:'network'}}`);
  }}finally{{
    sendButton.disabled=false;
    textInput.focus();
  }}
}});
setInterval(poll,1500);poll();
</script>
</body>
</html>
"#
    )
    .into_bytes()
}

fn html_response(body: Vec<u8>) -> ChatResponse {
    ChatResponse {
        status: 200,
        content_type: "text/html; charset=utf-8",
        body,
    }
}

fn api_response() -> ChatResponse {
    json_response(
        200,
        &ApiJson {
            ok: true,
            transport: "http-post",
            endpoints: &[
                "GET /",
                "GET /api",
                "GET /api/rooms",
                "GET /api/rooms/{room}/messages?since={id}",
                "POST /api/rooms/{room}/messages",
            ],
            post_body: "application/json {\"user\":\"name\",\"text\":\"message\",\"statement\":\"optional-tag\"}; form user=&text=&statement= is also accepted",
        },
    )
}

fn json_response<T: Serialize>(status: u16, value: &T) -> ChatResponse {
    let body = serde_json::to_vec(value).unwrap_or_else(|_| b"{\"ok\":false}".to_vec());
    ChatResponse {
        status,
        content_type: "application/json; charset=utf-8",
        body,
    }
}

fn error_response(status: u16, error: &'static str) -> ChatResponse {
    json_response(status, &ErrorJson { ok: false, error })
}

#[cfg(test)]
extern crate std;

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::ToString;

    #[test]
    fn post_and_poll_messages() {
        let mut hub = ChatHub::new(ChatConfig::default());
        let post = hub.handle(ChatRequest {
            method: ChatMethod::Post,
            path: "/api/rooms/Lobby/messages".to_string(),
            query: None,
            body: b"user=Ada&text=hello+there".to_vec(),
            now_ms: 42,
        });
        assert_eq!(post.status, 200);
        let post_text = String::from_utf8(post.body).unwrap();
        assert!(post_text.contains("\"id\":1"));
        assert!(post_text.contains("\"message\""));
        assert!(post_text.contains("\"text\":\"hello there\""));
        assert_eq!(hub.room_count(), 1);
        assert_eq!(hub.message_count("lobby"), 1);

        let get = hub.handle(ChatRequest {
            method: ChatMethod::Get,
            path: "/api/rooms/lobby/messages".to_string(),
            query: Some("since=0".to_string()),
            body: Vec::new(),
            now_ms: 0,
        });
        assert_eq!(get.status, 200);
        let text = String::from_utf8(get.body).unwrap();
        assert!(text.contains("\"user\":\"Ada\""));
        assert!(text.contains("\"text\":\"hello there\""));
    }

    #[test]
    fn post_json_message() {
        let mut hub = ChatHub::new(ChatConfig::default());
        let post = hub.handle(ChatRequest {
            method: ChatMethod::Post,
            path: "/api/rooms/Lobby/messages".to_string(),
            query: None,
            body: br#"{"user":"Ada","text":"hello json"}"#.to_vec(),
            now_ms: 42,
        });
        assert_eq!(post.status, 200);
        let post_text = String::from_utf8(post.body).unwrap();
        assert!(post_text.contains("\"message\""));
        assert!(post_text.contains("\"text\":\"hello json\""));
        assert_eq!(hub.message_count("lobby"), 1);

        let get = hub.handle(ChatRequest {
            method: ChatMethod::Get,
            path: "/api/rooms/lobby/messages".to_string(),
            query: Some("since=0".to_string()),
            body: Vec::new(),
            now_ms: 0,
        });
        let text = String::from_utf8(get.body).unwrap();
        assert!(text.contains("\"user\":\"Ada\""));
        assert!(text.contains("\"text\":\"hello json\""));
    }

    #[test]
    fn api_endpoint_describes_http_contract() {
        let mut hub = ChatHub::new(ChatConfig::default());
        let get = hub.handle(ChatRequest {
            method: ChatMethod::Get,
            path: "/api".to_string(),
            query: None,
            body: Vec::new(),
            now_ms: 0,
        });
        assert_eq!(get.status, 200);
        let text = String::from_utf8(get.body).unwrap();
        assert!(text.contains("\"transport\":\"http-post\""));
        assert!(text.contains("POST /api/rooms/{room}/messages"));
    }

    #[test]
    fn caps_room_history() {
        let mut hub = ChatHub::new(ChatConfig {
            max_messages_per_room: 1,
            ..ChatConfig::default()
        });
        for text in ["one", "two"] {
            let body = format!("user=Ada&text={}", text).into_bytes();
            assert_eq!(
                hub.handle(ChatRequest {
                    method: ChatMethod::Post,
                    path: "/api/rooms/lobby/messages".to_string(),
                    query: None,
                    body,
                    now_ms: 0,
                })
                .status,
                200
            );
        }
        assert_eq!(hub.message_count("lobby"), 1);
        let get = hub.handle(ChatRequest {
            method: ChatMethod::Get,
            path: "/api/rooms/lobby/messages".to_string(),
            query: Some("since=0".to_string()),
            body: Vec::new(),
            now_ms: 0,
        });
        let text = String::from_utf8(get.body).unwrap();
        assert!(!text.contains("\"one\""));
        assert!(text.contains("\"two\""));
    }
}
