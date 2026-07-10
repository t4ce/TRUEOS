extern crate alloc;

use alloc::{collections::VecDeque, format, vec::Vec};

use embassy_time::{Duration, Instant, Timer};
use embedded_websocket::{
    WebSocketKey, WebSocketReceiveMessageType, WebSocketSendMessageType, WebSocketServer,
};
use v::vnet as api;

use crate::r::net::{VNet, ports};

const PAGE_PATH: &str = "/";
const LEGACY_PAGE_PATH: &str = "/audio/live";
const WAV_PATH: &str = "/audio/live.wav";
const STATUS_PATH: &str = "/audio/status";
const SEND_PATH: &str = "/audio/send";
const RX_BUF_MAX: usize = 8 * 1024;
const VOICE_RX_BUF_MAX: usize = 32 * 1024;
const MAX_STREAM_CLIENTS: usize = 4;
const MAX_VOICE_SENDERS: usize = 3;
const MAX_IDLE_REQUESTS: usize = 4;
const MAX_SESSIONS: usize = MAX_STREAM_CLIENTS + MAX_VOICE_SENDERS + MAX_IDLE_REQUESTS;
const SAMPLE_RATE: usize = 48_000;
const CHANNELS: usize = 2;
const PREROLL_MS: usize = 150;
const VOICE_RING_MS: usize = 500;
const SEND_MS: usize = 50;
const POLL_MS: u64 = 5;
const REQUEST_IDLE_TIMEOUT_MS: u64 = 750;
const FIXED_SEND_TIMEOUT_MS: u64 = 1_500;
const VOICE_IDLE_TIMEOUT_MS: u64 = 5_000;
const WS_TX_BUF_MAX: usize = 1024;
const WS_FRAME_BUF_MAX: usize = 4 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SessionMode {
    ReadingRequest,
    SendingFixed,
    Streaming,
    VoiceSending,
}

struct AudioHttpSession {
    handle: api::NetHandle,
    ws: WebSocketServer,
    rx: Vec<u8>,
    mode: SessionMode,
    cursor: u64,
    voice_cursors: [u64; MAX_VOICE_SENDERS],
    exclude_sender_id: Option<u32>,
    voice_slot: Option<usize>,
    sender_id: Option<u32>,
    sender_rate: u32,
    resample_accum: u64,
    pending_bytes: usize,
    sent_bytes: usize,
    deadline: Instant,
    stream_chunks: usize,
    stream_samples: usize,
}

struct AudioHttpEndpoint {
    vnet: VNet,
    listener: Option<api::NetHandle>,
    listener_ready: bool,
    dev_idx: usize,
    sessions: Vec<AudioHttpSession>,
}

impl AudioHttpSession {
    fn new(handle: api::NetHandle) -> Self {
        Self {
            handle,
            ws: WebSocketServer::new_server(),
            rx: Vec::new(),
            mode: SessionMode::ReadingRequest,
            cursor: 0,
            voice_cursors: [0; MAX_VOICE_SENDERS],
            exclude_sender_id: None,
            voice_slot: None,
            sender_id: None,
            sender_rate: SAMPLE_RATE as u32,
            resample_accum: 0,
            pending_bytes: 0,
            sent_bytes: 0,
            deadline: Instant::now() + Duration::from_millis(REQUEST_IDLE_TIMEOUT_MS),
            stream_chunks: 0,
            stream_samples: 0,
        }
    }

    fn refresh_request_deadline(&mut self) {
        self.deadline = Instant::now() + Duration::from_millis(REQUEST_IDLE_TIMEOUT_MS);
    }

    fn refresh_fixed_deadline(&mut self) {
        self.deadline = Instant::now() + Duration::from_millis(FIXED_SEND_TIMEOUT_MS);
    }

    fn refresh_voice_deadline(&mut self) {
        self.deadline = Instant::now() + Duration::from_millis(VOICE_IDLE_TIMEOUT_MS);
    }

    fn is_timed_out(&self, now: Instant) -> bool {
        match self.mode {
            SessionMode::ReadingRequest | SessionMode::SendingFixed => now >= self.deadline,
            SessionMode::VoiceSending => now >= self.deadline,
            SessionMode::Streaming => false,
        }
    }
}

struct VoiceSlot {
    sender_id: Option<u32>,
    samples: VecDeque<i16>,
    first_seq: u64,
    next_seq: u64,
}

impl VoiceSlot {
    fn new() -> Self {
        Self {
            sender_id: None,
            samples: VecDeque::with_capacity(SAMPLE_RATE * VOICE_RING_MS / 1000),
            first_seq: 0,
            next_seq: 0,
        }
    }

    fn clear_for(&mut self, sender_id: u32) {
        self.sender_id = Some(sender_id);
        self.samples.clear();
        self.first_seq = self.next_seq;
    }

    fn release(&mut self) {
        self.sender_id = None;
        self.samples.clear();
        self.first_seq = self.next_seq;
    }

    fn push(&mut self, sample: i16) {
        let capacity = SAMPLE_RATE * VOICE_RING_MS / 1000;
        if self.samples.len() >= capacity {
            self.samples.pop_front();
            self.first_seq = self.first_seq.wrapping_add(1);
        }
        self.samples.push_back(sample);
        self.next_seq = self.next_seq.wrapping_add(1);
    }

    fn start_cursor(&self, preroll_frames: usize) -> u64 {
        self.next_seq
            .saturating_sub(preroll_frames as u64)
            .max(self.first_seq)
    }

    fn read_since(&self, cursor: u64, out: &mut Vec<i16>, max_samples: usize) -> u64 {
        let mut next = cursor.max(self.first_seq).min(self.next_seq);
        let take = core::cmp::min(
            self.next_seq.saturating_sub(next) as usize,
            max_samples.min(self.samples.len()),
        );
        for _ in 0..take {
            let idx = next.saturating_sub(self.first_seq) as usize;
            let Some(sample) = self.samples.get(idx) else {
                break;
            };
            out.push(*sample);
            next = next.wrapping_add(1);
        }
        next
    }
}

struct VoiceMixer {
    slots: [VoiceSlot; MAX_VOICE_SENDERS],
}

impl VoiceMixer {
    fn new() -> Self {
        Self {
            slots: core::array::from_fn(|_| VoiceSlot::new()),
        }
    }

    fn used(&self) -> usize {
        self.slots
            .iter()
            .filter(|slot| slot.sender_id.is_some())
            .count()
    }

    fn claim(&mut self, sender_id: u32) -> Option<usize> {
        if self
            .slots
            .iter()
            .any(|slot| slot.sender_id == Some(sender_id))
        {
            return None;
        }
        let idx = self
            .slots
            .iter()
            .position(|slot| slot.sender_id.is_none())?;
        self.slots[idx].clear_for(sender_id);
        Some(idx)
    }

    fn release(&mut self, slot_idx: usize, sender_id: Option<u32>) {
        let Some(slot) = self.slots.get_mut(slot_idx) else {
            return;
        };
        if slot.sender_id == sender_id {
            slot.release();
        }
    }

    fn start_cursors(&self) -> [u64; MAX_VOICE_SENDERS] {
        let preroll_frames = SAMPLE_RATE * PREROLL_MS / 1000;
        core::array::from_fn(|idx| self.slots[idx].start_cursor(preroll_frames))
    }

    fn push_pcm(&mut self, slot_idx: usize, bytes: &[u8], input_rate: u32, accum: &mut u64) {
        let Some(slot) = self.slots.get_mut(slot_idx) else {
            return;
        };
        let input_rate = u64::from(input_rate.max(1));
        for pair in bytes.chunks_exact(2) {
            let sample = i16::from_le_bytes([pair[0], pair[1]]);
            *accum = accum.saturating_add(SAMPLE_RATE as u64);
            while *accum >= input_rate {
                slot.push(sample);
                *accum -= input_rate;
            }
        }
    }
}

fn audio_http_open_endpoint(dev_idx: usize) -> Option<AudioHttpEndpoint> {
    let usable = crate::net::adapter::ipv4_at(dev_idx).is_some()
        || crate::net::link_state_at(dev_idx)
            .map(|state| state.up)
            .unwrap_or(false);
    if !usable {
        return None;
    }

    let vnet = VNet::open(dev_idx)?;
    if vnet
        .submit(api::Command::OpenTcpListen {
            port: ports::TINYAUDIO_LIVE_HTTP_TCP_PORT,
        })
        .is_err()
    {
        crate::log!(
            "tinyaudio-live-http: listen submit failed dev={} owner={}\n",
            dev_idx,
            vnet.owner()
        );
        return None;
    }

    let ip = crate::net::adapter::ipv4_at(dev_idx);
    let name = crate::net::device_name_at(dev_idx).unwrap_or("?");
    match ip {
        Some([a, b, c, d]) => crate::log!(
            "tinyaudio-live-http: listen submitted tcp {} owner={} dev={} {} ip={}.{}.{}.{}\n",
            ports::TINYAUDIO_LIVE_HTTP_TCP_PORT,
            vnet.owner(),
            dev_idx,
            name,
            a,
            b,
            c,
            d
        ),
        None => crate::log!(
            "tinyaudio-live-http: listen submitted tcp {} owner={} dev={} {} ip=none\n",
            ports::TINYAUDIO_LIVE_HTTP_TCP_PORT,
            vnet.owner(),
            dev_idx,
            name
        ),
    }

    Some(AudioHttpEndpoint {
        vnet,
        listener: None,
        listener_ready: false,
        dev_idx,
        sessions: Vec::new(),
    })
}

fn audio_http_add_endpoints(endpoints: &mut Vec<AudioHttpEndpoint>) -> usize {
    let mut added = 0usize;
    for dev_idx in 0..crate::net::device_count() {
        if endpoints.iter().any(|endpoint| endpoint.dev_idx == dev_idx) {
            continue;
        }
        if let Some(endpoint) = audio_http_open_endpoint(dev_idx) {
            endpoints.push(endpoint);
            added += 1;
        }
    }
    added
}

fn find_http_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n").map(|p| p + 4)
}

fn path_only(target: &str) -> &str {
    target
        .split_once('?')
        .map(|(path, _)| path)
        .unwrap_or(target)
}

fn http_request_target(req: &str) -> Option<&str> {
    let line_end = crate::r::pat::find_str(req, "\r\n")
        .or_else(|| req.find('\n'))
        .unwrap_or(req.len());
    let line = req.get(..line_end)?;
    let mut it = line.split_whitespace();
    if it.next()? != "GET" {
        return None;
    }
    it.next()
}

fn query_value<'a>(target: &'a str, key: &str) -> Option<&'a str> {
    let (_, query) = target.split_once('?')?;
    query.split('&').find_map(|part| {
        let (name, value) = part.split_once('=').unwrap_or((part, ""));
        (name == key).then_some(value)
    })
}

fn sender_id_from_target(target: &str) -> Option<u32> {
    let value = query_value(target, "id")?;
    u32::from_str_radix(value, 16).ok().filter(|id| *id != 0)
}

fn sender_rate_from_target(target: &str) -> u32 {
    query_value(target, "rate")
        .and_then(|value| value.parse::<u32>().ok())
        .filter(|rate| (8_000..=96_000).contains(rate))
        .unwrap_or(SAMPLE_RATE as u32)
}

fn http_header_value<'a>(req: &'a str, key: &str) -> Option<&'a str> {
    let mut lines = req.split('\n');
    let _ = lines.next()?;
    for line in lines {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':')
            && name.trim().eq_ignore_ascii_case(key)
        {
            return Some(value.trim());
        }
    }
    None
}

fn is_valid_ws_upgrade(req: &str) -> bool {
    http_header_value(req, "Upgrade")
        .map(|value| value.eq_ignore_ascii_case("websocket"))
        .unwrap_or(false)
        && http_header_value(req, "Connection")
            .map(|value| {
                value
                    .split(',')
                    .any(|part| part.trim().eq_ignore_ascii_case("Upgrade"))
            })
            .unwrap_or(false)
}

fn send_tcp_bytes(vnet: &VNet, handle: api::NetHandle, bytes: &[u8]) -> bool {
    for chunk in bytes.chunks(api::MAX_MSG) {
        if vnet
            .submit(api::Command::SendTcp {
                handle,
                data: api::ByteBuf::from_slice_trunc(chunk),
            })
            .is_err()
        {
            return false;
        }
    }
    true
}

fn close_session(vnet: &VNet, handle: api::NetHandle) {
    let _ = vnet.submit(api::Command::Close { handle });
}

fn send_fixed_response(vnet: &VNet, session: &mut AudioHttpSession, bytes: &[u8]) -> bool {
    if !send_tcp_bytes(vnet, session.handle, bytes) {
        close_session(vnet, session.handle);
        return false;
    }
    session.mode = SessionMode::SendingFixed;
    session.pending_bytes = bytes.len();
    session.sent_bytes = 0;
    session.refresh_fixed_deadline();
    true
}

fn response_with_body(status: &str, content_type: &str, body: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(status.as_bytes());
    out.extend_from_slice(b"Content-Type: ");
    out.extend_from_slice(content_type.as_bytes());
    out.extend_from_slice(b"\r\nCache-Control: no-store\r\nContent-Length: ");
    out.extend_from_slice(format!("{}", body.len()).as_bytes());
    out.extend_from_slice(b"\r\nConnection: close\r\n\r\n");
    out.extend_from_slice(body);
    out
}

fn live_audio_page() -> Vec<u8> {
    response_with_body(
        "HTTP/1.1 200 OK\r\n",
        "text/html; charset=utf-8",
        br#"<!doctype html>
<html><head><meta charset="utf-8"><title>TRUEOS Live Audio</title>
<meta name="viewport" content="width=device-width,initial-scale=1">
<style>
body{font-family:system-ui,sans-serif;margin:2rem;max-width:42rem;color:#181818}
.controls{display:flex;gap:.65rem;flex-wrap:wrap}button{font:inherit;padding:.55rem .85rem}
audio{width:100%;margin-top:1rem}output{display:block;margin-top:.65rem;color:#555}
#slots{font-weight:650;color:#181818}.hint{font-size:.9rem;color:#666;line-height:1.4}
</style></head><body><h1>TRUEOS Live Audio</h1>
<div class="controls"><button id="start" type="button">Start listening</button><button id="mic" type="button">Unmute microphone</button></div>
<audio id="audio" controls preload="none"></audio>
<output id="status">idle</output><output id="slots">0/3 voice slots used</output>
<p class="hint" id="micHint">Microphone audio is mixed for the other listeners; your own sender ID is left out of your stream.</p>
<script>
const audio=document.getElementById('audio'),statusEl=document.getElementById('status'),slotsEl=document.getElementById('slots'),micButton=document.getElementById('mic'),hint=document.getElementById('micHint');
const id=(()=>{const a=new Uint32Array(1);if(self.crypto&&crypto.getRandomValues)crypto.getRandomValues(a);else a[0]=(Math.random()*0xffffffff)>>>0;return (a[0]||1).toString(16)})();
let ws=null,micStream=null,ctx=null,source=null,processor=null,muteGain=null,closing=false;
function setStatus(x){statusEl.textContent=x}
function startListening(){if(!audio.src)audio.src='/audio/live.wav?id='+id+'&t='+Date.now();return audio.play().then(()=>setStatus('listening')).catch(e=>setStatus(e&&e.name?e.name:'playback blocked'))}
document.getElementById('start').onclick=startListening;
audio.onplaying=()=>setStatus(ws?'listening + microphone live':'listening');audio.onwaiting=()=>setStatus('buffering');audio.onerror=()=>setStatus('playback error');
async function updateSlots(){try{const r=await fetch('/audio/status?t='+Date.now(),{cache:'no-store'});if(!r.ok)return;const v=await r.json();slotsEl.textContent=v.used+'/'+v.total+' voice slots used'}catch(_e){}}
setInterval(updateSlots,1500);updateSlots();
function stopMic(message,closeSocket=true){closing=true;if(processor){processor.disconnect();processor.onaudioprocess=null}if(source)source.disconnect();if(muteGain)muteGain.disconnect();if(micStream)micStream.getTracks().forEach(t=>t.stop());if(ctx)ctx.close();if(closeSocket&&ws)ws.close();ws=null;micStream=null;ctx=null;source=null;processor=null;muteGain=null;micButton.textContent='Unmute microphone';closing=false;if(message)setStatus(message);updateSlots()}
async function unmute(){
 if(ws){stopMic(audio.paused?'microphone muted':'listening');return}
 if(!navigator.mediaDevices||!navigator.mediaDevices.getUserMedia){hint.textContent='Microphone capture needs HTTPS (or localhost) in current browsers.';setStatus('microphone unavailable on this HTTP origin');return}
 micButton.disabled=true;
 try{
  micStream=await navigator.mediaDevices.getUserMedia({audio:{channelCount:1,echoCancellation:true,noiseSuppression:true,autoGainControl:true}});
  const AC=self.AudioContext||self.webkitAudioContext;ctx=new AC({sampleRate:48000});await ctx.resume();
  const scheme=location.protocol==='https:'?'wss://':'ws://';ws=new WebSocket(scheme+location.host+'/audio/send?id='+id+'&rate='+ctx.sampleRate);ws.binaryType='arraybuffer';
  ws.onopen=()=>{
   source=ctx.createMediaStreamSource(micStream);processor=ctx.createScriptProcessor(1024,1,1);muteGain=ctx.createGain();muteGain.gain.value=0;
   processor.onaudioprocess=e=>{if(!ws||ws.readyState!==WebSocket.OPEN||ws.bufferedAmount>65536)return;const input=e.inputBuffer.getChannelData(0),pcm=new Int16Array(input.length);for(let i=0;i<input.length;i++){const x=Math.max(-1,Math.min(1,input[i]));pcm[i]=x<0?x*32768:x*32767}ws.send(pcm.buffer)};
   source.connect(processor);processor.connect(muteGain);muteGain.connect(ctx.destination);micButton.textContent='Mute microphone';setStatus(audio.paused?'microphone live':'listening + microphone live');startListening();updateSlots();
  };
  ws.onerror=()=>setStatus('voice slot unavailable');
  ws.onclose=()=>{if(!closing)stopMic(audio.paused?'microphone disconnected':'listening',false)};
 }catch(e){stopMic(e&&e.name?e.name:'microphone failed')}
 finally{micButton.disabled=false}
}
micButton.onclick=unmute;window.addEventListener('beforeunload',()=>stopMic('',true));
</script></body></html>"#,
    )
}

fn not_found_response() -> Vec<u8> {
    response_with_body("HTTP/1.1 404 Not Found\r\n", "text/plain; charset=utf-8", b"not found\n")
}

fn busy_response() -> Vec<u8> {
    response_with_body(
        "HTTP/1.1 503 Service Unavailable\r\n",
        "text/plain; charset=utf-8",
        b"too many live audio clients\n",
    )
}

fn voice_busy_response() -> Vec<u8> {
    response_with_body(
        "HTTP/1.1 503 Service Unavailable\r\n",
        "text/plain; charset=utf-8",
        b"all three voice slots are in use\n",
    )
}

fn bad_request_response() -> Vec<u8> {
    response_with_body(
        "HTTP/1.1 400 Bad Request\r\n",
        "text/plain; charset=utf-8",
        b"invalid voice websocket request\n",
    )
}

fn status_response(used: usize) -> Vec<u8> {
    let body = format!("{{\"used\":{},\"total\":{}}}\n", used, MAX_VOICE_SENDERS);
    response_with_body("HTTP/1.1 200 OK\r\n", "application/json; charset=utf-8", body.as_bytes())
}

fn wav_stream_head() -> Vec<u8> {
    let mut head = Vec::new();
    head.extend_from_slice(b"HTTP/1.1 200 OK\r\n");
    head.extend_from_slice(b"Content-Type: audio/wav\r\n");
    head.extend_from_slice(b"Cache-Control: no-store\r\n");
    head.extend_from_slice(b"Connection: close\r\n\r\n");
    head.extend_from_slice(crate::tst::esynth::live_wav_stream_header().as_slice());
    head
}

fn send_ws_reply(
    vnet: &VNet,
    session: &mut AudioHttpSession,
    msg_type: WebSocketSendMessageType,
    payload: &[u8],
) -> Result<(), ()> {
    let mut frame_buf = [0u8; WS_TX_BUF_MAX];
    let len = session
        .ws
        .write(msg_type, true, payload, &mut frame_buf)
        .map_err(|_| ())?;
    if send_tcp_bytes(vnet, session.handle, &frame_buf[..len]) {
        Ok(())
    } else {
        Err(())
    }
}

fn try_open_voice_websocket(
    vnet: &VNet,
    session: &mut AudioHttpSession,
    mixer: &mut VoiceMixer,
    req: &str,
    target: &str,
    header_end: usize,
) -> bool {
    let sender_id = sender_id_from_target(target);
    let key = http_header_value(req, "Sec-WebSocket-Key");
    if !is_valid_ws_upgrade(req) || sender_id.is_none() || key.is_none() {
        let response = bad_request_response();
        return !send_fixed_response(vnet, session, response.as_slice());
    }

    let sender_id = sender_id.unwrap_or(0);
    let Some(slot_idx) = mixer.claim(sender_id) else {
        let response = voice_busy_response();
        return !send_fixed_response(vnet, session, response.as_slice());
    };

    let Ok(key) = WebSocketKey::try_from(key.unwrap_or("")) else {
        mixer.release(slot_idx, Some(sender_id));
        let response = bad_request_response();
        return !send_fixed_response(vnet, session, response.as_slice());
    };
    let mut response = [0u8; WS_TX_BUF_MAX];
    let Ok(len) = session.ws.server_accept(&key, None, &mut response) else {
        mixer.release(slot_idx, Some(sender_id));
        close_session(vnet, session.handle);
        return true;
    };
    if !send_tcp_bytes(vnet, session.handle, &response[..len]) {
        mixer.release(slot_idx, Some(sender_id));
        close_session(vnet, session.handle);
        return true;
    }

    let remaining = session.rx.split_off(header_end);
    session.rx = remaining;
    session.mode = SessionMode::VoiceSending;
    session.voice_slot = Some(slot_idx);
    session.sender_id = Some(sender_id);
    session.sender_rate = sender_rate_from_target(target);
    session.resample_accum = 0;
    session.refresh_voice_deadline();
    session.pending_bytes = 0;
    session.sent_bytes = 0;
    let opened = format!(
        "{{\"kind\":\"slot\",\"slot\":{},\"used\":{},\"total\":{}}}",
        slot_idx + 1,
        mixer.used(),
        MAX_VOICE_SENDERS
    );
    let _ = send_ws_reply(vnet, session, WebSocketSendMessageType::Text, opened.as_bytes());
    crate::log!(
        "tinyaudio-live-http: voice opened handle={} id={:08x} slot={} rate={} used={}/{}\n",
        session.handle.0,
        sender_id,
        slot_idx + 1,
        session.sender_rate,
        mixer.used(),
        MAX_VOICE_SENDERS
    );
    false
}

fn handle_request(
    vnet: &VNet,
    session: &mut AudioHttpSession,
    active_streams: usize,
    mixer: &mut VoiceMixer,
) -> bool {
    let Some(header_end) = find_http_header_end(session.rx.as_slice()) else {
        return false;
    };

    let header = session.rx[..header_end].to_vec();
    let Ok(req) = core::str::from_utf8(header.as_slice()) else {
        close_session(vnet, session.handle);
        return true;
    };

    let target = http_request_target(req);
    let path = target.map(path_only);
    crate::log!(
        "tinyaudio-live-http: request handle={} path={:?} bytes={}\n",
        session.handle.0,
        path,
        header_end
    );

    match path {
        Some(PAGE_PATH) | Some(LEGACY_PAGE_PATH) => {
            let page = live_audio_page();
            !send_fixed_response(vnet, session, page.as_slice())
        }
        Some(STATUS_PATH) => {
            let response = status_response(mixer.used());
            !send_fixed_response(vnet, session, response.as_slice())
        }
        Some(WAV_PATH) => {
            if active_streams >= MAX_STREAM_CLIENTS {
                let response = busy_response();
                return !send_fixed_response(vnet, session, response.as_slice());
            }
            let preroll_samples = SAMPLE_RATE * CHANNELS * PREROLL_MS / 1000;
            session.cursor =
                crate::tst::esynth::live_pcm_stream_start_cursor(preroll_samples).unwrap_or(0);
            session.voice_cursors = mixer.start_cursors();
            session.exclude_sender_id = target.and_then(sender_id_from_target);
            let head = wav_stream_head();
            if !send_tcp_bytes(vnet, session.handle, head.as_slice()) {
                close_session(vnet, session.handle);
                return true;
            }
            session.mode = SessionMode::Streaming;
            session.rx.clear();
            session.pending_bytes = 0;
            session.sent_bytes = 0;
            session.stream_chunks = 0;
            session.stream_samples = 0;
            crate::log!(
                "tinyaudio-live-http: stream opened handle={} exclude={:?}\n",
                session.handle.0,
                session.exclude_sender_id
            );
            false
        }
        Some(SEND_PATH) => try_open_voice_websocket(
            vnet,
            session,
            mixer,
            req,
            target.unwrap_or(SEND_PATH),
            header_end,
        ),
        _ => {
            let response = not_found_response();
            !send_fixed_response(vnet, session, response.as_slice())
        }
    }
}

fn handle_voice_frames(
    vnet: &VNet,
    session: &mut AudioHttpSession,
    mixer: &mut VoiceMixer,
) -> bool {
    let mut payload = [0u8; WS_FRAME_BUF_MAX];
    loop {
        let frame = match session.ws.read(session.rx.as_slice(), &mut payload) {
            Ok(frame) => frame,
            Err(embedded_websocket::Error::ReadFrameIncomplete) => break,
            Err(err) => {
                crate::log!(
                    "tinyaudio-live-http: voice frame failed handle={} err={:?}\n",
                    session.handle.0,
                    err
                );
                close_session(vnet, session.handle);
                return true;
            }
        };
        if frame.len_from == 0 {
            break;
        }
        let remaining = session.rx.split_off(frame.len_from);
        session.rx = remaining;

        match frame.message_type {
            WebSocketReceiveMessageType::Binary => {
                session.refresh_voice_deadline();
                if let Some(slot_idx) = session.voice_slot {
                    mixer.push_pcm(
                        slot_idx,
                        &payload[..frame.len_to & !1],
                        session.sender_rate,
                        &mut session.resample_accum,
                    );
                }
            }
            WebSocketReceiveMessageType::Ping => {
                let _ = send_ws_reply(
                    vnet,
                    session,
                    WebSocketSendMessageType::Pong,
                    &payload[..frame.len_to],
                );
            }
            WebSocketReceiveMessageType::CloseMustReply => {
                let _ = send_ws_reply(
                    vnet,
                    session,
                    WebSocketSendMessageType::CloseReply,
                    &payload[..frame.len_to],
                );
                close_session(vnet, session.handle);
                return true;
            }
            WebSocketReceiveMessageType::CloseCompleted => {
                close_session(vnet, session.handle);
                return true;
            }
            WebSocketReceiveMessageType::Text | WebSocketReceiveMessageType::Pong => {}
        }
    }
    false
}

fn stream_audio_tick(vnet: &VNet, session: &mut AudioHttpSession, mixer: &VoiceMixer) -> bool {
    let max_samples = SAMPLE_RATE * CHANNELS * SEND_MS / 1000;
    let mut samples = Vec::with_capacity(max_samples);

    let Some(next) =
        crate::tst::esynth::live_pcm_read_since(session.cursor, &mut samples, max_samples)
    else {
        return false;
    };
    session.cursor = next;

    if samples.is_empty() {
        return false;
    }

    let frames = samples.len() / CHANNELS;
    let mut voice = Vec::with_capacity(frames);
    for slot_idx in 0..MAX_VOICE_SENDERS {
        let slot = &mixer.slots[slot_idx];
        if slot.sender_id.is_none() || slot.sender_id == session.exclude_sender_id {
            continue;
        }
        voice.clear();
        session.voice_cursors[slot_idx] =
            slot.read_since(session.voice_cursors[slot_idx], &mut voice, frames);
        for (frame_idx, voice_sample) in voice.iter().copied().enumerate() {
            let contribution = i32::from(voice_sample) / 2;
            for channel in 0..CHANNELS {
                let idx = frame_idx * CHANNELS + channel;
                let mixed = i32::from(samples[idx]) + contribution;
                samples[idx] = mixed.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
            }
        }
    }

    let mut bytes = Vec::with_capacity(samples.len() * core::mem::size_of::<i16>());
    for sample in samples.iter().copied() {
        bytes.extend_from_slice(&sample.to_le_bytes());
    }

    if !send_tcp_bytes(vnet, session.handle, bytes.as_slice()) {
        close_session(vnet, session.handle);
        return true;
    }
    session.pending_bytes = session.pending_bytes.saturating_add(bytes.len());

    session.stream_chunks = session.stream_chunks.saturating_add(1);
    session.stream_samples = session.stream_samples.saturating_add(samples.len());
    if session.stream_chunks <= 4 || session.stream_chunks.is_multiple_of(100) {
        crate::log!(
            "tinyaudio-live-http: stream chunk handle={} chunk={} samples={} total_samples={} bytes={}\n",
            session.handle.0,
            session.stream_chunks,
            samples.len(),
            session.stream_samples,
            bytes.len()
        );
    }

    false
}

fn active_stream_count(endpoint: &AudioHttpEndpoint) -> usize {
    endpoint
        .sessions
        .iter()
        .filter(|session| session.mode == SessionMode::Streaming)
        .count()
}

fn release_voice_session(mixer: &mut VoiceMixer, session: &AudioHttpSession) {
    let Some(slot_idx) = session.voice_slot else {
        return;
    };
    mixer.release(slot_idx, session.sender_id);
    crate::log!(
        "tinyaudio-live-http: voice released handle={} id={:?} slot={} used={}/{}\n",
        session.handle.0,
        session.sender_id,
        slot_idx + 1,
        mixer.used(),
        MAX_VOICE_SENDERS
    );
}

fn remove_session(endpoint: &mut AudioHttpEndpoint, idx: usize, mixer: &mut VoiceMixer) {
    let session = endpoint.sessions.remove(idx);
    release_voice_session(mixer, &session);
}

fn clear_sessions(endpoint: &mut AudioHttpEndpoint, mixer: &mut VoiceMixer) {
    for session in endpoint.sessions.drain(..) {
        release_voice_session(mixer, &session);
    }
}

fn reopen_listener(endpoint: &mut AudioHttpEndpoint) {
    endpoint.listener = None;
    endpoint.listener_ready = false;
    let _ = endpoint.vnet.submit(api::Command::OpenTcpListen {
        port: ports::TINYAUDIO_LIVE_HTTP_TCP_PORT,
    });
}

fn close_oldest_reading_request(endpoint: &mut AudioHttpEndpoint) -> bool {
    let Some(idx) = endpoint
        .sessions
        .iter()
        .position(|session| session.mode == SessionMode::ReadingRequest)
    else {
        return false;
    };

    let handle = endpoint.sessions[idx].handle;
    crate::log!(
        "tinyaudio-live-http: idle preconnect close dev={} handle={} active={}\n",
        endpoint.dev_idx,
        handle.0,
        endpoint.sessions.len()
    );
    close_session(&endpoint.vnet, handle);
    endpoint.sessions.remove(idx);
    true
}

fn prune_idle_sessions(endpoint: &mut AudioHttpEndpoint, mixer: &mut VoiceMixer) {
    let now = Instant::now();
    let mut idx = 0usize;
    while idx < endpoint.sessions.len() {
        if endpoint.sessions[idx].is_timed_out(now) {
            let handle = endpoint.sessions[idx].handle;
            crate::log!(
                "tinyaudio-live-http: timeout close dev={} handle={} mode={:?}\n",
                endpoint.dev_idx,
                handle.0,
                endpoint.sessions[idx].mode
            );
            close_session(&endpoint.vnet, handle);
            remove_session(endpoint, idx, mixer);
        } else {
            idx += 1;
        }
    }
}

#[embassy_executor::task]
pub async fn tinyaudio_live_http_task() {
    let mut endpoints: Vec<AudioHttpEndpoint> = Vec::new();
    let mut mixer = VoiceMixer::new();
    loop {
        audio_http_add_endpoints(&mut endpoints);
        if !endpoints.is_empty() {
            break;
        }
        crate::log!("tinyaudio-live-http: waiting for a usable NIC\n");
        Timer::after(Duration::from_millis(250)).await;
    }

    let mut endpoint_discovery_ticks = 0u32;
    loop {
        if endpoint_discovery_ticks == 0 {
            audio_http_add_endpoints(&mut endpoints);
        }
        endpoint_discovery_ticks = (endpoint_discovery_ticks + 1) % 100;

        for endpoint in endpoints.iter_mut() {
            prune_idle_sessions(endpoint, &mut mixer);

            while let Some(ev) = endpoint.vnet.pop_event() {
                match ev {
                    api::Event::Opened { handle, kind } => {
                        if kind == api::SocketKind::Tcp {
                            endpoint.listener = Some(handle);
                            endpoint.listener_ready = true;
                            crate::log!(
                                "tinyaudio-live-http: tcp listen opened dev={} handle={} port={} page={} stream={}\n",
                                endpoint.dev_idx,
                                handle.0,
                                ports::TINYAUDIO_LIVE_HTTP_TCP_PORT,
                                PAGE_PATH,
                                WAV_PATH
                            );
                        }
                    }
                    api::Event::TcpEstablished { handle, .. } => {
                        if endpoint.listener == Some(handle) {
                            reopen_listener(endpoint);
                        }

                        if endpoint
                            .sessions
                            .iter()
                            .any(|session| session.handle == handle)
                        {
                            continue;
                        }

                        if endpoint.sessions.len() >= MAX_SESSIONS {
                            close_oldest_reading_request(endpoint);
                            if endpoint.sessions.len() >= MAX_SESSIONS {
                                crate::log!(
                                    "tinyaudio-live-http: max sessions close dev={} handle={} active={}\n",
                                    endpoint.dev_idx,
                                    handle.0,
                                    endpoint.sessions.len()
                                );
                                close_session(&endpoint.vnet, handle);
                                continue;
                            }
                        }

                        endpoint.sessions.push(AudioHttpSession::new(handle));
                        crate::log!(
                            "tinyaudio-live-http: tcp established dev={} handle={}\n",
                            endpoint.dev_idx,
                            handle.0
                        );
                    }
                    api::Event::TcpData { handle, data } => {
                        if endpoint.listener == Some(handle) {
                            reopen_listener(endpoint);
                        }

                        let idx = match endpoint
                            .sessions
                            .iter()
                            .position(|session| session.handle == handle)
                        {
                            Some(idx) => idx,
                            None => {
                                if endpoint.sessions.len() >= MAX_SESSIONS {
                                    close_oldest_reading_request(endpoint);
                                    if endpoint.sessions.len() >= MAX_SESSIONS {
                                        crate::log!(
                                            "tinyaudio-live-http: max sessions close dev={} handle={} active={}\n",
                                            endpoint.dev_idx,
                                            handle.0,
                                            endpoint.sessions.len()
                                        );
                                        close_session(&endpoint.vnet, handle);
                                        continue;
                                    }
                                }

                                endpoint.sessions.push(AudioHttpSession::new(handle));
                                crate::log!(
                                    "tinyaudio-live-http: tcp data before established dev={} handle={}\n",
                                    endpoint.dev_idx,
                                    handle.0
                                );
                                endpoint.sessions.len() - 1
                            }
                        };
                        let active_streams = active_stream_count(endpoint);
                        let session = &mut endpoint.sessions[idx];
                        match session.mode {
                            SessionMode::ReadingRequest => {
                                if session.rx.len().saturating_add(data.len()) > RX_BUF_MAX {
                                    close_session(&endpoint.vnet, handle);
                                    remove_session(endpoint, idx, &mut mixer);
                                    continue;
                                }
                                session.rx.extend_from_slice(data.as_slice());
                                session.refresh_request_deadline();
                                let mut closed = handle_request(
                                    &endpoint.vnet,
                                    session,
                                    active_streams,
                                    &mut mixer,
                                );
                                if !closed && session.mode == SessionMode::VoiceSending {
                                    closed =
                                        handle_voice_frames(&endpoint.vnet, session, &mut mixer);
                                }
                                if closed {
                                    remove_session(endpoint, idx, &mut mixer);
                                }
                            }
                            SessionMode::VoiceSending => {
                                if session.rx.len().saturating_add(data.len()) > VOICE_RX_BUF_MAX {
                                    close_session(&endpoint.vnet, handle);
                                    remove_session(endpoint, idx, &mut mixer);
                                    continue;
                                }
                                session.rx.extend_from_slice(data.as_slice());
                                if handle_voice_frames(&endpoint.vnet, session, &mut mixer) {
                                    remove_session(endpoint, idx, &mut mixer);
                                }
                            }
                            SessionMode::SendingFixed | SessionMode::Streaming => {}
                        }
                    }
                    api::Event::Closed { handle } => {
                        if let Some(idx) = endpoint
                            .sessions
                            .iter()
                            .position(|session| session.handle == handle)
                        {
                            remove_session(endpoint, idx, &mut mixer);
                        } else if Some(handle) == endpoint.listener {
                            clear_sessions(endpoint, &mut mixer);
                            reopen_listener(endpoint);
                        }
                    }
                    api::Event::Error { msg } => {
                        if msg != "bad handle" {
                            crate::log!(
                                "tinyaudio-live-http: error dev={} {}\n",
                                endpoint.dev_idx,
                                msg
                            );
                        }
                    }
                    api::Event::TcpSent { handle, len } => {
                        if let Some(session) = endpoint
                            .sessions
                            .iter_mut()
                            .find(|session| session.handle == handle)
                        {
                            session.sent_bytes = session.sent_bytes.saturating_add(len as usize);
                            if session.mode == SessionMode::SendingFixed
                                && session.sent_bytes >= session.pending_bytes
                            {
                                close_session(&endpoint.vnet, handle);
                            }
                        }
                    }
                    api::Event::UdpPacket { .. }
                    | api::Event::UdpPacketV6 { .. }
                    | api::Event::IpPacket { .. }
                    | api::Event::IcmpReply { .. }
                    | api::Event::IcmpReplyV6 { .. } => {}
                }
            }

            let mut idx = 0usize;
            while idx < endpoint.sessions.len() {
                if endpoint.sessions[idx].mode == SessionMode::Streaming
                    && stream_audio_tick(&endpoint.vnet, &mut endpoint.sessions[idx], &mixer)
                {
                    remove_session(endpoint, idx, &mut mixer);
                } else {
                    idx += 1;
                }
            }
        }

        Timer::after(Duration::from_millis(POLL_MS)).await;
    }
}
