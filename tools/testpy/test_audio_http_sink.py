#!/usr/bin/env python3
"""Exercise production sink admission and HTTP playback with a recording TCP stub."""
from pathlib import Path
import re
import subprocess
import tempfile
from test_blueprint_memory_profile import item

ROOT = Path(__file__).resolve().parents[2]


def main():
    source = (ROOT / 'src/r/services/audio_http.rs').read_text()
    markers = [
        'enum SessionMode', 'struct AudioHttpSession', 'impl AudioHttpSession',
        'fn find_http_header_end', 'fn path_only', 'fn http_request_target',
        'fn send_tcp_bytes', 'fn close_session', 'fn send_fixed_response',
        'fn response_with_body', 'fn live_audio_page', 'fn not_found_response',
        'fn busy_response', 'fn wav_stream_head', 'fn handle_request',
        'fn append_wav_header', 'fn clip_request_id', 'fn start_clip_response', 'fn send_clip_tick',
        'fn status_response', 'fn render_audio_block', 'fn stream_audio_tick', 'struct ClipTransfer',
    ]
    bodies = []
    for marker in markers:
        body = item(source, marker)
        if marker == 'enum SessionMode':
            body = '#[derive(Clone, Copy, Debug, PartialEq, Eq)]\n' + body
        body = body.replace('include_bytes!("audio_http/player.html")',
                            'include_bytes!(r"' + str(ROOT/'src/r/services/audio_http/player.html') + '")')
        bodies.append(body)
    constants = '\n'.join(re.findall(r'^const .+;$', source, re.M))
    harness = r'''
#![allow(dead_code)]
extern crate alloc;
use alloc::{format, vec::Vec};
use std::cell::{RefCell, Cell};
#[macro_export] macro_rules! log { ($($t:tt)*) => {}; }
mod r { pub mod pat { pub fn find_str(s:&str, n:&str)->Option<usize> { s.find(n) } } }
#[derive(Clone, Copy, PartialEq, PartialOrd)] struct Instant(u64);
struct Duration(u64);
impl Instant { fn now()->Self { Self(0) } }
impl Duration { fn from_millis(n:u64)->Self { Self(n) } }
impl core::ops::Add<Duration> for Instant {
    type Output=Self; fn add(self, d:Duration)->Self { Self(self.0+d.0) }
}
mod api {
    pub const MAX_MSG:usize=1024;
    #[derive(Clone,Copy)] pub struct NetHandle(pub u32);
    pub struct ByteBuf(pub Vec<u8>);
    impl ByteBuf { pub fn from_slice_trunc(b:&[u8])->Self { Self(b[..b.len().min(MAX_MSG)].to_vec()) } }
    pub enum Command { SendTcp { handle:NetHandle, data:ByteBuf }, Close { handle:NetHandle } }
}
#[derive(Default)] struct VNet { bytes:RefCell<Vec<u8>>, closed:RefCell<bool>, fail_next:Cell<bool> }
impl VNet {
    fn submit(&self,c:api::Command)->Result<(),()> {
        match c {
            api::Command::SendTcp {data,..} => {
                if self.fail_next.replace(false) { return Err(()); }
                self.bytes.borrow_mut().extend(data.0)
            },
            api::Command::Close {..} => *self.closed.borrow_mut()=true,
        }; Ok(())
    }
}
@CONSTANTS@
@PRODUCTION@
#[path="@SINK@"] mod sink;
use sink::Sink;
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    const fn new(value:T)->Self {Self(std::sync::Mutex::new(value))}
    fn lock(&self)->std::sync::MutexGuard<'_,T> {self.0.lock().unwrap()}
}
static SINK:Mutex<Sink>=Mutex::new(Sink::new());
fn request(path:&str)->(VNet,AudioHttpSession) {
    let net=VNet::default(); let mut s=AudioHttpSession::new(api::NetHandle(1));
    s.rx=format!("GET {path} HTTP/1.1\r\nHost: trueos\r\n\r\n").into_bytes();
    assert!(!handle_request(&net,&mut s,0)); (net,s)
}
#[test] fn listeners_receive_pcm_wav_without_microphone_or_upload_routes() {
    let (net,s)=request("/audio/live.wav");
    assert_eq!(s.mode,SessionMode::Streaming);
    let bytes=net.bytes.borrow(); let start=find_http_header_end(&bytes).unwrap();
    let wav=&bytes[start..];
    assert_eq!(&wav[..4],b"RIFF"); assert_eq!(&wav[8..16],b"WAVEfmt ");
    assert_eq!(u16::from_le_bytes(wav[22..24].try_into().unwrap()),2);
    assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()),48_000);
    assert_eq!(u16::from_le_bytes(wav[34..36].try_into().unwrap()),16);
    let (net,s)=request("/audio/send?id=42&rate=48000");
    assert_eq!(s.mode,SessionMode::SendingFixed);
    assert!(net.bytes.borrow().starts_with(b"HTTP/1.1 404"));
    let page=String::from_utf8(live_audio_page()).unwrap();
    assert!(page.contains("class LivePlayer"));
    for token in ["getUserMedia", "WebSocket", "microphone", "/audio/send"] { assert!(!page.contains(token)); }
}
#[test] fn fragmented_get_waits_and_non_get_cannot_ingest_pcm() {
    let net=VNet::default(); let mut s=AudioHttpSession::new(api::NetHandle(1));
    s.rx=b"GET /audio/live.wav HTTP/1.1\r\n".to_vec();
    assert!(!handle_request(&net,&mut s,0)); assert!(net.bytes.borrow().is_empty());
    s.rx.extend_from_slice(b"\r\n"); assert!(!handle_request(&net,&mut s,0));
    assert_eq!(s.mode,SessionMode::Streaming);
    let net=VNet::default(); let mut s=AudioHttpSession::new(api::NetHandle(2));
    s.rx=b"POST /audio/live.wav HTTP/1.1\r\n\r\nPCM".to_vec();
    assert!(!handle_request(&net,&mut s,0));
    assert!(net.bytes.borrow().starts_with(b"HTTP/1.1 404"));
}
#[test] fn listeners_are_bounded_and_slow_listeners_close() {
    let net=VNet::default(); let mut s=AudioHttpSession::new(api::NetHandle(1));
    s.rx=b"GET /audio/live.wav HTTP/1.1\r\n\r\n".to_vec();
    assert!(!handle_request(&net,&mut s,MAX_STREAM_CLIENTS));
    assert!(net.bytes.borrow().starts_with(b"HTTP/1.1 503"));
    let (net,mut s)=request("/audio/live.wav");
    s.pending_bytes=48_001; s.sent_bytes=0;
    assert!(stream_audio_tick(&net,&mut s,&[0;4])); assert!(*net.closed.borrow());
}
#[test] fn crossfade_endpoint_preserves_two_synchronized_producer_slots() {
    let (net,mut session)=request("/audio/live.pcm");
    assert_eq!(session.mode,SessionMode::StreamingSources);
    assert!(net.bytes.borrow().starts_with(b"HTTP/1.1 200"));
    let header_end=find_http_header_end(&net.bytes.borrow()).unwrap();
    let mut sink=Sink::new();
    sink.submit(&[100,0,200,0]).unwrap();
    sink.submit(&[200,0,100,0,40,0,60,0]).unwrap();
    let mut mixed=[0; BLOCK_BYTES];
    let mut wire=[0; PCM_PREFIX_BYTES+2*BLOCK_BYTES];
    render_audio_block(&mut sink, &mut mixed, &mut wire);
    assert_eq!(&wire[..8], &[1,0,2,0,100,0,200,0]);
    assert_eq!(&wire[PCM_PREFIX_BYTES+BLOCK_BYTES..][..8], &[200,0,100,0,40,0,60,0]);
    assert_eq!(&mixed[..8], &[150,0,150,0,40,0,60,0]);
    assert!(!stream_audio_tick(&net,&mut session,&wire));
    assert_eq!(&net.bytes.borrow()[header_end..], &wire);
    assert_eq!(sink.active_count(),0);
}
#[test] fn finite_wav_starts_at_byte_zero_after_clock_advance_and_snapshot_replacement() {
    *SINK.lock()=Sink::new();
    let pcm:Vec<u8>=(0..192_000*2).map(|i|((i*37+13)%251) as u8).collect();
    SINK.lock().submit(&pcm).unwrap();
    SINK.lock().render(&mut vec![0;192_000]);
    let id=SINK.lock().snapshot().unwrap().id;
    let (net,mut session)=request(&format!("/audio/clip.wav?id={id}"));
    assert_eq!(session.mode,SessionMode::SendingClip);
    // Fail one send before its payload is queued; retry must retain offset0.
    net.fail_next.set(true);
    assert!(!send_clip_tick(&net,&mut session));
    assert_eq!(session.clip.as_ref().unwrap().offset,0);
    SINK.lock().render(&mut vec![0;192_000]);
    SINK.lock().submit(&[9;8]).unwrap();
    loop {
        session.sent_bytes=session.pending_bytes;
        if send_clip_tick(&net,&mut session) {break;}
        assert!(session.pending_bytes-session.sent_bytes<=48_000);
    }
    let bytes=net.bytes.borrow();let start=find_http_header_end(&bytes).unwrap();
    let header=std::str::from_utf8(&bytes[..start]).unwrap();
    assert!(header.contains(&format!("Content-Length: {}\r\n",pcm.len()+44)));
    let wav=&bytes[start..];assert_eq!(wav.len(),pcm.len()+44);
    assert_eq!(u32::from_le_bytes(wav[4..8].try_into().unwrap()) as usize,pcm.len()+36);
    assert_eq!(u32::from_le_bytes(wav[40..44].try_into().unwrap()) as usize,pcm.len());
    assert_eq!(&wav[44..],pcm.as_slice());
    assert!(*net.closed.borrow());
    drop(bytes);
    let (stale,_)=request(&format!("/audio/clip.wav?id={id}"));
    assert!(stale.bytes.borrow().starts_with(b"HTTP/1.1 409"));
    assert_eq!(clip_request_id("/audio/clip.wav?id=1&id=2"),None);
}
'''.replace('@CONSTANTS@',constants).replace('@PRODUCTION@','\n'.join(bodies)).replace('@SINK@',str(ROOT / 'src/r/services/audio_http/sink.rs'))
    with tempfile.TemporaryDirectory(prefix='trueos-audio-http-') as work:
        source_path=Path(work)/'test.rs'; binary=Path(work)/'test'
        source_path.write_text(harness)
        subprocess.run(['rustup','run','nightly-2026-07-10','rustc','--edition=2024','--test',str(source_path),'-o',str(binary)],check=True)
        subprocess.run([str(binary)],check=True)


if __name__ == '__main__':
    main()
