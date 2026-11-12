#!/usr/bin/env python3
"""Exercise production SSH media ownership, bounds and TME1 packetization."""
from pathlib import Path
import re
import subprocess
import tempfile

from test_sdk_metadata_v2 import item

ROOT = Path(__file__).resolve().parents[2]


def main():
    source = (ROOT / "src/ui4/h264_encode_udp.rs").read_text()
    definitions = "\n".join(item(source, signature) for signature in [
        "struct EncodedAccessUnit", "pub(super) struct MediaUdpStreamReport",
        "fn send_access_unit(", "fn encode_header(",
    ]).replace("pub(super)", "pub(crate)")
    definitions = definitions.replace("pub(crate) struct MediaUdpStreamReport", "#[derive(Default)] pub(crate) struct MediaUdpStreamReport")
    definitions += "\n#[derive(Clone,Copy,Debug,Eq,PartialEq)]\n" + item(source, "enum EgressSessionPhase")
    definitions += "\n#[derive(Clone,Copy)]\n" + item(source, "struct EgressSessionRequest")
    definitions += "\n" + "\n".join(item(source, signature) for signature in [
        "struct EgressPipeline", "impl EgressPipeline", "fn request_egress_session(",
        "fn take_egress_session_request(", "fn mark_egress_session_ready(",
        "fn egress_session_ready(", "fn finish_cancelled_start(",
        "fn abort_egress_session(", "fn finish_egress_producer(",
        "fn complete_egress_session(", "fn take_egress_report(",
    ]).replace("pub(super)", "pub(crate)")
    constants = "\n".join(re.findall(r"^const (?:MAGIC|VERSION|HEADER_BYTES|DATAGRAM_BYTES|PAYLOAD_BYTES|MAX_FRAGMENT_COUNT|FLAG_START|FLAG_END|FLAG_KEYFRAME|FLAG_SESSION_END):[^\n]+", source, re.MULTILINE))
    harness = r'''
#![allow(dead_code)]
extern crate alloc;
use alloc::{vec::Vec,collections::VecDeque};
use core::sync::atomic::{AtomicU32,AtomicUsize,Ordering};
use spin::Mutex;
#[path="@QUEUE@"] mod rdp_media_queue;
static SSH_MEDIA: Mutex<rdp_media_queue::Queue> = Mutex::new(rdp_media_queue::Queue::new());
static SSH_MEDIA_OWNER_SLOT: AtomicU32 = AtomicU32::new(3);
static MEDIA_WAKES: AtomicUsize = AtomicUsize::new(0);
mod shell3 {pub mod net {
    pub fn notify_owner(slot:u32) {
        assert_eq!(slot,3);
        assert!(crate::SSH_MEDIA.try_lock().is_some(),"media lock held during wake");
        crate::MEDIA_WAKES.fetch_add(1,crate::Ordering::SeqCst);
    }
}}
static EGRESS_PIPELINE: Mutex<EgressPipeline> = Mutex::new(EgressPipeline::new());
struct Wake;
impl Wake {fn signal(&self,_:()){}}
static EGRESS_WAKE:Wake=Wake;
static PRODUCER_WAKE:Wake=Wake;
@CONSTANTS@
@DEFINITIONS@
#[test] fn disconnect_before_producer_observes_ready_completes_without_hardware_work() {
    *EGRESS_PIPELINE.lock() = EgressPipeline::new();
    let request = EgressSessionRequest {session_id:91,access_unit_count:60,target_hz:60};
    assert!(request_egress_session(request));
    assert!(take_egress_session_request().is_some());
    assert!(mark_egress_session_ready(91));
    // SSH disconnect wins the scheduling race before the producer handles
    // its readiness wake. Egress must await acknowledgement, not new frames.
    abort_egress_session(91);
    assert!(!egress_session_ready(91));
    assert!(!EGRESS_PIPELINE.lock().producer_finished);
    assert!(take_egress_report(91).is_none());
    finish_cancelled_start(91);
    assert!(EGRESS_PIPELINE.lock().producer_finished);
    complete_egress_session(91,MediaUdpStreamReport {session_id:91,..Default::default()});
    assert_eq!(take_egress_report(91).unwrap().sent_access_units,0);
    assert_eq!(EGRESS_PIPELINE.lock().phase,EgressSessionPhase::Idle);
    assert!(request_egress_session(EgressSessionRequest {session_id:92,..request}));
    take_egress_session_request();
    assert!(mark_egress_session_ready(92));
    finish_cancelled_start(91);
    finish_cancelled_start(92);
    assert!(egress_session_ready(92));
    assert!(!EGRESS_PIPELINE.lock().producer_finished);
}
#[test] fn active_abort_keeps_producer_completion_and_report_handshake() {
    *EGRESS_PIPELINE.lock() = EgressPipeline::new();
    request_egress_session(EgressSessionRequest {session_id:93,access_unit_count:60,target_hz:60});
    take_egress_session_request(); mark_egress_session_ready(93);
    assert!(egress_session_ready(93));
    abort_egress_session(93);
    finish_egress_producer(93,1,17);
    complete_egress_session(93,MediaUdpStreamReport {session_id:93,..Default::default()});
    let report=take_egress_report(93).unwrap();
    assert_eq!(report.dropped_access_units,1);
    assert_eq!(report.dropped_bytes,17);
    assert!(take_egress_report(93).is_none());
    assert_eq!(EGRESS_PIPELINE.lock().phase,EgressSessionPhase::Idle);
}
#[test] fn ssh_payload_keeps_complete_tme1_fragments_and_crc() {
    *SSH_MEDIA.lock() = rdp_media_queue::Queue::new();
    SSH_MEDIA.lock().subscribe(7);
    let bytes: Vec<u8> = (0..3000).map(|index| index as u8).collect();
    let unit = EncodedAccessUnit { sequence: 3, keyframe: true, bytes: bytes.clone() };
    let mut report = MediaUdpStreamReport { session_id: 91, ..Default::default() };
    let mut sequence = 4;
    MEDIA_WAKES.store(0,Ordering::SeqCst);
    assert!(send_access_unit(7, &unit, true, &mut sequence, &mut report));
    assert_eq!(MEDIA_WAKES.load(Ordering::SeqCst),1,"publish one wake for the complete AU");
    let mut decoded = Vec::new();
    for fragment in 0..3 {
        let packet = SSH_MEDIA.lock().pop(7).unwrap();
        assert!(packet.len() <= rdp_media_queue::MAX_PACKET_BYTES);
        assert_eq!(&packet[..4], b"TME1");
        assert_eq!(u32::from_be_bytes(packet[8..12].try_into().unwrap()), 91);
        assert_eq!(u32::from_be_bytes(packet[12..16].try_into().unwrap()), 4 + fragment);
        assert_eq!(u16::from_be_bytes(packet[20..22].try_into().unwrap()), fragment as u16);
        assert_eq!(u16::from_be_bytes(packet[22..24].try_into().unwrap()), 3);
        assert_eq!(u16::from_be_bytes(packet[24..26].try_into().unwrap()) as usize, packet.len() - HEADER_BYTES);
        assert_eq!(u32::from_be_bytes(packet[28..32].try_into().unwrap()), crc32fast::hash(&packet[HEADER_BYTES..]));
        assert_ne!(packet[5] & FLAG_KEYFRAME, 0);
        assert_eq!(packet[5] & FLAG_START != 0, fragment == 0);
        assert_eq!(packet[5] & FLAG_END != 0, fragment == 2);
        assert_eq!(packet[5] & FLAG_SESSION_END != 0, fragment == 2);
        decoded.extend_from_slice(&packet[HEADER_BYTES..]);
    }
    assert_eq!(decoded, bytes);
    assert_eq!(sequence, 7);
    assert_eq!(report.sent_datagrams, 3);
    assert_eq!(report.sent_payload_bytes, 3000);
    assert!(SSH_MEDIA.lock().pop(7).is_none());
}
#[test] fn retired_session_cannot_deliver_video_to_new_owner() {
    *SSH_MEDIA.lock() = rdp_media_queue::Queue::new();
    SSH_MEDIA.lock().subscribe(7);
    SSH_MEDIA.lock().unsubscribe(7);
    SSH_MEDIA.lock().subscribe(8);
    let unit = EncodedAccessUnit { sequence: 0, keyframe: true, bytes: vec![0; 100] };
    let mut report = MediaUdpStreamReport::default();
    assert!(!send_access_unit(7, &unit, false, &mut 0, &mut report));
    assert!(SSH_MEDIA.lock().pop(8).is_none());
    assert!(send_access_unit(8, &unit, false, &mut 0, &mut report));
    assert!(SSH_MEDIA.lock().pop(7).is_none());
    assert!(SSH_MEDIA.lock().pop(8).is_some());
}
#[test] fn oversized_access_unit_is_rejected_before_partial_publication() {
    *SSH_MEDIA.lock() = rdp_media_queue::Queue::new();
    SSH_MEDIA.lock().subscribe(7);
    let unit = EncodedAccessUnit { sequence: 0, keyframe: true, bytes: vec![0; PAYLOAD_BYTES * MAX_FRAGMENT_COUNT + 1] };
    assert!(!send_access_unit(7, &unit, false, &mut 0, &mut MediaUdpStreamReport::default()));
    assert!(SSH_MEDIA.lock().pop(7).is_none());
}
'''.replace("@QUEUE@", str(ROOT / "src/ui4/rdp_media_queue.rs"))
    harness = harness.replace("@CONSTANTS@", constants).replace("@DEFINITIONS@", definitions)
    channel = re.search(r'channel\s*=\s*"([^"]+)"', (ROOT / "rust-toolchain.toml").read_text())[1]
    with tempfile.TemporaryDirectory(prefix="trueos-rdp-ssh-media-") as directory:
        directory = Path(directory)
        (directory / "lib.rs").write_text(harness)
        (directory / "Cargo.toml").write_text('[package]\nname="rdp-ssh-media-tests"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[lib]\npath="lib.rs"\n[dependencies]\nspin="0.10"\ncrc32fast="1.5"\n')
        subprocess.run(["cargo", "+" + channel, "test", "--offline", "--manifest-path", str(directory / "Cargo.toml"), "--target", "x86_64-unknown-linux-gnu", "--target-dir", str(ROOT / "bld/test-cache/rdp-ssh-media"), "--", "--test-threads=1"], cwd=directory, check=True)


if __name__ == "__main__":
    main()
