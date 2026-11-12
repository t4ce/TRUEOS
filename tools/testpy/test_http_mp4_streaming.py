#!/usr/bin/env python3
"""Exercise the production HTTP MP4 reader against a deterministic range server.

Only network transport, playback cancellation and timers are mocked. MP4 sample
indexing, fragment parsing, read-ahead and Annex-B conversion are production code.
"""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
source = (ROOT / "src/intel/media/hw_vid.rs").read_text()
start = source.rfind("#[derive(", 0, source.index("struct Mp4Box {"))
end = source.index("fn h264_has_annexb_start_code(")
demux = source[start:end]
access_units = source[source.index("#[derive(Copy, Clone, Debug)]\nstruct H264StreamNal {"):source.index("struct H264IndexedFrame {")]
memory_reader = source[source.index("struct H264MemoryNalReader {"):source.index("/// Fill a requested range completely;")]
assembler = source[source.index("async fn h264_next_stream_access_unit("):source.index("enum H264NalReader {")]
http_nals = source[source.index("struct H264HttpNalReader {"):source.index("/// Complete access-unit transactions")]
nal_bits = source[source.index("fn h264_find_start_code("):source.index("/// Encoded Blueprint ingress.")]

head = r'''
#![allow(dead_code, unused_imports, unused_variables)]
extern crate alloc;
extern crate self as trueos_time;
use alloc::{string::String, vec::Vec};
use core::sync::atomic::{AtomicBool, Ordering};
use std::cell::{Cell, RefCell};
use std::future::Future;
use std::task::{Context, Poll};
static CANCELLED:AtomicBool=AtomicBool::new(false);
#[macro_export]macro_rules! log {($($t:tt)*)=>{{let _=format_args!($($t)*);}};}
#[macro_export]macro_rules! log_info {($($t:tt)*)=>{};}
#[macro_export]macro_rules! log_warn {($($t:tt)*)=>{};}
pub use std::time::{Duration, Instant};
pub struct Timer;
impl Timer {
 pub async fn after_millis(_:u64){std::future::pending::<()>().await}
 pub async fn after(_:Duration){std::future::pending::<()>().await}
}
mod ui4 {
 #[derive(Clone,Copy)]pub struct VideoPlaybackSession;
 impl VideoPlaybackSession {pub fn is_cancelled(self)->bool {crate::CANCELLED.load(crate::Ordering::SeqCst)}}
}
mod allocators {pub fn host_heap_integrity_bounded(){}}
#[derive(Default)]struct Server {
 bytes:Vec<u8>, reads:Vec<(u64,usize)>, fail_at:Option<u64>, short_read:bool,
}
thread_local! {static SERVER:RefCell<Server>=RefCell::new(Server::default());}
mod r {pub mod net {pub mod https {
 use alloc::{string::String, vec::Vec};
 #[derive(Clone,Debug,PartialEq,Eq)]pub struct MediaRangeIdentity {
  pub final_url:String,pub total_len:u64,pub etag:Option<String>,pub last_modified:Option<String>,
 }
 pub struct MediaRangeResponse {pub bytes:Vec<u8>,pub identity:MediaRangeIdentity}
 pub struct MediaFetchCancellation;
 impl MediaFetchCancellation {pub fn new()->Self {Self}pub fn cancel(&self){}}
 pub async fn get_media_range_bytes_shared(url:&str,offset:u64,length:usize,_:u32,_:&MediaFetchCancellation,_:Option<&str>,_:Option<&str>,expected:Option<&MediaRangeIdentity>)->Result<MediaRangeResponse,String> {
  crate::SERVER.with(|server| {
   let mut server=server.borrow_mut();server.reads.push((offset,length));
   if server.fail_at.is_some_and(|at|offset>=at){return Err("mock range transport failed".into());}
   let identity=MediaRangeIdentity{final_url:url.into(),total_len:server.bytes.len() as u64,etag:Some("\"fixture-v1\"".into()),last_modified:None};
   if expected.is_some_and(|old|old!=&identity){return Err("resource changed".into());}
   let start=usize::try_from(offset).unwrap();
   if start>=server.bytes.len(){return Err("range outside file".into());}
   let end=start.saturating_add(length).min(server.bytes.len());
   let mut bytes=server.bytes[start..end].to_vec();
   if server.short_read {bytes.pop();}
   Ok(MediaRangeResponse{bytes,identity})
  })
 }
}}}
mod media {
use alloc::{string::String,vec::Vec};
use core::fmt::Write;
use crate::ui4::VideoPlaybackSession;
use trueos_time::{Duration as EmbassyDuration,Instant as EmbassyInstant,Timer};
const H264_ONLINE_MEDIA_FETCH_TIMEOUT_MS:u64=120_000;
const H264_ONLINE_MEDIA_FETCH_MAX_BYTES:usize=512*1024*1024;
const H264_FS_METADATA_CAP_BYTES:usize=32*1024*1024;
const H264_FS_PICTURE_CAP_BYTES:usize=8*1024*1024;
const H264_FS_SAMPLE_READ_AHEAD_BYTES:usize=1024*1024;
const H264_FS_INDEX_READ_AHEAD_BYTES:usize=64*1024;
'''
head += f'#[path="{ROOT / "src/intel/media/http_mp4.rs"}"] mod http_mp4;\n'

# Unused source variants are stubs; HTTP NAL delivery, memory NAL parsing and
# access-unit assembly run verbatim from the production decode path.
reader_dispatch = r'''
struct UnusedReader {failed:bool}
enum H264NalReader {File(UnusedReader),Hls(UnusedReader),Http(H264HttpNalReader),Memory(H264MemoryNalReader),Live(UnusedReader)}
impl H264NalReader {
 fn failed(&self)->bool {match self {Self::Http(r)=>r.failed,_=>false}}
 async fn next_nal(&mut self)->Option<H264BufferedNal> {match self {Self::Http(r)=>r.next_nal().await,Self::Memory(r)=>r.next_nal().await,_=>None}}
}
'''

tail = r'''
#[cfg(test)]mod tests {
 use super::*;
 use super::http_mp4::HttpMp4Reader;
 use crate::{SERVER,Server,CANCELLED,Ordering};
 fn run<T>(future:impl core::future::Future<Output=T>)->T {
  let mut future=Box::pin(future);
  let mut context=core::task::Context::from_waker(core::task::Waker::noop());
  for _ in 0..32 {if let core::task::Poll::Ready(value)=future.as_mut().poll(&mut context){return value;}}
  panic!("mocked byte-range fetch did not complete");
 }
 fn install(bytes:Vec<u8>) {CANCELLED.store(false,Ordering::SeqCst);SERVER.with(|s|*s.borrow_mut()=Server{bytes,..Server::default()});}
 fn open(indexed:Option<usize>)->Result<HttpMp4Reader,&'static str> {
  run(HttpMp4Reader::open(crate::ui4::VideoPlaybackSession,"https://fixture.test/movie.mp4",indexed,None,Some("https://fixture.test/watch")))
 }
 fn next(reader:&mut HttpMp4Reader)->Result<Option<(Vec<u8>,H264SampleTiming)>,&'static str> {run(reader.next_sample())}
 fn bx(name:&[u8;4],data:&[u8])->Vec<u8> {[(8+data.len() as u32).to_be_bytes().as_slice(),name,data].concat()}
 fn ints(values:&[u32])->Vec<u8> {values.iter().copied().flat_map(u32::to_be_bytes).collect()}
 fn avc_track(tables:Vec<Vec<u8>>)->Vec<u8> {
  let avcc=bx(b"avcC",&[1,100,0,21,255,225,0,2,0x67,0x80,1,0,2,0x68,0x80]);
  let visual=bx(b"avc1",&[vec![0;78],avcc].concat());
  let stsd=bx(b"stsd",&[ints(&[0,1]),visual].concat());
  let minf=bx(b"minf",&bx(b"stbl",&[vec![stsd],tables].concat().concat()));
  let mut mdhd=vec![0;16];mdhd[12..].copy_from_slice(&90000u32.to_be_bytes());
  let mut hdlr=vec![0;12];hdlr[8..].copy_from_slice(b"vide");
  let mdia=bx(b"mdia",&[bx(b"mdhd",&mdhd),bx(b"hdlr",&hdlr),minf].concat());
  let mut tkhd=vec![0;16];tkhd[12..].copy_from_slice(&1u32.to_be_bytes());
  bx(b"trak",&[bx(b"tkhd",&tkhd),mdia].concat())
 }
 fn ftyp()->Vec<u8> {bx(b"ftyp",b"iso6\0\0\0\0iso6")}
 fn classic(bad_offset:bool)->(Vec<u8>,usize) {
  let first=ftyp().len()+8;
  let second=first+3*1024*1024;
  let mut payload=vec![0;4*1024*1024];
  payload[..6].copy_from_slice(&[0,0,0,2,0x65,0x80]);
  payload[second-first..second-first+6].copy_from_slice(&[0,0,0,2,0x41,0x81]);
  let tables=vec![
   bx(b"stsz",&ints(&[0,0,2,6,6])),
   bx(b"stsc",&ints(&[0,1,1,1,1])),
   bx(b"stco",&ints(&[0,2,if bad_offset{u32::MAX}else{first as u32},second as u32])),
   bx(b"stts",&ints(&[0,1,2,3600])),
   bx(b"stss",&ints(&[0,1,1])),
   bx(b"ctts",&ints(&[0x01000000,2,1,3600,1,(-3600i32) as u32])),
  ];
  ([ftyp(),bx(b"mdat",&payload),bx(b"moov",&avc_track(tables))].concat(),second)
 }
 fn initialization()->Vec<u8> {
  let trex=bx(b"trex",&ints(&[0,1,1,3600,6,0]));
  [ftyp(),bx(b"moov",&[avc_track(vec![]),bx(b"mvex",&trex)].concat())].concat()
 }
 fn fragment(time:u32,offset:Option<i32>,padding:usize)->Vec<u8> {
  let tfhd=bx(b"tfhd",&ints(&[0x020000,1]));
  let tfdt=bx(b"tfdt",&ints(&[0,time]));
  let trun=|offset:i32|bx(b"trun",&ints(&[0x01000a01,2,offset as u32,6,3600,6,(-3600i32) as u32]));
  let provisional=bx(b"moof",&bx(b"traf",&[tfhd.clone(),tfdt.clone(),trun(0)].concat()));
  let moof=bx(b"moof",&bx(b"traf",&[tfhd,tfdt,trun(offset.unwrap_or(provisional.len() as i32+8))].concat()));
  let mut payload=vec![0,0,0,2,0x65,0x80,0,0,0,2,0x41,0x81];payload.resize(payload.len()+padding,0);
  [moof,bx(b"mdat",&payload)].concat()
 }
 fn fragments(offset:Option<i32>)->(Vec<u8>,usize) {
  let initialization=initialization();let first=fragment(9000,offset,3*1024*1024);
  let later=initialization.len()+first.len();
  ([initialization,first,fragment(16200,None,2*1024*1024)].concat(),later)
 }
 fn fragment_with_runs(time:u32,base:Option<u64>,multiple_runs:bool,empty:bool)->Vec<u8> {
  let tfhd=bx(b"tfhd",&match base {
   Some(base)=>[ints(&[1,1]),base.to_be_bytes().to_vec()].concat(),
   None=>ints(&[0x020000,1]),
  });
  let tfdt=bx(b"tfdt",&ints(&[0,time]));
  let runs=|offset:i32| {
   if multiple_runs {
    [bx(b"trun",&ints(&[1,1,offset as u32])),bx(b"trun",&ints(&[0,1]))].concat()
   } else {bx(b"trun",&ints(&[1,if empty{0}else{2},offset as u32]))}
  };
  let provisional=bx(b"moof",&bx(b"traf",&[tfhd.clone(),tfdt.clone(),runs(0)].concat()));
  let offset=if base.is_some(){0}else{provisional.len() as i32+8};
  let moof=bx(b"moof",&bx(b"traf",&[tfhd,tfdt,runs(offset)].concat()));
  let payload=if empty{vec![]}else{vec![0,0,0,2,0x65,0x80,0,0,0,2,0x41,0x81]};
  [moof,bx(b"mdat",&payload)].concat()
 }
 fn check_first(bytes:&[u8]) {
  for nal in [&[0,0,0,1,0x67][..],&[0,0,0,1,0x68][..],&[0,0,0,1,0x65,0x80][..]] {
   assert!(bytes.windows(nal.len()).any(|window|window==nal),"missing AVC headers or first sample");
  }
  assert!(!bytes.windows(6).any(|window|window==[0,0,0,1,0x41,0x81]),"reader returned a later picture");
 }
 #[test]fn tail_moov_starts_without_downloading_the_mdat() {
  let (bytes,later)=classic(false);let len=bytes.len();install(bytes);
  let mut reader=open(None).unwrap();let (first,timing)=next(&mut reader).unwrap().unwrap();check_first(&first);
  assert_eq!((timing.dts,timing.pts,timing.duration,timing.timescale),(0,3600,3600,90000));
  SERVER.with(|server| {let server=server.borrow();
   assert!(server.reads.iter().all(|&(offset,count)|!(offset<=later as u64&&offset+count as u64>later as u64)),"later compressed picture downloaded before the first picture: {:?}",server.reads);
   assert!(server.reads.iter().map(|(_,count)|count).sum::<usize>()<len/2,"read-ahead downloaded most of the movie");
  });
  let (second,timing)=next(&mut reader).unwrap().unwrap();assert!(second.windows(6).any(|w|w==[0,0,0,1,0x41,0x81]));
  assert_eq!((timing.dts,timing.pts),(3600,0));assert!(next(&mut reader).unwrap().is_none());
 }
 #[test]fn fragments_are_fetched_as_playback_reaches_them_with_pts_preserved() {
  let (bytes,later)=fragments(None);install(bytes);
  let mut reader=open(None).unwrap();let (first,timing)=next(&mut reader).unwrap().unwrap();check_first(&first);
  assert_eq!((timing.dts,timing.pts,timing.duration,timing.timescale),(9000,12600,3600,90000));
  SERVER.with(|s|assert!(s.borrow().reads.iter().all(|&(offset,count)|offset+count as u64<=later as u64),"later fragment fetched before first sample"));
  assert_eq!(next(&mut reader).unwrap().unwrap().1.pts,9000);
  assert_eq!(next(&mut reader).unwrap().unwrap().1.dts,16200);
  assert_eq!(next(&mut reader).unwrap().unwrap().1.dts,19800);
  assert!(next(&mut reader).unwrap().is_none());
 }
 #[test]fn a_second_trun_without_offset_continues_after_the_first_run() {
  install([initialization(),fragment_with_runs(9000,None,true,false)].concat());
  let mut reader=open(None).unwrap();let (first,timing)=next(&mut reader).unwrap().unwrap();check_first(&first);assert_eq!(timing.dts,9000);
  let (second,timing)=next(&mut reader).unwrap().unwrap();assert!(second.windows(6).any(|w|w==[0,0,0,1,0x41,0x81]));assert_eq!(timing.dts,12600);
  assert!(next(&mut reader).unwrap().is_none());
 }
 #[test]fn absolute_tfhd_base_is_preserved_in_a_later_fragment() {
  let prefix=[initialization(),fragment(9000,None,3*1024*1024)].concat();
  let placeholder=fragment_with_runs(16200,Some(0),false,false);
  let absolute_payload=(prefix.len()+placeholder.len()-12) as u64;
  install([prefix,fragment_with_runs(16200,Some(absolute_payload),false,false)].concat());
  let mut reader=open(None).unwrap();assert_eq!(next(&mut reader).unwrap().unwrap().1.dts,9000);assert_eq!(next(&mut reader).unwrap().unwrap().1.dts,12600);
  let (third,timing)=next(&mut reader).unwrap().unwrap();check_first(&third);assert_eq!(timing.dts,16200);
  assert_eq!(next(&mut reader).unwrap().unwrap().1.dts,19800);assert!(next(&mut reader).unwrap().is_none());
 }
 #[test]fn first_run_without_data_offset_starts_at_the_explicit_tfhd_base() {
  let init=initialization();
  let moof=|base:u64|bx(b"moof",&bx(b"traf",&[
   bx(b"tfhd",&[ints(&[1,1]),base.to_be_bytes().to_vec()].concat()),
   bx(b"tfdt",&ints(&[0,9000])),bx(b"trun",&ints(&[0,2])),
  ].concat()));
  let base=(init.len()+moof(0).len()+8+32) as u64;
  let payload=[vec![0;32],vec![0,0,0,2,0x65,0x80,0,0,0,2,0x41,0x81]].concat();
  install([init,moof(base),bx(b"mdat",&payload)].concat());
  let mut reader=open(None).unwrap();let (first,timing)=next(&mut reader).unwrap().unwrap();check_first(&first);assert_eq!(timing.dts,9000);
  let (second,timing)=next(&mut reader).unwrap().unwrap();assert!(second.windows(6).any(|w|w==[0,0,0,1,0x41,0x81]));assert_eq!(timing.dts,12600);
 }
 #[test]fn empty_fragments_do_not_report_end_before_later_samples() {
  install([initialization(),fragment_with_runs(0,None,false,true),fragment_with_runs(9000,None,false,false)].concat());
  let mut reader=open(None).unwrap();let (first,timing)=next(&mut reader).unwrap().unwrap();check_first(&first);assert_eq!(timing.dts,9000);
  assert_eq!(next(&mut reader).unwrap().unwrap().1.dts,12600);assert!(next(&mut reader).unwrap().is_none());
 }
 #[test]fn impossible_fragment_sample_count_is_rejected_from_metadata() {
  let mut bytes=[initialization(),fragment_with_runs(9000,None,false,false)].concat();
  let trun=bytes.windows(4).position(|w|w==b"trun").unwrap();bytes[trun+8..trun+12].copy_from_slice(&u32::MAX.to_be_bytes());
  install(bytes);assert!(open(None).is_err(),"unbounded fragmented sample count admitted");
 }
 #[test]fn impossible_classic_sample_count_is_rejected_from_metadata() {
  let (mut bytes,_)=classic(false);
  let stsz=bytes.windows(4).position(|w|w==b"stsz").unwrap();
  bytes[stsz+8..stsz+12].copy_from_slice(&6u32.to_be_bytes());
  bytes[stsz+12..stsz+16].copy_from_slice(&u32::MAX.to_be_bytes());
  install(bytes);assert!(open(None).is_err(),"unbounded classic sample count admitted");
 }
 #[test]fn a_complete_small_object_reuses_the_initial_probe_for_samples() {
  install([initialization(),fragment_with_runs(9000,None,false,false)].concat());
  let mut reader=open(None).unwrap();assert!(next(&mut reader).unwrap().is_some());assert!(next(&mut reader).unwrap().is_some());assert!(next(&mut reader).unwrap().is_none());
  SERVER.with(|s|assert_eq!(s.borrow().reads.len(),1,"small complete object fetched again after the initial probe"));
 }
 #[test]fn production_access_unit_assembly_does_not_wait_for_the_next_http_picture() {
  let (bytes,later)=classic(false);install(bytes);let http=open(None).unwrap();
  SERVER.with(|s|{let mut s=s.borrow_mut();s.reads.clear();s.fail_at=Some(later as u64);});
  let mut reader=H264NalReader::Http(H264HttpNalReader{http,memory:None,offset:0,failed:false});
  let (mut pending,mut sps,mut pps,mut missing,mut count)=(None,None,None,0,0);
  let unit=run(h264_next_stream_access_unit(crate::ui4::VideoPlaybackSession,&mut reader,&mut pending,&mut sps,&mut pps,&mut missing,&mut count)).expect("first complete picture waited for a later HTTP sample");
  assert_eq!((unit.nal_type,unit.vcl_nals),(5,1));assert!(!unit.sps.is_empty());assert!(!unit.pps.is_empty());assert!(!reader.failed());
  SERVER.with(|s|assert!(s.borrow().reads.iter().all(|&(offset,_)|offset<later as u64),"NAL assembly requested the next picture before emitting the current one"));
 }
 #[test]fn indexed_hls_prefix_excludes_unlisted_fragments() {
  let (bytes,later)=fragments(None);install(bytes);
  let mut reader=open(Some(later)).unwrap();assert!(next(&mut reader).unwrap().is_some());assert!(next(&mut reader).unwrap().is_some());assert!(next(&mut reader).unwrap().is_none());
  SERVER.with(|s|assert!(s.borrow().reads.iter().all(|&(offset,count)|offset+count as u64<=later as u64),"download escaped the HLS admitted prefix"));
 }
 #[test]fn short_indexed_hls_prefix_also_bounds_the_initial_probe() {
  let admitted=[initialization(),fragment(9000,None,0)].concat();let length=admitted.len();
  assert!(length<64*1024);
  install([admitted,fragment(16200,None,2*1024*1024)].concat());
  let mut reader=open(Some(length)).unwrap();
  assert!(next(&mut reader).unwrap().is_some());assert!(next(&mut reader).unwrap().is_some());assert!(next(&mut reader).unwrap().is_none());
  SERVER.with(|s|assert!(s.borrow().reads.iter().all(|&(offset,count)|offset+count as u64<=length as u64),"initial probe downloaded unlisted HLS fragments"));
 }
 #[test]fn out_of_file_sample_and_fragment_offsets_fail_before_payload_fetch() {
  let (bytes,_)=classic(true);install(bytes);assert!(open(None).is_err());
  let (bytes,_)=fragments(Some(i32::MAX));install(bytes);
  let failed=match open(None){Err(_)=>true,Ok(mut reader)=>next(&mut reader).is_err()};assert!(failed,"out-of-file fragmented sample reached decode");
 }
 #[test]fn cancellation_stops_subsequent_http_reads() {
  let (bytes,_)=classic(false);install(bytes);let mut reader=open(None).unwrap();
  let before=SERVER.with(|s|s.borrow().reads.len());CANCELLED.store(true,Ordering::SeqCst);
  assert!(next(&mut reader).is_err());assert_eq!(SERVER.with(|s|s.borrow().reads.len()),before,"cancelled reader performed another fetch");
 }
 #[test]fn truncated_range_response_never_reaches_decode() {
  let (bytes,_)=classic(false);install(bytes);let mut reader=open(None).unwrap();
  assert!(next(&mut reader).unwrap().is_some());
  SERVER.with(|s|s.borrow_mut().short_read=true);
  let result=next(&mut reader);assert!(result.is_err(),"truncated HTTP range produced a sample");
 }
}
}
'''

with tempfile.TemporaryDirectory(prefix="http-mp4-streaming-") as directory:
    path = Path(directory)
    (path / "test.rs").write_text(head + demux + access_units + memory_reader + assembler + http_nals + nal_bits + reader_dispatch + tail)
    subprocess.run(["rustc", "--edition=2024", "--test", str(path / "test.rs"), "-o", str(path / "test")], check=True)
    subprocess.run([str(path / "test"), "--test-threads=1", "--nocapture"], check=True)
