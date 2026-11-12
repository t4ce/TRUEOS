#!/usr/bin/env python3
"""Exercise the production SSH RDP record parser and session cleanup."""
from pathlib import Path
import subprocess
import tempfile
import re

ROOT = Path(__file__).resolve().parents[2]
SOURCE = r'''
extern crate alloc;
extern crate self as trueos_crypto;
pub type AccountId = u64;
use std::cell::RefCell;
#[derive(Default)] struct World { inputs:Vec<Vec<u8>>, opens:usize, drops:usize, viewer:Option<u64>, packets:Vec<Vec<u8>> }
thread_local! { static WORLD: RefCell<World> = RefCell::new(World::default()); }
fn world<T>(f:impl FnOnce(&mut World)->T)->T { WORLD.with(|w| f(&mut w.borrow_mut())) }
mod ui4 {
 pub fn output_dimensions()->Option<(u32,u32)>{Some((2560,1440))}
 pub fn rdp_media_subscribe(id:u64)->bool{crate::world(|w|{if w.viewer.is_some(){false}else{w.viewer=Some(id);true}})}
 pub fn rdp_media_unsubscribe(id:u64){crate::world(|w|if w.viewer==Some(id){w.viewer=None;w.packets.clear()})}
 pub fn rdp_media_take_packet(id:u64)->Option<Vec<u8>>{crate::world(|w| if w.viewer==Some(id)&&!w.packets.is_empty(){Some(w.packets.remove(0))}else{None})}
}
mod shell3 {
 pub mod rdp {
  pub struct InputSession(u64);
  impl InputSession {
   pub fn new(account:u64)->Result<Self,&'static str>{crate::world(|w|w.opens+=1);Ok(Self(account+1))}
   pub fn id(&self)->u64{self.0}
   pub fn input(&mut self,data:&[u8])->Result<(),&'static str>{crate::world(|w|w.inputs.push(data.to_vec()));Ok(())}
   pub fn control(&self)->Vec<u8>{b"THIC\x01\x00\x01\x00".to_vec()}
  }
  impl Drop for InputSession{fn drop(&mut self){crate::world(|w|w.drops+=1);}}
 }
 #[path="@CHANNEL@"] pub mod rdp_channel;
}
use shell3::rdp_channel::Channel;
fn reset(){world(|w|*w=World::default());}
fn record(kind:u8,payload:&[u8])->Vec<u8>{let mut b=vec![kind];b.extend_from_slice(&(payload.len() as u32).to_le_bytes());b.extend_from_slice(payload);b}
#[test]fn fragmented_hello_and_input_preserve_exact_record_boundaries(){
 reset();let mut c=Channel::new(0);let hello=record(1,&[1,0]);
 for b in hello {c.receive(&[b]).unwrap();}
 assert!(c.initialized());assert_eq!(c.output,record(0x81,&[0,10,0,0,160,5,0,0]));
 let mut frames=record(2,b"one");frames.extend(record(2,b"two"));
 for b in frames.chunks(2){c.receive(b).unwrap();}
 assert_eq!(world(|w|w.inputs.clone()),vec![b"one".to_vec(),b"two".to_vec()]);
}
#[test]fn noview_never_claims_or_reads_video_and_drop_releases_devices(){
 reset();{let mut c=Channel::new(0);c.receive(&record(1,&[1,0])).unwrap();c.output.clear();c.poll();
 assert_eq!(c.output[0],0x82);assert_eq!(world(|w|w.viewer),None);}
 assert_eq!(world(|w|(w.opens,w.drops)),(1,1));
}
#[test]fn view_uses_same_input_admission_and_releases_media_on_disconnect(){
 reset();{let mut c=Channel::new(0);c.receive(&record(1,&[1,1])).unwrap();
 assert_eq!(world(|w|w.viewer),Some(1));c.output.clear();c.poll();c.output.clear();
 world(|w|w.packets.push(b"TME1packet".to_vec()));c.poll();assert_eq!(c.output,record(0x83,b"TME1packet"));}
 assert_eq!(world(|w|w.viewer),None);assert_eq!(world(|w|w.drops),1);
}
#[test]fn duplicate_and_invalid_hellos_cannot_allocate_extra_devices(){
 reset();let mut c=Channel::new(0);
 for bad in [record(2,b"input"),record(1,&[2,0]),record(1,&[1,2]),record(1,&[])]{assert!(c.receive(&bad).is_err());}
 assert_eq!(world(|w|w.opens),0);c.receive(&record(1,&[1,0])).unwrap();
 assert!(c.receive(&record(1,&[1,0])).is_err());assert_eq!(world(|w|w.opens),1);
}
#[test]fn oversized_record_is_rejected_before_payload_or_devices(){
 reset();let mut c=Channel::new(0);assert!(c.receive(&[1,1,0,1,0]).is_err());
 assert_eq!(world(|w|w.opens),0);
}
#[test]fn failed_view_claim_cleans_up_new_input_session(){
 reset();world(|w|w.viewer=Some(99));let mut c=Channel::new(0);
 assert!(c.receive(&record(1,&[1,1])).is_err());assert!(!c.initialized());
 assert_eq!(world(|w|(w.opens,w.drops)),(1,1));assert_eq!(world(|w|w.viewer),Some(99));
}
'''

def main():
    channel = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / 'rust-toolchain.toml').read_text())[1]
    with tempfile.TemporaryDirectory(prefix='rdp-ssh-channel-') as tmp:
        tmp = Path(tmp)
        source = tmp / 'test.rs'
        source.write_text(SOURCE.replace('@CHANNEL@', str(ROOT / 'src/shell3/rdp_channel.rs')))
        subprocess.run(['rustc', '+' + channel, '--edition=2024', '--test', str(source), '-o', str(tmp / 'tests')], check=True)
        subprocess.run([str(tmp / 'tests'), '--test-threads=1'], check=True)

if __name__ == '__main__': main()
