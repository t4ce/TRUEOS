#!/usr/bin/env python3
"""Run production AX201 DMA layout, host-command and RX parser contracts.

MMIO is ordinary host memory and the DMA allocator is instrumented. No real
PCI device, firmware execution or successful wireless association is implied.
"""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
source = r'''
#![allow(dead_code, unused_variables)]
extern crate alloc;
#[macro_export] macro_rules! log_info {
    (target:$target:literal; $($args:tt)*)=>{{ let _=format_args!($($args)*); }}
}
#[macro_export] macro_rules! log_warn {
    (target:$target:literal; $($args:tt)*)=>{{ let _=format_args!($($args)*); }}
}
mod dma {
    use std::{alloc::{Layout,alloc_zeroed,dealloc as free},collections::HashMap,sync::{Mutex,OnceLock}};
    fn layouts()->&'static Mutex<HashMap<usize,Layout>> {
        static ALL:OnceLock<Mutex<HashMap<usize,Layout>>>=OnceLock::new();
        ALL.get_or_init(||Mutex::new(HashMap::new()))
    }
    pub fn alloc(bytes:usize,align:usize)->Option<(u64,*mut u8)> {
        let layout=Layout::from_size_align(bytes,align).ok()?;
        let pointer=unsafe{alloc_zeroed(layout)};
        if pointer.is_null(){return None;}
        layouts().lock().unwrap().insert(pointer as usize,layout);
        Some((pointer as u64,pointer))
    }
    pub fn dealloc(pointer:*mut u8,_:usize) {
        let layout=layouts().lock().unwrap().remove(&(pointer as usize)).expect("DMA freed exactly once");
        unsafe{free(pointer,layout)};
    }
}
mod intel { pub fn dma_cache_flush_range(_: *const u8,_:usize){} }
mod pci {
    pub fn config_read_u16(_:u8,_:u8,_:u8,_:u16)->u16{0}
    pub fn config_write_u16(_:u8,_:u8,_:u8,_:u16,_:u16){}
}
mod wait { pub fn spin_until_timeout_no_exec(_:u64,mut f:impl FnMut()->bool)->bool {f()} }
mod net {
    #[path="FIRMWARE"] pub mod iwl_ax201_fw;
    pub mod iwl_ax201_transport {
TRANSPORT
        #[cfg(test)] mod host_contract_tests {
            use super::*;
            fn transport(mmio:&mut [u32])->Transport {
                transport_with_firmware(mmio,crate::net::iwl_ax201_fw::EMBEDDED)
            }
            fn transport_with_firmware(mmio:&mut [u32],image:&[u8])->Transport {
                let firmware=crate::net::iwl_ax201_fw::parse(image).unwrap();
                Transport::new(mmio.as_mut_ptr() as usize,mmio.len()*4,(0,2,0),&firmware).unwrap()
            }
            fn firmware_with_queue_api(version:Option<u8>)->Vec<u8> {
                let mut image=crate::net::iwl_ax201_fw::EMBEDDED.to_vec();
                let mut at=88;
                while at<image.len() {
                    let kind=u32::from_le_bytes(image[at..at+4].try_into().unwrap());
                    let len=u32::from_le_bytes(image[at+4..at+8].try_into().unwrap())as usize;
                    if kind==48 {
                        for entry in image[at+8..at+8+len].chunks_exact_mut(4) {
                            if entry[0]==0x17&&entry[1]==5 {
                                if let Some(version)=version {entry[2]=version;} else {entry[1]=0xff;}
                                return image;
                            }
                        }
                    }
                    at+=8+(len+3)&!3;
                }
                panic!("embedded firmware must advertise scheduler queue API");
            }
            fn qword(bytes:&[u8],at:usize)->u64 {u64::from_le_bytes(bytes[at..at+8].try_into().unwrap())}
            #[test]
            fn context_layout_matches_linux_22000_contract() {
                let mut mmio=vec![0u32;0x2000/4];
                let tr=transport(&mut mmio);
                let c=tr.context.bytes();
                assert_eq!(c.len(),1792);
                assert_eq!(u16::from_le_bytes([c[4],c[5]]),448);
                assert_eq!(qword(c,24),tr.free.phys);
                assert_eq!(qword(c,32),tr.used.phys);
                assert_eq!(qword(c,40),tr.status.phys);
                assert_eq!(qword(c,48),tr.tfd.phys);
                assert_eq!(c[56],2); // 2^(2+3) = 32 host command descriptors
                assert!(c[64..192].iter().all(|&b|b==0));
                assert_ne!(qword(c,192),0); // UMAC
                assert_ne!(qword(c,192+512),0); // LMAC
                assert_ne!(qword(c,192+1024),0); // authenticated paging CSS
                assert!(c[1728..].iter().all(|&b|b==0));
            }
            #[test]
            fn long_command_uses_queue_zero_and_two_transfer_buffers() {
                let mut mmio=vec![0u32;0x2000/4];
                let mut tr=transport(&mut mmio);
                tr.restocked=true;
                let payload=[0xa5u8;32];
                let sequence=tr.send_command(0,0x6a,3,&payload).unwrap();
                assert_eq!(sequence,0);
                assert_eq!(&tr.commands.bytes()[..8],&[0x6a,1,0,0,32,0,0,3]);
                assert_eq!(&tr.commands.bytes()[8..40],&payload);
                let d=tr.tfd.bytes();
                assert_eq!(u16::from_le_bytes([d[0],d[1]]),2);
                assert_eq!(u16::from_le_bytes([d[2],d[3]]),20);
                assert_eq!(qword(d,4),tr.commands.phys);
                assert_eq!(u16::from_le_bytes([d[12],d[13]]),20);
                assert_eq!(qword(d,14),tr.commands.phys+20);
                assert_eq!(mmio[0x460/4],1);
            }
            fn allocate_data(tr:&mut Transport)->u16 {
                tr.restocked=true;
                let config=tr.prepare_tx_queue(1,16).unwrap();
                assert_eq!(tr.queue_allocation_command().unwrap(),(5,0x17,0));
                assert_eq!(config.len(),36);
                assert_eq!(&config[..20],&[0,0,0,0,2,0,0,0,15,0,0,0,0,0,0,0,2,0,0,0]);
                assert_ne!(qword(&config,20),0);assert_ne!(qword(&config,28),0);
                tr.activate_tx_queue(&[4,0,0,0,0,0,0,0]).unwrap()
            }
            #[test]
            fn advertised_queue_v3_uses_datapath_add_body_and_wide_wire_version_zero() {
                let mut mmio=vec![0u32;0x2000/4];let mut tr=transport(&mut mmio);
                assert_eq!(tr.queue_alloc_cmd_version,3);tr.restocked=true;
                let body=tr.prepare_tx_queue(3,16).unwrap();
                assert_eq!(&body[..20],&[0,0,0,0,8,0,0,0,15,0,0,0,0,0,0,0,2,0,0,0]);
                let queue=tr.pending_data_queue.as_ref().unwrap();
                assert_eq!(qword(&body,20),queue.byte_count.phys);
                assert_eq!(qword(&body,28),queue.tfd.phys);
                let (group,id,version)=tr.queue_allocation_command().unwrap();
                tr.send_command(group,id,version,&body).unwrap();
                assert_eq!(&tr.commands.bytes()[..8],&[0x17,5,0,0,36,0,0,0]);
                assert_eq!(&tr.commands.bytes()[8..44],&body);
            }
            #[test]
            fn absent_datapath_queue_metadata_uses_legacy_24_byte_body() {
                let image=firmware_with_queue_api(None);
                let fw=crate::net::iwl_ax201_fw::parse(&image).unwrap();
                assert_eq!(fw.command_version(5,0x17),None);
                let mut mmio=vec![0u32;0x2000/4];let mut tr=transport_with_firmware(&mut mmio,&image);
                assert_eq!(tr.queue_allocation_command().unwrap(),(1,0x1d,0));tr.restocked=true;
                let body=tr.prepare_tx_queue(3,0).unwrap();
                assert_eq!(body.len(),24);assert_eq!(&body[..8],&[3,0,1,0,2,0,0,0]);
                let queue=tr.pending_data_queue.as_ref().unwrap();
                assert_eq!(qword(&body,8),queue.byte_count.phys);
                assert_eq!(qword(&body,16),queue.tfd.phys);
                assert_eq!(tr.activate_tx_queue(&[7,0,0,0,0,0,0,0]).unwrap(),7);
            }
            #[test]
            fn unsupported_queue_metadata_rejects_before_dma_publication() {
                let image=firmware_with_queue_api(Some(4));
                let mut mmio=vec![0u32;0x2000/4];let mut tr=transport_with_firmware(&mut mmio,&image);
                let reason="AX201 unsupported queue allocation command version";
                assert_eq!(tr.queue_allocation_command().unwrap_err(),reason);tr.restocked=true;
                assert_eq!(tr.prepare_tx_queue(1,0).unwrap_err(),reason);
                assert!(tr.pending_data_queue.is_none());assert_eq!(mmio[0x460/4],0);
            }
            fn authentication()->Vec<u8> {
                let mut frame=vec![0;30];frame[0]=0xb0;frame[4..10].fill(2);frame[10..16].fill(4);frame[16..22].fill(2);
                frame[26]=1;frame
            }
            fn tx_notification(sequence:u16,next:u8)->Notification {
                let mut payload=vec![0;48];payload[0]=1;payload[36]=4;payload[40]=1;
                payload[42..44].copy_from_slice(&sequence.to_le_bytes());payload[44]=next;
                Notification{group:0,id:0x1c,sequence,payload}
            }
            #[test]
            fn queue_reply_flags_do_not_override_a_valid_unused_queue_id() {
                let mut mmio=vec![0u32;0x2000/4];let mut tr=transport(&mut mmio);tr.restocked=true;
                tr.prepare_tx_queue(1,16).unwrap();
                assert_eq!(tr.activate_tx_queue(&[4,0,0x42,0,0,0,0,0]).unwrap(),4);
                assert!(tr.pending_data_queue.is_none());assert_eq!(tr.data_queues.len(),1);
                assert_eq!(tr.data_queues[0].read,0);assert_eq!(tr.data_queues[0].write,0);
            }
            #[test]
            fn queue_reply_write_pointer_masks_to_hardware_ring_and_retires_across_wrap() {
                let mut mmio=vec![0u32;0x2000/4];let mut tr=transport(&mut mmio);tr.restocked=true;
                tr.prepare_tx_queue(1,16).unwrap();
                let qid=tr.activate_tx_queue(&[4,0,0x42,0,0xff,0xff,0,0]).unwrap();
                assert_eq!(tr.data_queues[0].read,255);assert_eq!(tr.data_queues[0].write,255);
                let frame=authentication();
                let token=tr.send_raw80211(qid,&frame,TxOptions{band_2ghz:true,antenna:1,encrypt:false,high_priority:true}).unwrap();
                assert_eq!(token,0x41f);assert_eq!(mmio[0x460/4],0x40000);
                let queue=&tr.data_queues[0];
                assert_eq!(queue.write,0);assert_eq!(queue.outstanding,1);
                assert!(queue.busy[31]);assert!(queue.busy[..31].iter().all(|&busy|!busy));
                assert_eq!(&queue.packets.bytes()[31*DATA_BYTES..31*DATA_BYTES+4],&[0x1c,0,0x1f,4]);
                assert_eq!(qword(queue.tfd.bytes(),31*TFD_BYTES+4),queue.packets.phys+(31*DATA_BYTES)as u64);
                tr.retire_tx_notification(&tx_notification(token,0)).unwrap();
                let queue=&tr.data_queues[0];
                assert_eq!(queue.read,0);assert_eq!(queue.write,0);assert_eq!(queue.outstanding,0);
                assert!(queue.busy.iter().all(|&busy|!busy));assert!(tr.tx_queue_idle(qid));
                assert_eq!(tr.tx_completion_status(qid,token),Some(true));
            }
            #[test]
            fn queue_reply_rejects_reserved_out_of_range_and_duplicate_ids() {
                let mut mmio=vec![0u32;0x2000/4];let mut tr=transport(&mut mmio);tr.restocked=true;
                tr.prepare_tx_queue(1,16).unwrap();
                let reserved=tr.activate_tx_queue(&[0,0,0x42,0,0,0,0,0]).unwrap_err();
                let range=tr.activate_tx_queue(&[0,2,0x42,0,0,0,0,0]).unwrap_err();
                assert!(tr.activate_tx_queue(&[0xff,0xff,0,0,0,0,0,0]).is_err());
                assert!(tr.pending_data_queue.is_some());assert!(tr.data_queues.is_empty());
                tr.activate_tx_queue(&[4,0,0,0,0,0,0,0]).unwrap();tr.prepare_tx_queue(1,0).unwrap();
                let duplicate=tr.activate_tx_queue(&[4,0,0x42,0,0,0,0,0]).unwrap_err();
                assert_ne!(reserved,range);assert_ne!(reserved,duplicate);assert_ne!(range,duplicate);
                assert!(tr.pending_data_queue.is_some());assert_eq!(tr.data_queues.len(),1);
            }
            #[test]
            fn queue_reply_requires_exact_length_and_a_pending_allocation() {
                let mut mmio=vec![0u32;0x2000/4];let mut tr=transport(&mut mmio);tr.restocked=true;
                let no_pending=tr.activate_tx_queue(&[4,0,0,0,0,0,0,0]).unwrap_err();
                tr.prepare_tx_queue(1,16).unwrap();
                for bytes in [&[][..],&[4,0,0,0,0,0,0][..],&[4,0,0,0,0,0,0,0,0][..]] {
                    let malformed=tr.activate_tx_queue(bytes).unwrap_err();assert_ne!(malformed,no_pending);
                    assert!(tr.pending_data_queue.is_some());assert!(tr.data_queues.is_empty());
                }
                tr.activate_tx_queue(&[4,0,0,0,0,0,0,0]).unwrap();
            }
            #[test]
            fn data_queue_config_and_frame_tfd_match_gen2_contract() {
                let mut mmio=vec![0u32;0x2000/4];let mut tr=transport(&mut mmio);
                let qid=allocate_data(&mut tr);let frame=authentication();
                let token=tr.send_raw80211(qid,&frame,TxOptions{band_2ghz:true,antenna:1,encrypt:false,high_priority:true}).unwrap();
                assert_eq!(token,0x400);assert_eq!(mmio[0x460/4],0x40001);
                let queue=&tr.data_queues[0];let packet=queue.packets.bytes();
                assert_eq!(&packet[..4],&[0x1c,0,0,4]);
                assert_eq!(u16::from_le_bytes([packet[4],packet[5]]),30);
                assert_eq!(&packet[8..12],&7u32.to_le_bytes());
                assert_eq!(&packet[20..24],&0x4000u32.to_le_bytes());
                assert_eq!(&packet[24..54],&frame);
                let descriptor=queue.tfd.bytes();assert_eq!(u16::from_le_bytes([descriptor[0],descriptor[1]]),3);
                assert_eq!(u16::from_le_bytes([descriptor[2],descriptor[3]]),20);
                assert_eq!(u16::from_le_bytes([descriptor[12],descriptor[13]]),28);
                assert_eq!(u16::from_le_bytes([descriptor[22],descriptor[23]]),6);
                assert_eq!(queue.byte_count.bytes()[0],8);
                assert!(queue.busy[0]);
                tr.retire_tx_notification(&tx_notification(token,1)).unwrap();
                assert!(!tr.data_queues[0].busy[0]);assert_eq!(tr.data_queues[0].outstanding,0);
            }
            #[test]
            fn data_slot_cannot_be_reused_until_matching_retirement() {
                let mut mmio=vec![0u32;0x2000/4];let mut tr=transport(&mut mmio);
                let qid=allocate_data(&mut tr);let frame=authentication();
                let options=TxOptions{band_2ghz:false,antenna:2,encrypt:false,high_priority:true};
                for _ in 0..32{tr.send_raw80211(qid,&frame,options).unwrap();}
                assert_eq!(tr.send_raw80211(qid,&frame,options).unwrap_err(),"AX201 TX queue full");
                let mut wrong_queue=tx_notification(0x400,1);wrong_queue.payload[36]=5;
                assert!(tr.retire_tx_notification(&wrong_queue).is_err());
                assert!(tr.retire_tx_notification(&tx_notification(0x400,33)).is_err());
                assert_eq!(tr.data_queues[0].outstanding,32);
                tr.retire_tx_notification(&tx_notification(0x400,1)).unwrap();
                tr.send_raw80211(qid,&frame,options).unwrap();
                assert_eq!(tr.data_queues[0].outstanding,32);
                assert_eq!(&tr.data_queues[0].packets.bytes()[20..24],&0x8100u32.to_le_bytes());
            }
            #[test]
            fn tx_retirement_uses_owned_queue_and_ssn_instead_of_aggregate_sequence() {
                let mut mmio=vec![0u32;0x2000/4];let mut tr=transport(&mut mmio);
                let qid=allocate_data(&mut tr);let frame=authentication();
                let token=tr.send_raw80211(qid,&frame,TxOptions{band_2ghz:true,antenna:1,encrypt:false,high_priority:true}).unwrap();
                assert_eq!(tr.tx_completion_status(qid,token),None);
                let mut report=tx_notification(0xffff,1);report.sequence=0xbeef;
                tr.retire_tx_notification(&report).unwrap();
                assert_eq!(tr.tx_completion_status(qid,token),Some(true));
                assert_eq!(tr.tx_completion_status(qid+32,token),None);
                assert_eq!(tr.tx_completion_status(qid,0x500),None);
                assert_eq!(tr.tx_completion_status(qid,0x401),None);
                assert_eq!(tr.tx_completion_status(qid,0x420),None);
                assert_eq!(tr.tx_completion_status(qid,0x8400),None);
                assert!(tr.retire_tx_notification(&report).is_err());
                assert_eq!(tr.tx_completion_status(qid,token),None);
            }
            #[test]
            fn cumulative_retirement_applies_status_to_first_owned_frame_only() {
                let mut mmio=vec![0u32;0x2000/4];let mut tr=transport(&mut mmio);
                let qid=allocate_data(&mut tr);let frame=authentication();
                let options=TxOptions{band_2ghz:true,antenna:1,encrypt:false,high_priority:true};
                let tokens=(0..3).map(|_|tr.send_raw80211(qid,&frame,options).unwrap()).collect::<Vec<_>>();
                let mut report=tx_notification(0xffff,3);report.payload[40]=0x83;
                tr.retire_tx_notification(&report).unwrap();
                assert_eq!(tr.tx_completion_status(qid,tokens[0]),Some(false));
                assert_eq!(tr.tx_completion_status(qid,tokens[1]),Some(true));
                assert_eq!(tr.tx_completion_status(qid,tokens[2]),Some(true));
                assert_eq!(tr.tx_completion_status(qid,0x403),None);assert!(tr.tx_queue_idle(qid));
            }
            #[test]
            fn cumulative_retirement_validates_all_ownership_before_mutating() {
                let mut mmio=vec![0u32;0x2000/4];let mut tr=transport(&mut mmio);
                let qid=allocate_data(&mut tr);let frame=authentication();
                let options=TxOptions{band_2ghz:true,antenna:1,encrypt:false,high_priority:true};
                let token=tr.send_raw80211(qid,&frame,options).unwrap();tr.send_raw80211(qid,&frame,options).unwrap();
                tr.data_queues[0].busy[1]=false;
                assert!(tr.retire_tx_notification(&tx_notification(0xffff,2)).is_err());
                let queue=&tr.data_queues[0];
                assert_eq!(queue.read,0);assert_eq!(queue.write,2);assert_eq!(queue.outstanding,2);
                assert!(queue.busy[0]);assert!(!queue.busy[1]);assert_eq!(tr.tx_completion_status(qid,token),None);
            }
            #[test]
            fn invalid_ssn_cannot_complete_or_release_an_owned_frame() {
                let mut mmio=vec![0u32;0x2000/4];let mut tr=transport(&mut mmio);
                let qid=allocate_data(&mut tr);let frame=authentication();
                let token=tr.send_raw80211(qid,&frame,TxOptions{band_2ghz:true,antenna:1,encrypt:false,high_priority:true}).unwrap();
                for next in [0,2,255] {
                    assert!(tr.retire_tx_notification(&tx_notification(0xffff,next)).is_err());
                    assert_eq!(tr.data_queues[0].read,0);assert_eq!(tr.data_queues[0].outstanding,1);
                    assert!(tr.data_queues[0].busy[0]);assert_eq!(tr.tx_completion_status(qid,token),None);
                }
            }
            #[test]
            fn completion_status_is_scoped_to_current_notification_and_poll() {
                let mut mmio=vec![0u32;0x2000/4];let mut tr=transport(&mut mmio);
                let qid=allocate_data(&mut tr);let frame=authentication();
                let options=TxOptions{band_2ghz:true,antenna:1,encrypt:false,high_priority:true};
                let token=tr.send_raw80211(qid,&frame,options).unwrap();
                tr.pending.push_back(tx_notification(0xffff,1));
                assert!(tr.poll().unwrap().is_some());assert_eq!(tr.tx_completion_status(qid,token),Some(true));
                tr.pending.push_back(Notification{group:12,id:2,sequence:0,payload:vec![]});
                assert!(tr.poll().unwrap().is_some());assert_eq!(tr.tx_completion_status(qid,token),None);
                let token=tr.send_raw80211(qid,&frame,options).unwrap();
                tr.retire_tx_notification(&tx_notification(0xffff,2)).unwrap();
                assert_eq!(tr.tx_completion_status(qid,token),Some(true));
                tr.failed=true;assert!(tr.poll().is_err());assert_eq!(tr.tx_completion_status(qid,token),None);
            }
            #[test]
            fn raw_rx_and_data_completions_never_reclaim_command_queue() {
                for id in [0x1c,0xc0,0xc1,0xc5]{
                    assert!(unsolicited(&Notification{group:0,id,sequence:0,payload:vec![]}));
                }
                assert!(unsolicited(&Notification{group:1,id:0x1c,sequence:0,payload:vec![]}));
                assert!(!unsolicited(&Notification{group:12,id:2,sequence:0,payload:vec![]}));
            }
            #[test]
            fn host_command_storage_wrap_and_wire_sequence_wrap_are_independent() {
                let mut mmio=vec![0u32;0x2000/4];
                let mut tr=transport(&mut mmio);tr.restocked=true;
                // Gen2 stores32 descriptors, but the producer/sequence byte
                // runs0..255. Drive real RX reply parsing across both wraps.
                for iteration in 0..=256usize {
                    let sequence=tr.send_command(2,3,1,&(iteration as u32).to_le_bytes()).unwrap();
                    let slot=iteration&31;
                    assert_eq!(sequence,(iteration as u8)as u16);
                    let start=slot*CMD_BYTES;
                    assert_eq!(&tr.commands.bytes()[start+2..start+4],&sequence.to_le_bytes());
                    assert_eq!(&tr.commands.bytes()[start+8..start+12],&(iteration as u32).to_le_bytes());
                    assert_eq!(qword(tr.tfd.bytes(),slot*TFD_BYTES+4),tr.commands.phys+start as u64);
                    assert_eq!(mmio[0x460/4],iteration.wrapping_add(1)as u8 as u32);
                    let page=tr.rx[0].bytes_mut();page.fill(0);
                    page[..4].copy_from_slice(&4u32.to_le_bytes());
                    page[4]=3;page[5]=2;page[6..8].copy_from_slice(&sequence.to_le_bytes());
                    tr.used.put32(tr.rx_read*4,1);
                    tr.status.put16(0,((tr.rx_read+1)&(RX_COUNT-1))as u16);
                    let reply=tr.poll().unwrap().unwrap();
                    assert_eq!((reply.group,reply.id,reply.sequence),(2,3,sequence));
                    assert!(!tr.tx_busy[slot]);
                    assert!(tr.tx_busy.iter().all(|busy|!*busy));
                }
            }
            #[test]
            fn command_slot_is_never_reused_until_response() {
                let mut mmio=vec![0u32;0x2000/4];
                let mut tr=transport(&mut mmio);
                tr.restocked=true;
                for sequence in 0..32 {assert_eq!(tr.send_command(2,3,1,&[]).unwrap(),sequence);}
                assert_eq!(tr.send_command(2,3,1,&[]).unwrap_err(),"AX201 command queue full");
                assert!(tr.send_command(2,3,1,&vec![0;4096]).is_err());
            }
        }
    }
}
'''
transport = (ROOT / 'src/net/iwl_ax201_transport.rs').read_text().replace('"iwl_ax201_data.rs"', '"' + str(ROOT / 'src/net/iwl_ax201_data.rs') + '"')
# The production module's inner documentation must precede any module items.
source = source.replace('FIRMWARE', str(ROOT / 'src/net/iwl_ax201_fw.rs')).replace('TRANSPORT', transport)
with tempfile.TemporaryDirectory(prefix='trueos-ax201-transport-') as directory:
    directory = Path(directory)
    (directory / 'test.rs').write_text(source)
    subprocess.run(['rustc', '--edition=2024', '--test', str(directory / 'test.rs'), '-o', str(directory / 'test')], check=True)
    subprocess.run([str(directory / 'test')], check=True)
