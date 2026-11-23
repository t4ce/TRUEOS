#!/usr/bin/env python3
"""Exercise PCM mixing, HTTP playback, and HDA jack policy on the host."""
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
        'struct AudioHttpEndpoint', 'struct ClosingTcp', 'impl AudioHttpEndpoint',
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
    hda_source = (ROOT / 'src/intel/hda/playback.rs').read_text()
    pin_constants = re.findall(
        r'^const (?:DIGITAL_WIDGET|PIN_PRESENCE_DETECT|PIN_OUTPUT_CAPABLE|JACK_DETECT_OVERRIDE):.+;$',
        hda_source, re.M,
    )
    hda_policy = '\n'.join([
        *pin_constants,
        *(item(hda_source, marker) for marker in ('enum JackKind', 'impl JackKind', 'fn select_jack(')),
    ])
    harness = r'''
#![allow(dead_code, unused_imports)]
extern crate alloc;
extern crate self as spin;
extern crate self as trueos_time;
use alloc::{format, vec::Vec};
use std::cell::{RefCell, Cell};
#[macro_export] macro_rules! log { ($($t:tt)*) => {}; }
#[macro_export] macro_rules! log_info { ($($t:tt)*) => {}; }
mod r { pub mod pat { pub fn find_str(s:&str, n:&str)->Option<usize> { s.find(n) } } }
mod net {
    pub struct Link { pub up: bool }
    pub fn link_state_at(_:usize)->Option<Link> { Some(Link { up: true }) }
}
mod hv {
    pub const TRUEOS_VM_ID_LIMIT: usize = 64;
    pub fn vm_run_generation(_:u8)->Option<u64> { None }
}
mod ports { pub const AUDIO_HTTP_TCP_PORT:u16=8080; }
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)] struct Instant(u64);
struct Duration(u64);
struct Timer;
impl Timer {
    async fn at(_:Instant) {}
}
impl Instant {
    fn now()->Self { Self(0) }
    fn as_millis(self)->u64 { self.0 }
}
impl Duration { const fn from_millis(n:u64)->Self { Self(n) } }
impl core::ops::Add<Duration> for Instant {
    type Output=Self; fn add(self, d:Duration)->Self { Self(self.0+d.0) }
}
impl core::ops::AddAssign<Duration> for Instant {
    fn add_assign(&mut self, duration:Duration) { self.0 += duration.0; }
}
mod api {
    pub const MAX_MSG:usize=1024;
    #[derive(Clone,Copy,Debug,PartialEq,Eq)] pub struct NetHandle(pub u32);
    pub struct ByteBuf(pub Vec<u8>);
    impl ByteBuf { pub fn from_slice_trunc(b:&[u8])->Self { Self(b[..b.len().min(MAX_MSG)].to_vec()) } }
    pub enum Command {
        SendTcp { handle:NetHandle, data:ByteBuf },
        Close { handle:NetHandle },
        OpenTcpListen { port:u16 },
    }
}
#[derive(Default)] struct VNet {
    bytes:RefCell<Vec<u8>>,
    closed:RefCell<bool>,
    closed_handles:RefCell<Vec<api::NetHandle>>,
    fail_next:Cell<bool>,
    listeners:Cell<usize>,
}
impl VNet {
    fn submit(&self,c:api::Command)->Result<(),()> {
        if self.fail_next.replace(false) { return Err(()); }
        match c {
            api::Command::SendTcp {data,..} => {
                self.bytes.borrow_mut().extend(data.0)
            },
            api::Command::Close {handle} => {
                *self.closed.borrow_mut()=true;
                self.closed_handles.borrow_mut().push(handle);
            },
            api::Command::OpenTcpListen {port} => {
                assert_eq!(port,ports::AUDIO_HTTP_TCP_PORT);
                self.listeners.set(self.listeners.get()+1);
            },
        }; Ok(())
    }
    fn open_esp32_rtp_mic(&self)->Result<(),()> { Ok(()) }
}
@CONSTANTS@
@PRODUCTION@
@HDA_POLICY@
#[path="audio_playback.rs"] mod audio_playback;
use audio_playback::{ProducerOwner, OutputBucket};
struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    const fn new(value:T)->Self {Self(std::sync::Mutex::new(value))}
    fn lock(&self)->std::sync::MutexGuard<'_,T> {self.0.lock().unwrap()}
}
fn reset_playback() {
    audio_playback::reset_test_sink();
    audio_playback::set_http_enabled(true);
    audio_playback::set_http_rendering(true);
}
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
    reset_playback();
    audio_playback::submit_test_clip(ProducerOwner::Host, &[100,0,200,0]).unwrap();
    audio_playback::submit_test_clip(ProducerOwner::Host, &[200,0,100,0,40,0,60,0]).unwrap();
    let mut mixed=[0; BLOCK_BYTES];
    let mut wire=[0; PCM_PREFIX_BYTES+2*BLOCK_BYTES];
    render_audio_block(&mut mixed, &mut wire, 0);
    assert_eq!(&wire[..8], &[1,0,2,0,100,0,200,0]);
    assert_eq!(&wire[PCM_PREFIX_BYTES+BLOCK_BYTES..][..8], &[200,0,100,0,40,0,60,0]);
    assert_eq!(&mixed[..8], &[150,0,150,0,40,0,60,0]);
    assert!(!stream_audio_tick(&net,&mut session,&wire));
    assert_eq!(&net.bytes.borrow()[header_end..], &wire);
    assert_eq!(audio_playback::http_status().1, [None, None]);
}

#[test]
fn movie_audio_uses_the_same_http_lanes_and_jack_mix_as_an_ordinary_clip() {
    reset_playback();
    audio_playback::set_local_playback(true);
    let clip=[100,0,200,0];
    let soundtrack=[200,0,100,0];
    audio_playback::submit_test_clip(ProducerOwner::Host, &clip).unwrap();
    let clip_id=audio_playback::http_snapshot().unwrap().id;
    let video=audio_playback::VideoAudioOwner { slot: 0, generation: 1 };
    let pcm=audio_playback::begin_video_pcm(video).unwrap();
    assert_eq!(pcm.write_pcm(&soundtrack), Ok(soundtrack.len()));
    assert_eq!(audio_playback::http_status().1, [Some(clip_id), None]);
    audio_playback::start_video(video);

    let (net,mut session)=request("/audio/live.pcm");
    let header_len=net.bytes.borrow().len();
    let mut mixed=[0;BLOCK_BYTES];
    let mut wire=[0;PCM_PREFIX_BYTES+2*BLOCK_BYTES];
    render_audio_block(&mut mixed,&mut wire,0);
    assert_eq!(&wire[..PCM_PREFIX_BYTES], &[1,0,1,0]);
    assert_eq!(&wire[PCM_PREFIX_BYTES..][..4], &clip);
    assert_eq!(&wire[PCM_PREFIX_BYTES+BLOCK_BYTES..][..4], &soundtrack);
    assert_eq!(&mixed[..4], &[150,0,150,0]);
    assert!(!stream_audio_tick(&net,&mut session,&wire));
    assert_eq!(&net.bytes.borrow()[header_len..], &wire);

    let mut local=[[0i16;2];1];
    audio_playback::render_local(&mut local);
    assert_eq!(local, [[150,150]]);
    assert_eq!(audio_playback::http_snapshot().unwrap().id, clip_id);
}

#[test] fn finite_wav_starts_at_byte_zero_after_clock_advance_and_snapshot_replacement() {
    reset_playback();
    let pcm:Vec<u8>=(0..192_000*2).map(|i|((i*37+13)%251) as u8).collect();
    audio_playback::submit_test_clip(ProducerOwner::Host, &pcm).unwrap();
    let mut mixed=[0;BLOCK_BYTES];
    let mut wire=[0;PCM_PREFIX_BYTES+2*BLOCK_BYTES];
    render_audio_block(&mut mixed, &mut wire, 0);
    let id=audio_playback::http_snapshot().unwrap().id;
    let (net,mut session)=request(&format!("/audio/clip.wav?id={id}"));
    assert_eq!(session.mode,SessionMode::SendingClip);
    // Fail one send before its payload is queued; retry must retain offset0.
    net.fail_next.set(true);
    assert!(!send_clip_tick(&net,&mut session));
    assert_eq!(session.clip.as_ref().unwrap().offset,0);
    render_audio_block(&mut mixed, &mut wire, 10);
    audio_playback::submit_test_clip(ProducerOwner::Host, &[9;8]).unwrap();
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

#[test]
fn removing_http_route_hides_the_clip_and_stops_its_existing_download() {
    reset_playback();
    audio_playback::submit_test_clip(ProducerOwner::Host, &[7; 8]).unwrap();
    let id=audio_playback::http_snapshot().unwrap().id;
    let (net,mut session)=request(&format!("/audio/clip.wav?id={id}"));
    let header_len=net.bytes.borrow().len();
    assert!(audio_playback::toggle_route(id, OutputBucket::Http));
    assert_eq!(audio_playback::http_status(), (None, [None, None]));
    assert!(send_clip_tick(&net,&mut session));
    assert_eq!(net.bytes.borrow().len(), header_len);
    assert!(*net.closed.borrow());
    let (unrouted,_)=request(&format!("/audio/clip.wav?id={id}"));
    assert!(unrouted.bytes.borrow().starts_with(b"HTTP/1.1 409"));
}

#[test]
fn turning_http_off_keeps_local_playback_and_the_http_assignment() {
    reset_playback();
    audio_playback::set_local_playback(true);
    audio_playback::submit_test_clip(ProducerOwner::Host, &[1,0,2,0,3,0,4,0]).unwrap();
    audio_playback::set_http_enabled(false);
    assert_eq!(audio_playback::http_status(), (None, [None, None]));
    assert!(audio_playback::routing_snapshot().sources[0].unwrap().http);

    let mut local=[[0i16;2];1];
    let mut mixed=[9;BLOCK_BYTES];
    let mut wire=[9;PCM_PREFIX_BYTES+2*BLOCK_BYTES];
    render_audio_block(&mut mixed,&mut wire,0);
    assert!(wire.iter().all(|&byte| byte==0));
    assert!(mixed.iter().all(|&byte| byte==0));
    audio_playback::render_local(&mut local);
    assert_eq!(local, [[1,2]]);

    audio_playback::set_http_enabled(true);
    audio_playback::set_http_rendering(true);
    render_audio_block(&mut mixed,&mut wire,10);
    assert_eq!(&mixed[..4], &[3,0,4,0]);
    audio_playback::render_local(&mut local);
    assert_eq!(local, [[3,4]]);
}

#[test]
fn http_shutdown_retries_tcp_closes_before_reopening_and_keeps_the_mic() {
    let listener=api::NetHandle(3);
    let mic=api::NetHandle(4);
    let client=api::NetHandle(5);
    let mut endpoint=AudioHttpEndpoint {
        vnet: VNet::default(),
        listener: Some(listener),
        listener_retry: Instant(0),
        closing_tcp: Vec::new(),
        rtp_mic: Some(mic),
        rtp_mic_retry: Instant(0),
        dev_idx: 0,
        sessions: vec![AudioHttpSession::new(client)],
    };

    endpoint.stop_http(Instant(1));
    endpoint.vnet.fail_next.set(true);
    endpoint.flush_tcp_closes(Instant(1));
    endpoint.ensure_listener(Instant(1));
    assert_eq!(endpoint.vnet.listeners.get(), 0);
    assert_eq!(*endpoint.vnet.closed_handles.borrow(), [client]);

    endpoint.flush_tcp_closes(Instant(2));
    assert_eq!(*endpoint.vnet.closed_handles.borrow(), [client, listener]);
    endpoint.ensure_listener(Instant(2));
    endpoint.ensure_listener(Instant(3));
    assert_eq!(endpoint.vnet.listeners.get(), 1);
    assert!(endpoint.sessions.is_empty());
    assert_eq!(endpoint.rtp_mic, Some(mic));
}

#[test]
fn local_output_requires_a_detectable_mainboard_line_or_headphone_jack() {
    const PIN_WIDGET: u32 = 4 << 20;
    const DETECTABLE_OUTPUT: u32 = PIN_PRESENCE_DETECT | PIN_OUTPUT_CAPABLE;
    const REAR_LINE_OUT: u32 = 0x01014010;
    const FRONT_HEADPHONE: u32 = 0x02214020;
    assert!(matches!(
        JackKind::from_pin(PIN_WIDGET, DETECTABLE_OUTPUT, REAR_LINE_OUT),
        Some(JackKind::LineOut),
    ));
    assert!(matches!(
        JackKind::from_pin(PIN_WIDGET, DETECTABLE_OUTPUT, FRONT_HEADPHONE),
        Some(JackKind::Headphone),
    ));

    for config in [
        REAR_LINE_OUT | (1 << 20), // Speaker
        REAR_LINE_OUT | (4 << 20), // S/PDIF
        REAR_LINE_OUT | (1 << 28), // Internal location
        REAR_LINE_OUT | (1 << 30), // Port without a physical jack
        REAR_LINE_OUT | (2 << 30), // Fixed port
        REAR_LINE_OUT | (3 << 30), // Combined jack and fixed port
        REAR_LINE_OUT | JACK_DETECT_OVERRIDE,
    ] {
        assert!(JackKind::from_pin(PIN_WIDGET, DETECTABLE_OUTPUT, config).is_none());
    }
    assert!(JackKind::from_pin(PIN_WIDGET | DIGITAL_WIDGET, DETECTABLE_OUTPUT, REAR_LINE_OUT).is_none());
    assert!(JackKind::from_pin(PIN_WIDGET, PIN_PRESENCE_DETECT, REAR_LINE_OUT).is_none());
    assert!(JackKind::from_pin(PIN_WIDGET, PIN_OUTPUT_CAPABLE, FRONT_HEADPHONE).is_none());
}

#[test]
fn local_output_keeps_the_connected_jack_until_unplugged() {
    assert_eq!(select_jack(None, [].into_iter()), None);
    assert_eq!(select_jack(None, [false, false].into_iter()), None);
    assert_eq!(select_jack(None, [true, true].into_iter()), Some(0));
    assert_eq!(select_jack(None, [false, true].into_iter()), Some(1));
    assert_eq!(select_jack(Some(1), [true, true].into_iter()), Some(1));
    assert_eq!(select_jack(Some(1), [true, false].into_iter()), Some(0));
    assert_eq!(select_jack(Some(0), [false, false].into_iter()), None);
}
'''
    harness = (harness.replace('@CONSTANTS@', constants)
               .replace('@PRODUCTION@', '\n'.join(bodies))
               .replace('@HDA_POLICY@', hda_policy))
    with tempfile.TemporaryDirectory(prefix='trueos-audio-http-') as work:
        router_path = ROOT / 'src/r/services/audio_playback.rs'
        router = re.sub(r'^#\[trueos_executor::task.*\]\n', '', router_path.read_text(), flags=re.M)
        router = router.replace('mod sink;', f'#[path="{router_path.with_suffix("") / "sink.rs"}"] mod sink;')
        (Path(work) / 'audio_playback.rs').write_text(router + '''
pub(super) fn reset_test_sink() { *SINK.lock() = sink::Sink::new(); }
pub(super) fn submit_test_clip(owner: ProducerOwner, pcm: &[u8]) -> Result<(), i32> {
    SINK.lock().submit(owner, pcm)
}
''')
        source_path=Path(work)/'test.rs'; binary=Path(work)/'test'
        source_path.write_text(harness)
        subprocess.run(['rustup','run','nightly-2026-07-10','rustc','--edition=2024','--test',str(source_path),'-o',str(binary)],check=True)
        subprocess.run([str(binary),'--test-threads=1'],check=True)


if __name__ == '__main__':
    main()
