#!/usr/bin/env python3
"""Exercise production unified firmware initialization with a scripted transport.

Uses the real QuZ parser/asset, MVM/link/station protocols, WPA2 and CCMP.
A scripted radio replaces MMIO/DMA; entropy and the clock are deterministic.
Firmware initialization, scan and association/key lifecycles run without hardware.
RustCrypto dependencies are cached/offline in a temporary host Cargo crate.
"""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
source = r'''
#![allow(dead_code)]
extern crate alloc;
extern crate self as trueos_time;
use std::cell::Cell;
thread_local! {static NOW:Cell<u64>=const {Cell::new(0)};}
pub struct Instant(u64);
impl Instant {
    pub fn now()->Self {Self(NOW.with(|value|value.get()))}
    pub fn as_millis(&self)->u64 {self.0}
}
#[macro_export] macro_rules! log_warn {
    (target:$target:literal; $($args:tt)*)=>{{ let _=format_args!($($args)*); }}
}
mod tyche {
    pub fn fill_bytes(bytes:&mut[u8])->bool {bytes.fill(0x22);true}
}
mod net {
    #[path="STATION"] pub(crate) mod iwl_ax201_station;
    #[path="WPA2"] pub(crate) mod iwl_ax201_wpa2;
    #[path="LINK"] pub(crate) mod iwl_ax201_link;
    pub(crate) mod wifi {
        #[derive(Clone,Copy,Debug,PartialEq,Eq)] pub enum WifiState {NoHardware,Disabled,Disconnected,Scanning,Connecting,Authenticating,Connected,Failed}
        #[derive(Clone,Copy,Debug,PartialEq,Eq)] pub enum WifiSecurity {Open,WEP,WPA,WPA2,WPA3,Unknown}
        #[derive(Clone,Debug)] pub struct WifiNetwork {
            pub ssid:String,pub bssid:[u8;6],pub channel:u8,pub signal_dbm:i8,
            pub security:WifiSecurity,pub frequency_mhz:u16,
        }
    }
    #[path="FIRMWARE"] pub(crate) mod iwl_ax201_fw;
    pub(crate) mod iwl_ax201_transport {
        use std::{collections::VecDeque,sync::{Arc,Mutex}};
        #[path="DATA"] pub(crate) mod data;
        #[derive(Clone,Copy,Debug)] pub struct TxOptions {pub band_2ghz:bool,pub antenna:u8,pub encrypt:bool,pub high_priority:bool}
        #[derive(Clone,Debug)] pub struct RawTx {pub queue:u16,pub sequence:u16,pub frame:Vec<u8>,pub options:TxOptions}
        #[derive(Clone,Debug)] pub struct Notification {
            pub group:u8,pub id:u8,pub sequence:u16,pub payload:Vec<u8>,
        }
        #[derive(Clone,Debug)] pub struct Command {
            pub group:u8,pub id:u8,pub version:u8,pub sequence:u16,pub payload:Vec<u8>,
        }
        pub struct State {
            pub alive:Option<Notification>, pub incoming:VecDeque<Notification>,pub sent:Vec<Command>,pub sequence:u16,pub raw:Vec<RawTx>,pub queue_prepared:Vec<(u8,u8)>,pub active_queues:Vec<u16>,
        }
        pub type Shared=Arc<Mutex<State>>;
        pub struct Transport(pub Shared);
        impl Transport {
            pub fn start(&mut self)->Result<Notification,&'static str>{self.0.lock().unwrap().alive.take().ok_or("no alive")}
            pub fn send_command(&mut self,group:u8,id:u8,version:u8,payload:&[u8])->Result<u16,&'static str>{
                let mut state=self.0.lock().unwrap();let sequence=state.sequence;state.sequence+=1;
                state.sent.push(Command{group:if group==0{1}else{group},id,version,sequence,payload:payload.to_vec()});Ok(sequence)
            }
            pub fn poll(&mut self)->Result<Option<Notification>,&'static str>{Ok(self.0.lock().unwrap().incoming.pop_front())}
            pub fn prepare_tx_queue(&mut self,station:u8,tid:u8)->Result<Vec<u8>,&'static str>{
                self.0.lock().unwrap().queue_prepared.push((station,tid));Ok(vec![station,tid])
            }
            pub fn activate_tx_queue(&mut self,payload:&[u8])->Result<u16,&'static str>{
                if payload.len()<2{return Err("mock bad queue response");}
                let queue=u16::from_le_bytes([payload[0],payload[1]]);self.0.lock().unwrap().active_queues.push(queue);Ok(queue)
            }
            pub fn send_raw80211(&mut self,queue:u16,frame:&[u8],options:TxOptions)->Result<u16,&'static str>{
                let mut state=self.0.lock().unwrap();if !state.active_queues.contains(&queue){return Err("mock queue inactive");}
                let sequence=state.sequence;state.sequence+=1;state.raw.push(RawTx{queue,sequence,frame:frame.to_vec(),options});Ok(sequence)
            }
        }
    }
    #[path="MVM"] mod iwl_ax201_mvm;
    #[cfg(test)] mod lifecycle_tests {
        use super::{iwl_ax201_fw as fw,iwl_ax201_transport::{Transport,State,Shared,Notification,Command},iwl_ax201_mvm::Runtime};
        use std::{sync::{Arc,Mutex},collections::VecDeque};
        fn fixture()->(Runtime,Shared){fixture_with_legacy_dqa(false)}
        fn fixture_with_legacy_dqa(legacy_dqa:bool)->(Runtime,Shared){
            crate::NOW.with(|value|value.set(0));
            let shared=Arc::new(Mutex::new(State{
                alive:Some(Notification{group:0,id:1,sequence:0x8000,payload:vec![0xfe,0xca,0,0]}),
                incoming:VecDeque::new(),sent:Vec::new(),sequence:0,raw:Vec::new(),queue_prepared:Vec::new(),active_queues:Vec::new(),
            }));
            let image=if legacy_dqa {
                // A synthetic capability on the otherwise real image exercises
                // the optional legacy command without changing production data.
                let mut bytes=fw::EMBEDDED.to_vec();
                bytes.extend_from_slice(&30u32.to_le_bytes());
                bytes.extend_from_slice(&8u32.to_le_bytes());
                bytes.extend_from_slice(&0u32.to_le_bytes());
                bytes.extend_from_slice(&(1u32<<12).to_le_bytes());
                &*Box::leak(bytes.into_boxed_slice())
            }else{fw::EMBEDDED};
            let firmware=fw::parse(image).unwrap();assert_eq!(firmware.has_capability(12),legacy_dqa);
            let runtime=Runtime::start(Transport(shared.clone()),firmware).unwrap();
            (runtime,shared)
        }
        fn latest(shared:&Shared)->Command{shared.lock().unwrap().sent.last().unwrap().clone()}
        fn inject(shared:&Shared,group:u8,id:u8,sequence:u16,payload:Vec<u8>){
            shared.lock().unwrap().incoming.push_back(Notification{group,id,sequence,payload});
        }
        fn ack(shared:&Shared){let command=latest(shared);inject(shared,command.group,command.id,command.sequence,vec![]);}
        fn complete(shared:&Shared){inject(shared,0,4,0x8000,vec![]);}
        fn nvm()->Vec<u8>{
            let mut bytes=vec![0;468];
            bytes[4..6].copy_from_slice(&0x1234u16.to_le_bytes());bytes[7]=3;
            bytes[8..12].copy_from_slice(&0x1fu32.to_le_bytes());
            bytes[12..16].copy_from_slice(&3u32.to_le_bytes());
            bytes[16..20].copy_from_slice(&3u32.to_le_bytes());
            bytes[20..24].copy_from_slice(&1u32.to_le_bytes());
            bytes[24..28].copy_from_slice(&2u32.to_le_bytes());
            bytes[28..32].copy_from_slice(&0x12345678u32.to_le_bytes());
            bytes[32..36].copy_from_slice(&0x90abcdefu32.to_le_bytes());bytes
        }
        fn to_init_complete(runtime:&mut Runtime,shared:&Shared){
            ack(shared);assert!(!runtime.poll().unwrap());
            assert_eq!((latest(shared).group,latest(shared).id),(12,0));
            ack(shared);assert!(!runtime.poll().unwrap());
            // Unified QuZ with normal AX201 config skips PHY_CONFIGURATION_CMD.
            assert_eq!(shared.lock().unwrap().sent.len(),2);
        }
        fn finish_radio_bootstrap(runtime:&mut Runtime,shared:&Shared){
            let antenna=latest(shared);
            assert_eq!((antenna.group,antenna.id,antenna.version),(1,0x98,0));
            assert_eq!(antenna.payload,3u32.to_le_bytes());
            assert!(runtime.begin_scan([2,1,2,3,4,5]).is_err());
            // Actual QuZ API77 lacks CAPA12. Its antenna acknowledgement
            // immediately makes the gen2 radio ready; no legacy DQA is sent.
            let count=shared.lock().unwrap().sent.len();
            ack(shared);assert!(runtime.poll().unwrap());
            assert_eq!(shared.lock().unwrap().sent.len(),count);
            assert!(!shared.lock().unwrap().sent.iter().any(|command|(command.group,command.id)==(5,0)));
        }
        #[test] fn unified_init_ack_and_nvm_ready(){
            let(mut runtime,shared)=fixture();
            let command=latest(&shared);
            assert_eq!((command.group,command.id),(2,3));
            assert_eq!(command.payload,2u32.to_le_bytes());
            assert_eq!(command.version,fw::parse(fw::EMBEDDED).unwrap().command_version(2,3).unwrap_or(0));
            to_init_complete(&mut runtime,&shared);
            complete(&shared);assert!(!runtime.poll().unwrap());
            let command=latest(&shared);assert_eq!((command.group,command.id),(12,2));
            assert_eq!(command.payload,[0;4]);
            inject(&shared,command.group,command.id,command.sequence,nvm());
            assert!(!runtime.poll().unwrap());
            finish_radio_bootstrap(&mut runtime,&shared);
            let info=runtime.nvm.as_ref().unwrap();assert_eq!(info.version,0x1234);
            assert_eq!(info.reserved_macs,3);assert_eq!(info.tx_chains,3);assert_eq!(info.rx_chains,3);
            assert_eq!(info.channels,[0x12345678,0x90abcdef]);
            assert!(runtime.poll().unwrap());
        }
        #[test] fn early_init_complete_is_retained_until_command_acks(){
            let(mut runtime,shared)=fixture();
            complete(&shared);
            assert!(!runtime.poll().unwrap());assert_eq!(shared.lock().unwrap().sent.len(),1);
            ack(&shared);assert!(!runtime.poll().unwrap());
            ack(&shared);assert!(!runtime.poll().unwrap());
            assert_eq!((latest(&shared).group,latest(&shared).id),(12,2));
            let nvm_command=latest(&shared);
            inject(&shared,12,2,nvm_command.sequence,nvm());
            assert!(!runtime.poll().unwrap());
            finish_radio_bootstrap(&mut runtime,&shared);
        }
        #[test] fn mismatched_response_does_not_advance_initialization(){
            let(mut runtime,shared)=fixture();let command=latest(&shared);
            inject(&shared,99,99,command.sequence,vec![]);
            assert_eq!(runtime.poll().unwrap_err(),"AX201 firmware command response mismatch");
            assert_eq!(shared.lock().unwrap().sent.len(),1);assert!(runtime.nvm.is_none());
        }
        #[test] fn command_timeout_fails_once_and_never_claims_ready(){
            let(mut runtime,_)=fixture();crate::NOW.with(|value|value.set(2000));
            assert_eq!(runtime.poll().unwrap_err(),"AX201 initialization command timeout");
            assert!(!runtime.poll().unwrap());assert!(runtime.nvm.is_none());
        }
        #[test] fn init_complete_timeout_is_separate_from_command_ack(){
            let(mut runtime,shared)=fixture();to_init_complete(&mut runtime,&shared);
            crate::NOW.with(|value|value.set(2001));
            assert_eq!(runtime.poll().unwrap_err(),"AX201 initialization command timeout");
        }
        #[test] fn explicit_firmware_error_stops_initialization(){
            let(mut runtime,shared)=fixture();inject(&shared,0,2,0x8000,vec![0;8]);
            assert_eq!(runtime.poll().unwrap_err(),"AX201 firmware REPLY_ERROR");
            assert!(!runtime.poll().unwrap());
        }
        #[test] fn legacy_dqa_is_sent_only_when_capability_is_advertised(){
            let(mut runtime,shared)=fixture_with_legacy_dqa(true);to_init_complete(&mut runtime,&shared);
            complete(&shared);assert!(!runtime.poll().unwrap());let command=latest(&shared);
            let mut data=nvm();
            data[24..28].copy_from_slice(&110u32.to_le_bytes());
            data[28..32].copy_from_slice(&1u32.to_le_bytes());
            data[32..36].copy_from_slice(&1u32.to_le_bytes());
            inject(&shared,command.group,command.id,command.sequence,data);
            assert!(!runtime.poll().unwrap());
            let antenna=latest(&shared);assert_eq!((antenna.group,antenna.id,antenna.version),(1,0x98,0));
            ack(&shared);assert!(!runtime.poll().unwrap());
            let dqa=latest(&shared);assert_eq!((dqa.group,dqa.id,dqa.version),(5,0,0));
            assert_eq!(dqa.payload,[0;4]);assert!(runtime.begin_scan([2,1,2,3,4,5]).is_err());
            ack(&shared);assert!(runtime.poll().unwrap());
            assert!(runtime.begin_scan([2,1,2,3,4,5]).is_ok());
        }
        #[test] fn radio_bootstrap_times_out_until_each_required_command_is_acknowledged(){
            // The real image requires TX antenna only; a legacy-DQA image
            // additionally requires the DQA acknowledgement before readiness.
            for (legacy_dqa,antenna_ack) in [(false,false),(true,false),(true,true)] {
                let(mut runtime,shared)=fixture_with_legacy_dqa(legacy_dqa);to_init_complete(&mut runtime,&shared);
                complete(&shared);assert!(!runtime.poll().unwrap());let command=latest(&shared);
                inject(&shared,command.group,command.id,command.sequence,nvm());
                assert!(!runtime.poll().unwrap());
                if antenna_ack { ack(&shared);assert!(!runtime.poll().unwrap()); }
                assert!(runtime.begin_scan([2,1,2,3,4,5]).is_err());
                crate::NOW.with(|value|value.set(2001));
                assert_eq!(runtime.poll().unwrap_err(),"AX201 initialization command timeout");
                crate::NOW.with(|value|value.set(0));
            }
        }
        fn scan_ready()->(Runtime,Shared){
            let(mut runtime,shared)=fixture();to_init_complete(&mut runtime,&shared);
            complete(&shared);assert!(!runtime.poll().unwrap());let command=latest(&shared);
            let mut data=nvm();
            data[24..28].copy_from_slice(&110u32.to_le_bytes());
            data[28..32].copy_from_slice(&1u32.to_le_bytes());
            data[32..36].copy_from_slice(&1u32.to_le_bytes());
            inject(&shared,command.group,command.id,command.sequence,data);
            assert!(!runtime.poll().unwrap());
            finish_radio_bootstrap(&mut runtime,&shared);(runtime,shared)
        }
        fn begin_scan(runtime:&mut Runtime,shared:&Shared){
            runtime.begin_scan([2,1,2,3,4,5]).unwrap();
            let config=latest(shared);assert_eq!((config.group,config.id,config.version),(1,0x0c,5));
            ack(shared);assert!(runtime.poll().unwrap());
            let request=latest(shared);assert_eq!((request.group,request.id,request.version),(1,0x0d,15));
            ack(shared);assert!(runtime.poll().unwrap());assert!(!runtime.scan_finished());
        }
        fn scan_complete(shared:&Shared,status:u8,uid:u32){
            let mut data=vec![0;16];data[..4].copy_from_slice(&uid.to_le_bytes());data[6]=status;
            inject(shared,0,0x0f,0x8000,data);
        }
        #[test] fn passive_scan_completes_and_collects_beacon(){
            let(mut runtime,shared)=scan_ready();begin_scan(&mut runtime,&shared);
            let mut frame=vec![0;36];frame[0]=0x80;frame[16..22].copy_from_slice(&[2,3,4,5,6,7]);
            frame.extend_from_slice(&[0,8]);frame.extend_from_slice(b"easyeasy");
            let mut packet=vec![0;48];packet[..2].copy_from_slice(&(frame.len() as u16).to_le_bytes());
            packet[3]=12;packet[12]=3;packet[32]=40;packet[33]=45;packet[34]=1;packet.extend_from_slice(&frame);
            inject(&shared,0,0xc1,0x8000,packet);scan_complete(&shared,1,1);
            assert!(runtime.poll().unwrap());assert!(runtime.scan_finished());
            let networks=runtime.scan_results();assert_eq!(networks.len(),1);assert_eq!(networks[0].ssid,"easyeasy");
            assert_eq!(networks[0].security,super::wifi::WifiSecurity::Open);
            assert_eq!(networks[0].channel,1);
        }
        #[test] fn aborted_and_malformed_scan_completion_fail_closed(){
            for status in [0,2]{
                let(mut runtime,shared)=scan_ready();begin_scan(&mut runtime,&shared);
                scan_complete(&shared,status,1);assert!(runtime.poll().is_err());assert!(!runtime.scan_finished());
            }
            let(mut runtime,shared)=scan_ready();begin_scan(&mut runtime,&shared);
            scan_complete(&shared,1,2);assert_eq!(runtime.poll().unwrap_err(),"AX201 unexpected scan UID");
        }
        #[test] fn passive_scan_timeout_is_bounded(){
            let(mut runtime,shared)=scan_ready();begin_scan(&mut runtime,&shared);
            crate::NOW.with(|value|value.set(30001));assert!(runtime.poll().is_err());assert!(!runtime.scan_finished());
        }
        #[test] fn malformed_nvm_never_becomes_ready(){
            let(mut runtime,shared)=fixture();to_init_complete(&mut runtime,&shared);
            complete(&shared);assert!(!runtime.poll().unwrap());let command=latest(&shared);
            inject(&shared,command.group,command.id,command.sequence,vec![0;7]);
            assert!(runtime.poll().is_err());assert!(runtime.nvm.is_none());assert!(!runtime.poll().unwrap());
        }
    }
}
'''
source += r'''
#[cfg(test)]
mod link_lifecycle_tests {
    use crate::net::{iwl_ax201_fw as fw,iwl_ax201_station as station,
        iwl_ax201_link::Connection,iwl_ax201_transport::{Transport,State,Shared,Notification,RawTx},
        iwl_ax201_wpa2::{derive_pmk,CcmpKey},wifi::WifiState};
    use std::{sync::{Arc,Mutex},collections::VecDeque};
    use aes::Aes128;
    use cipher::{BlockEncrypt,KeyInit,generic_array::GenericArray};
    use hmac::{Hmac,Mac};
    use sha1::Sha1;
    const STA:[u8;6]=[2,0,0,0,0,1];
    const AP:[u8;6]=[2,0,0,0,0,2];
    const RSN:[u8;22]=[48,20,1,0,0,15,172,4,1,0,0,15,172,4,1,0,0,15,172,2,0,0];
    const PSK:&[u8]=b"000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f";
    struct Fixture { fw:fw::Firmware<'static>,config:station::StationConfig,connection:Connection,transport:Transport,shared:Shared }
    fn fixture(protected:bool)->Fixture {
        crate::NOW.with(|clock|clock.set(0));
        let firmware=fw::parse(fw::EMBEDDED).unwrap();
        let config=station::StationConfig{mac:STA,bss:station::Bss{
            ssid:"easyeasy".into(),bssid:AP,channel:6,beacon_interval:100,capability:0x431,
            dtim_count:0,dtim_period:2,timestamp:0,rates:vec![0x82,0x84,0x8b,0x96,12,24,48,108],
            rsn_ie:if protected {RSN.to_vec()} else {vec![]},
        },tx_chains:3,rx_chains:3,device_timestamp:0,host_decrypt:true};
        let connection=Connection::new(&firmware,config.clone(),if protected{PSK}else{b""},if protected{&RSN}else{&[]},-42).unwrap();
        let shared=Arc::new(Mutex::new(State{alive:None,incoming:VecDeque::new(),sent:vec![],sequence:0,
            raw:vec![],queue_prepared:vec![],active_queues:vec![]}));
        Fixture{fw:firmware,config,connection,transport:Transport(shared.clone()),shared}
    }
    impl Fixture {
        fn notify(&mut self,group:u8,id:u8,sequence:u16,payload:Vec<u8>){
            self.connection.notification(&mut self.transport,&self.fw,&Notification{group,id,sequence,payload});
        }
        fn poll(&mut self){self.connection.poll(&mut self.transport);}
        fn ack(&mut self,response:station::Response){
            let cmd=self.shared.lock().unwrap().sent.last().unwrap().clone();
            let payload=match response{station::Response::Transport=>vec![],station::Response::ZeroStatus=>vec![0;4],station::Response::AddStation=>1u32.to_le_bytes().to_vec()};
            self.notify(cmd.group,cmd.id,cmd.sequence,payload);
        }
        fn configure(&mut self,session:bool){
            for command in station::prepare(&self.fw,&self.config).unwrap(){
                self.poll();let sent=self.shared.lock().unwrap().sent.last().unwrap().clone();
                assert_eq!((sent.group,sent.id,sent.version),(command.group,command.id,command.version));
                self.ack(command.response);
            }
            self.poll();
            for queue in [1u16,2]{
                let cmd=self.shared.lock().unwrap().sent.last().unwrap().clone();assert_eq!((cmd.group,cmd.id),(1,0x1d));
                self.notify(1,0x1d,cmd.sequence,queue.to_le_bytes().to_vec());
            }
            if session {
                let mut body=vec![0;16];body[4]=1;body[8]=1;self.notify(3,0xfb,0x8000,body);
            }
            self.poll();
        }
        fn raw(&self)->RawTx{self.shared.lock().unwrap().raw.last().unwrap().clone()}
        fn frame(&mut self,raw:Vec<u8>){
            let mut payload=vec![0;48];payload[..2].copy_from_slice(&(raw.len() as u16).to_le_bytes());
            payload[3]=12;payload[12]=3;payload[32]=42;payload[33]=50;payload[34]=6;
            payload.extend_from_slice(&raw);self.notify(0,0xc1,0x8000,payload);
        }
        fn management(&mut self,subtype:u8,body:&[u8]){
            let mut frame=vec![0;24];frame[0]=subtype;frame[4..10].copy_from_slice(&STA);
            frame[10..16].copy_from_slice(&AP);frame[16..22].copy_from_slice(&AP);frame.extend_from_slice(body);self.frame(frame);
        }
        fn associate(&mut self){
            self.configure(true);assert_eq!(self.raw().frame[0],0xb0);
            self.management(0xb0,&[0,0,2,0,0,0]);assert_eq!(self.raw().frame[0],0);
            self.management(0x10,&[0x31,4,0,0,1,0xc0]);
            for command in station::associated(&self.fw,&self.config,1).unwrap(){self.poll();self.ack(command.response);}
            self.poll();
        }
        fn eapol(&mut self,bytes:&[u8]){self.frame(downlink([0x88,0x8e],AP,bytes));}
        fn tx_result(&mut self,tx:&RawTx,status:u16){
            let mut payload=vec![0;48];payload[0]=1;payload[36..38].copy_from_slice(&tx.queue.to_le_bytes());
            payload[40..42].copy_from_slice(&status.to_le_bytes());payload[42..44].copy_from_slice(&tx.sequence.to_le_bytes());
            payload[44..48].copy_from_slice(&(u32::from(tx.sequence)+1).to_le_bytes());self.notify(1,0x1c,0x8000,payload);
        }
    }
    fn downlink(ether_type:[u8;2],source:[u8;6],payload:&[u8])->Vec<u8>{
        let mut frame=vec![0;24];frame[..2].copy_from_slice(&0x0208u16.to_le_bytes());
        frame[4..10].copy_from_slice(&STA);frame[10..16].copy_from_slice(&AP);frame[16..22].copy_from_slice(&source);
        frame.extend_from_slice(&[0xaa,0xaa,3,0,0,0]);frame.extend_from_slice(&ether_type);frame.extend_from_slice(payload);frame
    }
    fn ethernet()->Vec<u8>{let mut f=vec![0;60];f[..6].copy_from_slice(&AP);f[6..12].copy_from_slice(&STA);f[12..14].copy_from_slice(&[8,0]);f}
    fn hmac(key:&[u8],parts:&[&[u8]])->[u8;20]{let mut h=<Hmac<Sha1> as Mac>::new_from_slice(key).unwrap();for p in parts{h.update(p);}h.finalize().into_bytes().into()}
    fn ptk()->[u8;48]{
        let pmk=derive_pmk(b"easyeasy",PSK).unwrap();let mut out=[0;48];
        for n in 0..3u8{let hash=hmac(&pmk,&[b"Pairwise key expansion",&[0],&STA,&AP,&[0x11;32],&[0x22;32],&[n]]);let at=n as usize*20;let count=(48-at).min(20);out[at..at+count].copy_from_slice(&hash[..count]);}out
    }
    fn message1()->Vec<u8>{
        let mut packet=vec![0;99];packet[0]=2;packet[1]=3;packet[2..4].copy_from_slice(&95u16.to_be_bytes());packet[4]=2;
        packet[5..7].copy_from_slice(&(2u16|8|128).to_be_bytes());packet[7..9].copy_from_slice(&16u16.to_be_bytes());
        packet[9..17].copy_from_slice(&1u64.to_be_bytes());packet[17..49].fill(0x11);packet
    }
    fn wrapped(key:&[u8],plain:&[u8])->Vec<u8>{
        let cipher=Aes128::new(GenericArray::from_slice(key));let n=plain.len()/8;let mut out=vec![0;plain.len()+8];out[..8].fill(0xa6);out[8..].copy_from_slice(plain);
        for j in 0..6{for i in 1..=n{let mut block=[0;16];block[..8].copy_from_slice(&out[..8]);block[8..].copy_from_slice(&out[i*8..i*8+8]);cipher.encrypt_block(GenericArray::from_mut_slice(&mut block));
            let a=u64::from_be_bytes(block[..8].try_into().unwrap())^((n*j+i)as u64);out[..8].copy_from_slice(&a.to_be_bytes());out[i*8..i*8+8].copy_from_slice(&block[8..]);}}out
    }
    fn message3()->Vec<u8>{message3_with_gtk(1)}
    fn message3_with_gtk(id:u8)->Vec<u8>{
        let ptk=ptk();let mut key_data=RSN.to_vec();key_data.extend_from_slice(&[0xdd,22,0,15,172,1,id,0]);key_data.extend_from_slice(&[0x77;16]);key_data.extend_from_slice(&[0xdd,0]);
        let key_data=wrapped(&ptk[16..32],&key_data);let mut packet=message1();
        packet[5..7].copy_from_slice(&(2u16|8|64|128|256|512|4096).to_be_bytes());packet[9..17].copy_from_slice(&2u64.to_be_bytes());packet[97..99].copy_from_slice(&(key_data.len()as u16).to_be_bytes());packet.extend_from_slice(&key_data);
        let size=(packet.len()-4)as u16;packet[2..4].copy_from_slice(&size.to_be_bytes());let tag=hmac(&ptk[..16],&[&packet]);packet[81..97].copy_from_slice(&tag[..16]);packet
    }
    #[test]fn firmware_acks_alone_never_open_controlled_port(){
        let mut f=fixture(true);f.configure(false);assert!(!f.connection.connected());assert!(f.shared.lock().unwrap().raw.is_empty());
        assert!(f.connection.send(&mut f.transport,&ethernet()).is_err());
        crate::NOW.with(|clock|clock.set(2001));f.poll();assert_eq!(f.connection.state(),WifiState::Failed);
        assert_eq!(f.connection.last_error(),Some("AX201 station protocol timeout"));
    }
    #[test]fn ap_auth_rejection_closes_station(){
        let mut f=fixture(true);f.configure(true);f.management(0xb0,&[0,0,2,0,1,0]);
        assert_eq!(f.connection.state(),WifiState::Failed);assert!(!f.connection.connected());
        assert!(f.connection.send(&mut f.transport,&ethernet()).is_err());
    }
    #[test]fn open_association_routes_ethernet_and_disconnect_closes_port(){
        let mut f=fixture(false);f.associate();assert!(f.connection.connected());
        f.connection.send(&mut f.transport,&ethernet()).unwrap();assert_eq!(f.raw().queue,2);assert!(!f.raw().options.encrypt);
        let payload=[0x55;46];f.frame(downlink([8,0],AP,&payload));
        let packet=f.connection.receive().unwrap();assert_eq!(&packet[..6],&STA);assert_eq!(&packet[6..12],&AP);assert_eq!(&packet[12..14],&[8,0]);assert_eq!(&packet[14..],&payload);
        f.connection.disconnect();assert!(!f.connection.connected());assert!(f.connection.send(&mut f.transport,&ethernet()).is_err());
    }
    #[test]fn wpa2_stays_closed_until_valid_m3_and_matching_m4_radio_ack(){
        let mut f=fixture(true);f.associate();assert_eq!(f.connection.state(),WifiState::Authenticating);
        assert!(f.connection.send(&mut f.transport,&ethernet()).is_err());f.eapol(&message1());
        let m2=f.raw();assert_eq!(u16::from_be_bytes([m2.frame[37],m2.frame[38]])&256,256);assert!(!f.connection.connected());
        let mut bad=message3();bad[82]^=1;let count=f.shared.lock().unwrap().raw.len();f.eapol(&bad);
        assert_eq!(f.shared.lock().unwrap().raw.len(),count);assert!(!f.connection.connected());
        f.eapol(&message3());let m4=f.raw();assert!(!f.connection.connected());assert!(f.connection.send(&mut f.transport,&ethernet()).is_err());
        let mut unrelated=m4.clone();unrelated.queue=1;f.tx_result(&unrelated,1);assert!(!f.connection.connected());
        let mut unrelated=m4.clone();unrelated.sequence+=1;f.tx_result(&unrelated,1);assert!(!f.connection.connected());
        f.tx_result(&m4,1);assert!(f.connection.connected());
        f.connection.send(&mut f.transport,&ethernet()).unwrap();let tx=f.raw();assert_eq!(tx.queue,2);assert!(tx.frame[1]&0x40!=0);assert!(!tx.options.encrypt);
        let key:[u8;16]=ptk()[32..48].try_into().unwrap();let mut ap_key=CcmpKey::new(key,0,STA).unwrap();
        let packet=ap_key.encrypt(&downlink([8,0],AP,&[0x66;46])).unwrap();f.frame(packet.clone());assert!(f.connection.receive().is_some());
        f.frame(packet);assert!(f.connection.receive().is_none()); // replay never reaches Ethernet.
    }
    #[test]fn session_end_before_authentication_requires_new_start(){
        let mut f=fixture(true);f.configure(false);
        let mut event=vec![0;16];event[4]=1;event[8]=1;f.notify(3,0xfb,0x8000,event.clone());
        event[8]=0;f.notify(3,0xfb,0x8000,event);f.poll();
        assert!(f.shared.lock().unwrap().raw.is_empty());assert!(!f.connection.connected());
    }
    #[test]fn group_key_cannot_authenticate_unicast_data(){
        let mut f=fixture(true);f.associate();f.eapol(&message1());f.eapol(&message3());let m4=f.raw();f.tx_result(&m4,1);
        assert!(f.connection.connected());
        let mut gtk=CcmpKey::new([0x77;16],1,STA).unwrap();
        let invalid=gtk.encrypt(&downlink([8,0],AP,&[0x99;46])).unwrap();f.frame(invalid);
        assert!(f.connection.receive().is_none());
        let mut multicast=downlink([8,0],AP,&[0x88;46]);multicast[4..10].fill(0xff);
        f.frame(gtk.encrypt(&multicast).unwrap());let ethernet=f.connection.receive().unwrap();
        assert_eq!(&ethernet[..6],&[0xff;6]);assert_eq!(&ethernet[14..],&[0x88;46]);
    }
    #[test]fn group_key_id_zero_is_selected_by_multicast_receiver(){
        let mut f=fixture(true);f.associate();f.eapol(&message1());f.eapol(&message3_with_gtk(0));let m4=f.raw();f.tx_result(&m4,1);
        assert!(f.connection.connected());let mut gtk=CcmpKey::new([0x77;16],0,STA).unwrap();
        let mut multicast=downlink([8,0],AP,&[0x77;46]);multicast[4..10].fill(0xff);
        let frame=gtk.encrypt(&multicast).unwrap();f.frame(frame.clone());
        let ethernet=f.connection.receive().unwrap();assert_eq!(&ethernet[..6],&[0xff;6]);assert_eq!(&ethernet[14..],&[0x77;46]);
        f.frame(frame);assert!(f.connection.receive().is_none());
    }
    #[test]fn m4_radio_failure_never_opens_port(){
        let mut f=fixture(true);f.associate();f.eapol(&message1());f.eapol(&message3());let m4=f.raw();f.tx_result(&m4,0x82);
        assert_eq!(f.connection.state(),WifiState::Failed);assert!(!f.connection.connected());assert!(f.connection.send(&mut f.transport,&ethernet()).is_err());
    }
}
'''
for marker, name in [('FIRMWARE','iwl_ax201_fw.rs'),('MVM','iwl_ax201_mvm.rs'),('STATION','iwl_ax201_station.rs'),('WPA2','iwl_ax201_wpa2.rs'),('LINK','iwl_ax201_link.rs'),('DATA','iwl_ax201_data.rs')]:
    source = source.replace('\"' + marker + '\"', '\"' + str(ROOT / 'src/net' / name) + '\"')
with tempfile.TemporaryDirectory(prefix='trueos-ax201-mvm-') as directory:
    directory = Path(directory)
    (directory / 'src').mkdir()
    (directory / 'Cargo.toml').write_text('''[package]
name = "trueos-ax201-mvm-host-tests"
version = "0.1.0"
edition = "2024"
[dependencies]
hmac = { version = "0.12.1", default-features = false }
sha1 = { version = "=0.10.6", default-features = false }
aes = { version = "0.8.4", default-features = false }
cipher = { version = "0.4.4", default-features = false }
''')
    (directory / 'src/lib.rs').write_text(source)
    subprocess.run(['cargo', 'test', '--target', 'x86_64-unknown-linux-gnu', '--offline'], cwd=directory, check=True)
