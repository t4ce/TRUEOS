#!/usr/bin/env python3
"""Compile production capture menu/mux and validate the MKV with real decoders."""
from pathlib import Path
import json
import math
import re
import struct
import subprocess
import tempfile
import wave
from test_screenfilm import HARNESS
from capture_host import model
ROOT = Path(__file__).resolve().parents[2]

STUBS = r'''
extern crate self as trueos_executor;
#[derive(Clone,Copy)] pub struct Spawner;
impl Spawner {pub fn spawn<T>(&self,_:T){}}
mod workers {pub type WorkerSpawner=crate::Spawner;pub fn pick_background_spawner()->Option<crate::Spawner>{Some(crate::Spawner)}}
mod ui4 {
    pub fn writable_capture_root_handle()->Option<crate::disc::block::DeviceHandle>{if crate::S.lock().no_root {None} else {Some(crate::disc::block::DeviceHandle)}}
    pub fn request_wd_postblend_capture(_:crate::shell2::MatrixTarget)->Result<(),&'static str>{crate::SHOTS.lock().push(crate::chronos::monotonic_nanos());Ok(())}
    pub fn request_capture_film(_:crate::shell2::MatrixTarget,r:crate::shell3::capture::Recording)->Result<(),&'static str>{crate::RECORDINGS.lock().push(r);Ok(())}
}
static SHOTS:Mutex<Vec<u64>>=Mutex::new(Vec::new());
static RECORDINGS:Mutex<Vec<shell3::capture::Recording>>=Mutex::new(Vec::new());
static RETURNED:AtomicBool=AtomicBool::new(false);
mod shell3 {
    pub mod service {pub fn notify_work(){}}
    pub mod tui {
        #[derive(Clone,Copy)]pub struct Frontend {pub id:u64,pub cols:usize,pub rows:usize}
        pub struct Surface {pub cols:u32,pub rows:u32}
        pub fn native_slot(_:&str)->bool{false}
        pub fn native_visible(_:&crate::shell2::MatrixTarget)->bool{true}
        pub fn request(_:Frontend,_:&str)->Result<(),&'static str>{Ok(())}
        pub fn attach_native(_:Frontend,_:&crate::shell2::MatrixTarget)->Result<(),String>{Ok(())}
        pub fn cancel_native_attach(_:&crate::shell2::MatrixTarget){}
        pub fn native_return(_:&crate::shell2::MatrixTarget){crate::RETURNED.store(true,crate::Ordering::SeqCst);}
        pub fn native_write(_:&crate::shell2::MatrixTarget,_:&[u8]){}
        pub fn native_read(_:&crate::shell2::MatrixTarget)->Option<(Vec<u8>,Vec<String>)>{None}
        pub fn surface(_:&crate::shell2::MatrixTarget)->Option<Surface>{Some(Surface{cols:100,rows:25})}
    }
    #[path="@ROOT@/src/shell3/helper.rs"] mod helper;
    pub mod capture {
        @CAPTURE@
        fn menu_task(_:Kind,_:crate::shell2::MatrixTarget)->Result<(),()>{Ok(())}
        fn mux_task(_:Recording,_:Recording,_:Arc<Mutex<Option<Result<String,String>>>>)->Result<(),()>{Ok(())}
        @TESTS@
    }
}
'''
TESTS = r'''
#[cfg(test)] mod tests {
    use super::*;
    fn setup(){*crate::S.lock()=crate::State::default();crate::NOW.store(0,Ordering::SeqCst);crate::SHOTS.lock().clear();crate::RECORDINGS.lock().clear();crate::RETURNED.store(false,Ordering::SeqCst);LAST_SHOT.store(0,Ordering::SeqCst);}
    #[test] fn fragmented_escape_mouse_crlf_and_quit_are_unambiguous(){
        let mut input=Input::default();assert!(input.feed(b"\x1b",0).is_empty());assert!(input.timeout(74_000_000).is_none());
        assert_eq!(input.feed(b"[B\r\n",30_000_000),vec![Action::Down,Action::Choose]);
        assert!(input.feed(b"\x1b[<0;2;",40_000_000).is_empty());assert_eq!(input.feed(b"5M",50_000_000),vec![Action::Click(4)]);
        assert!(input.feed(b"\x1b[<0;2;5m\x1b[<32;2;5M",60_000_000).is_empty());
        assert_eq!(input.feed(b"\x1b[<64;2;5M\x1b[<65;2;5M",70_000_000),vec![Action::Up,Action::Down]);
        input.feed(b"\x1b",80_000_000);assert_eq!(input.timeout(155_000_000),Some(Action::Quit));
        assert_eq!(input.feed(b"hjq",160_000_000),vec![Action::Quit,Action::Down,Action::Quit]);
    }
    #[test] fn screenshot_cadence_delay_and_three_by_three_use_monotonic_time(){
        fn tick(menu:&mut Menu,target:&crate::shell2::MatrixTarget,now:u64){crate::NOW.store(now,Ordering::SeqCst);menu.pictures(target,now);}
        setup();let target=crate::shell2::MatrixTarget(1);let mut menu=Menu::new(Kind::Pic);
        menu.choose(&target,0);tick(&mut menu,&target,0);assert_eq!(*crate::SHOTS.lock(),vec![0]);
        menu.choose(&target,249_000_000);tick(&mut menu,&target,249_000_000);assert_eq!(crate::SHOTS.lock().len(),1);
        menu.choose(&target,250_000_000);tick(&mut menu,&target,250_000_000);assert_eq!(crate::SHOTS.lock().len(),2);
        menu.selected=1;menu.choose(&target,1_000_000_000);tick(&mut menu,&target,10_999_999_999);assert_eq!(crate::SHOTS.lock().len(),2);
        tick(&mut menu,&target,11_000_000_000);assert_eq!(crate::SHOTS.lock().len(),3);
        menu.selected=2;menu.choose(&target,12_000_000_000);
        for ns in [12_000_000_000,15_000_000_000,18_000_000_000]{tick(&mut menu,&target,ns);}
        assert_eq!(&crate::SHOTS.lock()[3..],&[12_000_000_000,15_000_000_000,18_000_000_000]);assert!(!menu.busy());
    }
    #[test] fn menu_keyboard_mouse_small_geometry_and_return_share_one_layout(){
        setup();let target=crate::shell2::MatrixTarget(1);let mut menu=Menu::new(Kind::Vid);
        menu.action(Action::Down,&target,25,0);assert_eq!(menu.selected,1);
        menu.action(Action::Click(8),&target,25,0);assert_eq!(menu.seconds,900);assert!(menu.busy());
        let recording=menu.video.as_ref().unwrap().clone();assert_eq!(recording.seconds(),900);assert!(!recording.stopped());
        assert!(menu.action(Action::Quit,&target,25,0));assert!(crate::RETURNED.load(Ordering::SeqCst));assert!(!recording.stopped());
        for rows in [5,8,12,25]{for cols in [20,70,100]{assert!(menu.frame(cols,rows,0).iter().any(|line|line.contains("VID")));}}
        menu.choose(&target,0);assert!(menu.video.as_ref().unwrap().stopped());
    }
    #[test] fn vaud_arms_two_tracks_on_one_clock_and_failed_audio_admission_stops_video(){
        setup();let target=crate::shell2::MatrixTarget(1);let mut menu=Menu::new(Kind::Vaud);menu.record(&target,3).unwrap();
        let v=menu.video.as_ref().unwrap();let a=menu.audio.as_ref().unwrap();assert!(Arc::ptr_eq(&v.clock,&a.clock));
        assert_eq!(v.clock.expected,3);assert_eq!(v.clock.epoch.load(Ordering::SeqCst),0);
        // Simulate video-ready while audio has not armed: no clock can start.
        v.clock.ready.fetch_or(VIDEO,Ordering::SeqCst);assert_eq!(v.clock.epoch.load(Ordering::SeqCst),0);
        let epoch=crate::run(a.ready()).unwrap();assert!(epoch>=100_000_000);assert_eq!(v.clock.epoch.load(Ordering::SeqCst),epoch);
        a.finish("audio failed",false);assert!(v.stopped());
        setup();crate::S.lock().slot_busy=true;let mut menu=Menu::new(Kind::Vaud);assert!(menu.record(&target,3).is_err());assert!(menu.video.as_ref().unwrap().stopped());
    }
    fn put(path:&str,data:Vec<u8>){let mut s=crate::S.lock();let id=s.records.len();s.records.push(data);s.paths.insert(path.into(),id);}
    fn sources()->(Recording,Recording){
        setup();let clock=Clock::new(VIDEO|AUDIO,3);let disk=crate::disc::block::DeviceHandle;
        let video=Recording::new(disk,"screenfilms/vaud-test.h264".into(),clock.clone(),VIDEO);
        let audio=Recording::new(disk,"recordings/vaud-test.wav".into(),clock,AUDIO);
        let bytes=include_bytes!("@VIDEO@").to_vec();let pcm=include_bytes!("@AUDIO@").to_vec();
        let mut starts=Vec::new();for i in 0..bytes.len().saturating_sub(4){if bytes[i..].starts_with(&[0,0,0,1,9]){starts.push(i);}}
        assert_eq!(starts.len(),10);assert_eq!(starts[0],0);
        let times=[200,400,600,800,1000,1700,1900,2100,2300,2500];
        for (i,&start) in starts.iter().enumerate(){let end=starts.get(i+1).copied().unwrap_or(bytes.len());video.frame(end-start,1_000_000_000+times[i]*1_000_000);}
        audio.started(1_000_000_000,2);video.started(1_200_000_000,0);video.finish("saved",true);audio.finish("saved",true);
        put(&video.path,bytes);put(&audio.path,pcm);(video,audio)
    }
    #[test] fn mux_writes_one_verified_playable_file_and_retires_only_its_sources(){
        let (v,a)=sources();let path=crate::run(mux::save(&v,&a)).unwrap();let s=crate::S.lock();assert_eq!(s.paths.len(),1);assert_eq!(s.deletes,2);
        std::fs::write("@OUTPUT@",&s.records[*s.paths.get(&path).unwrap()]).unwrap();
        assert!(s.max_chunk<=1024*1024);
    }
    #[test] fn mux_write_commit_and_short_read_failures_keep_both_sources(){
        for failure in 0..3{let (v,a)=sources();{let mut s=crate::S.lock();if failure==0{s.fail_copy=true;}else if failure==1{s.fail_commit=true;}else{s.short_read=true;}}
            assert!(crate::run(mux::save(&v,&a)).is_err());let s=crate::S.lock();assert!(s.paths.contains_key(&v.path)&&s.paths.contains_key(&a.path));assert_eq!(s.deletes,0);
        }
    }
}
'''

def main():
    with tempfile.TemporaryDirectory(prefix='trueos-capture-') as tmp:
        tmp = Path(tmp)
        video, audio, output = tmp/'source.h264', tmp/'source.wav', tmp/'combined.mkv'
        subprocess.run(['ffmpeg','-v','error','-f','lavfi','-i','testsrc=size=64x48:rate=5','-t','2','-pix_fmt','yuv420p','-c:v','libx264','-profile:v','baseline','-x264-params','aud=1:repeat-headers=1:keyint=5:bframes=0','-f','h264',str(video)],check=True)
        pcm = b''.join(struct.pack('<hh', round(math.sin(i*2*math.pi*440/48000)*12000), round(math.sin(i*2*math.pi*660/48000)*10000)) for i in range(round(2.6*48000)))
        with wave.open(str(audio),'wb') as wav:
            wav.setnchannels(2);wav.setsampwidth(2);wav.setframerate(48000);wav.writeframes(pcm)
        base = HARNESS[:HARNESS.index('mod ui4 {')].replace(model(),'')
        base = base.replace('pub mod wd_xyuv8888 {','pub mod avc_encode_probe {pub const FRAME_WIDTH:usize=64;pub const FRAME_HEIGHT:usize=48;} pub mod wd_xyuv8888 {')
        base = base.replace('    #[derive(Clone)] pub struct MatrixTarget', '''    pub const OUTPUT_SYSTEM_MASK:u16=1;
    pub fn matrix_target_for_slot_name(_:u16,_:&str)->MatrixTarget{MatrixTarget(1)}
    pub mod cmds {pub mod rec {
        pub fn request_capture(_:crate::shell2::MatrixTarget,r:crate::shell3::capture::Recording)->Result<(),&'static str>{if crate::S.lock().slot_busy{return Err("audio busy");}crate::RECORDINGS.lock().push(r);Ok(())}
    }}
    #[derive(Clone)] pub struct MatrixTarget''')
        capture = (ROOT/'src/shell3/capture.rs').read_text()
        capture = re.sub(r'^//!.*\n','',capture,flags=re.M)
        capture = re.sub(r'^#\[trueos_executor::task[^\n]*\n','',capture,flags=re.M)
        capture = capture.replace('async fn menu_task(', 'async fn menu_task_run(').replace('async fn mux_task(', 'async fn mux_task_run(')
        capture = capture.replace('mod mux;', f'#[path="{ROOT}/src/shell3/capture/mux.rs"] mod mux;')
        source = base + STUBS.replace('@ROOT@',str(ROOT)).replace('@CAPTURE@',capture).replace('@TESTS@',TESTS)
        for key, value in [('VIDEO',video),('AUDIO',audio),('OUTPUT',output)]: source=source.replace('@'+key+'@',str(value))
        rust, binary = tmp/'tests.rs', tmp/'tests'
        rust.write_text(source)
        subprocess.run(['rustc','--edition=2024','--cfg','feature="trueos_h264_encode_stream"','--test',str(rust),'-o',str(binary)],cwd=ROOT,check=True)
        subprocess.run([str(binary),'--test-threads=1'],check=True)
        probe = json.loads(subprocess.check_output(['ffprobe','-v','error','-show_streams','-show_packets','-of','json',str(output)]))
        streams = probe['streams']; assert [s['codec_name'] for s in streams]==['h264','pcm_s16le']
        assert (streams[0]['width'],streams[0]['height'])==(64,48)
        packets = [float(p['pts_time']) for p in probe['packets'] if p['stream_index']==0]
        assert packets == [0.2,0.4,0.6,0.8,1.0,1.7,1.9,2.1,2.3,2.5], packets
        def decoded(path, mapping, fmt):
            return subprocess.check_output(['ffmpeg','-v','error','-i',str(path),'-map',mapping,'-fps_mode','passthrough','-f',fmt,'-'])
        assert decoded(output,'0:v:0','rawvideo') == decoded(video,'0:v:0','rawvideo')
        assert decoded(output,'0:a:0','s16le') == pcm
        print('ffprobe: AVC + PCM tracks and capture timing verified; ffmpeg: both tracks decode exactly')

if __name__ == '__main__': main()
