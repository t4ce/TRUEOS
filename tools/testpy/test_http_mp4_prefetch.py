#!/usr/bin/env python3
"""Run the production HTTP producer and consumer on a deterministic host executor.

Reuses the parser/range fixtures, but schedules the producer independently and
keeps real queue backpressure, timing batches, cancellation and shutdown code.
"""
from pathlib import Path
import runpy
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
fixture = runpy.run_path(str(ROOT / "tools/testpy/test_http_mp4_streaming.py"), run_name="mp4_fixtures")

executor = r'''
extern crate self as atomic_waker;
extern crate self as spin;
pub struct AtomicWaker;
impl AtomicWaker {pub fn new()->Self{Self}pub fn register(&self,_:&core::task::Waker){}pub fn wake(&self){}}
pub struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T>{pub fn new(value:T)->Self{Self(std::sync::Mutex::new(value))}pub fn lock(&self)->std::sync::MutexGuard<'_,T>{self.0.lock().unwrap()}}
type Task=core::pin::Pin<Box<dyn Future<Output=()>>>;
thread_local!{static TASKS:RefCell<Vec<Task>>=const{RefCell::new(Vec::new())};static WORKER_AVAILABLE:Cell<bool>=const{Cell::new(true)};static TASK_AVAILABLE:Cell<bool>=const{Cell::new(true)};static CLOSES:Cell<usize>=const{Cell::new(0)};static CANCEL_FENCES:Cell<usize>=const{Cell::new(0)};}
fn poll_producers(){
 let tasks=TASKS.with(|t|core::mem::take(&mut *t.borrow_mut()));
 for mut task in tasks{let mut cx=Context::from_waker(core::task::Waker::noop());if task.as_mut().poll(&mut cx).is_pending(){TASKS.with(|t|t.borrow_mut().push(task));}}
}
mod workers {
 pub struct Worker;
 impl Worker {pub fn spawn(&self,future:impl core::future::Future<Output=()>+'static){crate::TASKS.with(|t|t.borrow_mut().push(Box::pin(future)));}}
 pub fn pick_background_spawner()->Option<Worker>{crate::WORKER_AVAILABLE.with(|available|available.get().then_some(Worker))}
}
pub struct Yield(bool);
impl Future for Yield{type Output=();fn poll(mut self:core::pin::Pin<&mut Self>,_:&mut Context<'_>)->Poll<()>{if self.0{Poll::Ready(())}else{self.0=true;Poll::Pending}}}
'''

buffer_tests = r'''
#[cfg(test)]mod prefetch_tests {
 use super::*;
 use crate::Cell;
 use crate::media::tests::{run,install,open,classic,initialization,fragment,ftyp,bx,ints,avc_track};
 fn next(reader:&mut BufferedHttpMp4Reader)->Result<Option<(Vec<u8>,H264SampleTiming)>,&'static str>{run(reader.next_sample())}
 #[test]fn failed_prefetch_admission_awaits_source_close(){
  for (worker,task) in [(false,true),(true,false)]{
   let (bytes,_)=classic(false);install(bytes);let parser=open(None).unwrap();let before=crate::CLOSES.with(Cell::get);
   crate::WORKER_AVAILABLE.with(|available|available.set(worker));crate::TASK_AVAILABLE.with(|available|available.set(task));
   assert!(run(parser.into_buffered()).is_err());assert_eq!(crate::CLOSES.with(Cell::get),before+1);crate::TASKS.with(|tasks|assert!(tasks.borrow().is_empty()));
   crate::WORKER_AVAILABLE.with(|available|available.set(true));crate::TASK_AVAILABLE.with(|available|available.set(true));
  }
 }
 #[test]fn fetches_next_network_window_while_consumer_is_idle(){
  let (bytes,later)=classic(false);install(bytes);let mut reader=run(open(None).unwrap().into_buffered()).unwrap();crate::poll_producers();
  assert_eq!(reader.shared.state.lock().queue.len(),2);
  crate::SERVER.with(|s|assert!(s.borrow().reads.iter().any(|&(offset,_)|offset==later as u64),"future sample was not fetched independently"));
  assert!(reader.timing.is_empty(),"consumer timing advanced before its first sample");
  let first=next(&mut reader).unwrap().unwrap();assert_eq!(first.1.pts,3600);assert_eq!(reader.timing.len(),2,"complete B-frame batch missing on first delivery");
  assert_eq!(next(&mut reader).unwrap().unwrap().1.pts,0);assert!(next(&mut reader).unwrap().is_none());run(reader.close());
 }
 #[test]fn media_time_watermarks_bound_and_refill_the_queue(){
  let mut bytes=initialization();for index in 0..30{bytes.extend(fragment(index*7200,None,0));}install(bytes);
  let mut reader=run(open(None).unwrap().into_buffered()).unwrap();crate::poll_producers();
  {let state=reader.shared.state.lock();assert_eq!(state.queue.len(),19);assert_eq!(state.duration_us,760_000);assert!(!state.done);assert!(state.bytes<=MAX_QUEUED_BYTES);}
  for _ in 0..13{next(&mut reader).unwrap().unwrap();}assert_eq!(reader.shared.state.lock().queue.len(),6);crate::poll_producers();
  {let state=reader.shared.state.lock();assert_eq!(state.queue.len(),19);assert_eq!(state.duration_us,760_000);assert!(state.max_pictures<=MAX_QUEUED_PICTURES);}
  run(reader.close());assert!(reader.shared.state.lock().done);assert!(reader.shared.state.lock().queue.is_empty());crate::TASKS.with(|tasks|assert!(tasks.borrow().is_empty()));
 }
 #[test]fn queued_good_picture_is_delivered_before_later_fetch_error(){
  let (bytes,later)=classic(false);install(bytes);let mut reader=run(open(None).unwrap().into_buffered()).unwrap();crate::SERVER.with(|s|s.borrow_mut().fail_at=Some(later as u64));
  assert!(next(&mut reader).unwrap().is_some());assert_eq!(next(&mut reader).unwrap_err(),"video HTTP range fetch failed");run(reader.close());
 }
 #[test]fn close_cancels_inflight_refill_and_waits_for_transport_fence(){
  let (bytes,later)=classic(false);install(bytes);let mut reader=run(open(None).unwrap().into_buffered()).unwrap();crate::SERVER.with(|s|s.borrow_mut().pending_at=Some(later as u64));crate::poll_producers();
  assert!(!reader.shared.state.lock().done);let before=crate::CANCEL_FENCES.with(Cell::get);run(reader.close());
  assert_eq!(crate::CANCEL_FENCES.with(Cell::get),before+1);assert!(reader.shared.state.lock().done);crate::TASKS.with(|tasks|assert!(tasks.borrow().is_empty()));
 }
 #[test]fn dropped_consumer_stops_and_reaps_the_owned_producer(){
  let mut bytes=initialization();for index in 0..30{bytes.extend(fragment(index*7200,None,0));}install(bytes);let reader=run(open(None).unwrap().into_buffered()).unwrap();let shared=reader.shared.clone();crate::poll_producers();drop(reader);crate::poll_producers();assert!(shared.state.lock().done);crate::TASKS.with(|tasks|assert!(tasks.borrow().is_empty()));
 }
 #[test]fn byte_capacity_holds_one_candidate_without_growing_queue(){
  let size=6*1024*1024;let first=ftyp().len()+8;let mut payload=vec![0;size*3];
  for index in 0..3{payload[index*size..index*size+4].copy_from_slice(&((size-4) as u32).to_be_bytes());payload[index*size+4]=if index==0{0x65}else{0x41};}
  let tables=vec![bx(b"stsz",&ints(&[0,size as u32,3])),bx(b"stsc",&ints(&[0,1,1,1,1])),bx(b"stco",&ints(&[0,3,first as u32,(first+size) as u32,(first+size*2) as u32])),bx(b"stts",&ints(&[0,1,3,1])),bx(b"stss",&ints(&[0,1,1]))];
  install([ftyp(),bx(b"mdat",&payload),bx(b"moov",&avc_track(tables))].concat());let mut reader=run(open(None).unwrap().into_buffered()).unwrap();crate::poll_producers();
  {let state=reader.shared.state.lock();assert_eq!(state.queue.len(),2);assert!(state.bytes<=MAX_QUEUED_BYTES);assert!(!state.done);}
  let reads=crate::SERVER.with(|s|s.borrow().reads.len());crate::poll_producers();assert_eq!(crate::SERVER.with(|s|s.borrow().reads.len()),reads,"blocked producer fetched more candidates");
  assert!(next(&mut reader).unwrap().is_some());crate::poll_producers();assert_eq!(reader.shared.state.lock().queue.len(),2);assert!(reader.shared.state.lock().done);run(reader.close());
 }
}
'''

if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="http-mp4-prefetch-") as directory:
        path = Path(directory)
        http = (ROOT / "src/intel/media/http_mp4.rs").read_text().replace("#[cfg(not(test))]\n", "")
        (path / "http_mp4.rs").write_text(http)
        buffer = (ROOT / "src/intel/media/http_mp4_buffer.rs").read_text()
        task_start = buffer.index("#[trueos_executor::task(pool_size = 3)]")
        buffer = buffer[:task_start] + "fn prefetch_task(shared:Arc<PrefetchShared>)->Result<impl core::future::Future<Output=()>,()>{if !crate::TASK_AVAILABLE.with(crate::Cell::get){return Err(());}Ok(async move {let reader=shared.reader.lock().take().unwrap();run_prefetch(reader,shared).await;})}\n"
        (path / "http_mp4_buffer.rs").write_text(buffer + buffer_tests)
        head = fixture["head"].replace(str(ROOT / "src/intel/media/http_mp4.rs"), str(path / "http_mp4.rs"))
        head = head.replace(" pub async fn after_millis(_:u64){std::future::pending::<()>().await}", " pub async fn after_millis(_:u64){crate::Yield(false).await}")
        head = head.replace("short_read:bool,", "short_read:bool,pending_at:Option<u64>,")
        head = head.replace("pub async fn close(&mut self) {}", "pub async fn close(&mut self) {crate::CLOSES.with(|n|n.set(n.get()+1));}")
        head = head.replace("pub struct MediaFetchCancellation;", "pub struct MediaFetchCancellation(core::sync::atomic::AtomicBool);")
        head = head.replace("pub fn new()->Self {Self}pub fn cancel(&self){}", "pub fn new()->Self {Self(core::sync::atomic::AtomicBool::new(false))}pub fn cancel(&self){self.0.store(true,crate::Ordering::Release);}")
        head = head.replace("_:u32,_:&MediaFetchCancellation", "_:u32,cancellation:&MediaFetchCancellation")
        fetch_body = "  crate::SERVER.with(|server| {\n   let mut server=server.borrow_mut();server.reads.push((offset,length));"
        head = head.replace(fetch_body, """  if crate::SERVER.with(|s|s.borrow().pending_at.is_some_and(|at|offset>=at)) {
   return core::future::poll_fn(|_|if cancellation.0.load(crate::Ordering::Acquire){crate::CANCEL_FENCES.with(|n|n.set(n.get()+1));core::task::Poll::Ready(Err(\"cancelled\".into()))}else{core::task::Poll::Pending}).await;
  }
""" + fetch_body)
        # The executor definitions must precede `mod media {` in the fixture.
        head = head.replace("mod media {", executor + "\nmod media {")
        tail = fixture["tail"].replace(" fn run<", " pub(crate) fn run<")
        for name in ["install", "open", "classic", "initialization", "fragment", "ftyp", "bx", "ints", "avc_track"]:
            tail = tail.replace(f" fn {name}(", f" pub(crate) fn {name}(")
        tail = tail.replace("for _ in 0..32 {if", "for _ in 0..1000 {crate::poll_producers();if")
        body = head + "".join(fixture[name] for name in ["demux", "access_units", "memory_reader", "assembler", "http_nals", "nal_bits", "reader_dispatch"]) + tail
        (path / "test.rs").write_text(body)
        subprocess.run(["rustc", "--edition=2024", "--test", str(path / "test.rs"), "-o", str(path / "test")], check=True)
        subprocess.run([str(path / "test"), "--test-threads=1"], check=True)
