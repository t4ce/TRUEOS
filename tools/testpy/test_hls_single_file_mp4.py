#!/usr/bin/env python3
"""Run production HLS admission and MP4 demux on host, without a kernel build.

Optional --playlist/--media arguments also exercise a captured single-file fMP4
playlist and its backing resource. The default tests are entirely offline.
"""
import argparse
import os
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--playlist', type=Path)
parser.add_argument('--media', type=Path)
args = parser.parse_args()
if bool(args.playlist) != bool(args.media):
    parser.error('--playlist and --media must be supplied together')

source = (ROOT / 'src/intel/media/hw_vid.rs').read_text()
start = source.rfind('#[derive(', 0, source.index('struct Mp4Box {'))
end = source.index('fn h264_has_annexb_start_code(')
demux = source[start:end]
head = r'''
#![allow(dead_code)]
extern crate alloc;
#[macro_export] macro_rules! log { ($($args:tt)*) => {{let _=format_args!($($args)*);}} }
mod media {
use alloc::{string::String, vec::Vec};
use core::fmt::Write;
'''
head += f'#[path = "{ROOT / "src/intel/media/hls.rs"}"] mod hls;\n'
tail = r'''
#[cfg(test)] mod tests {
 use super::*;
 fn bx(name:&[u8;4],data:&[u8])->Vec<u8>{[(8+data.len() as u32).to_be_bytes().as_slice(),name,data].concat()}
 fn init()->Vec<u8>{
  let avcc=bx(b"avcC",&[1,100,0,21,255,225,0,2,0x67,0x80,1,0,2,0x68,0x80]);
  let visual=bx(b"avc1",&[vec![0;78],avcc].concat());
  let stsd=bx(b"stsd",&[[0u8;4].as_slice(),1u32.to_be_bytes().as_slice(),&visual].concat());
  let minf=bx(b"minf",&bx(b"stbl",&stsd));
  let mut mdhd=vec![0;16];mdhd[12..].copy_from_slice(&90000u32.to_be_bytes());
  let mut hdlr=vec![0;12];hdlr[8..].copy_from_slice(b"vide");
  let mdia=bx(b"mdia",&[bx(b"mdhd",&mdhd),bx(b"hdlr",&hdlr),minf].concat());
  let mut tkhd=vec![0;16];tkhd[12..].copy_from_slice(&1u32.to_be_bytes());
  let trak=bx(b"trak",&[bx(b"tkhd",&tkhd),mdia].concat());
  let trex=bx(b"trex",&[0u32,1,1,3600,6,0].into_iter().flat_map(u32::to_be_bytes).collect::<Vec<_>>());
  [bx(b"ftyp",b"iso6\0\0\0\0iso6"),bx(b"moov",&[trak,bx(b"mvex",&trex)].concat())].concat()
 }
 fn fragment(data_offset:Option<i32>)->Vec<u8>{
  let tfhd=bx(b"tfhd",&[0x020000u32,1].into_iter().flat_map(u32::to_be_bytes).collect::<Vec<_>>());
  let tfdt=bx(b"tfdt",&[0u32,9000].into_iter().flat_map(u32::to_be_bytes).collect::<Vec<_>>());
  let trun=|offset:i32|bx(b"trun",&[0x01000a01u32,2,offset as u32,6,3600,6,(-3600i32) as u32].into_iter().flat_map(u32::to_be_bytes).collect::<Vec<_>>());
  let provisional=bx(b"moof",&bx(b"traf",&[tfhd.clone(),tfdt.clone(),trun(0)].concat()));
  let moof=bx(b"moof",&bx(b"traf",&[tfhd,tfdt,trun(data_offset.unwrap_or(provisional.len() as i32+8))].concat()));
  [moof,bx(b"mdat",&[0,0,0,2,0x65,0x80,0,0,0,2,0x41,0x80])].concat()
 }
 #[test]fn admitted_single_file_fmp4_preserves_avc_and_b_frame_timing(){
  let initialization=init();let fragment=fragment(None);
  let text=format!("#EXTM3U\n#EXT-X-MAP:URI=\"clip.mp4\",BYTERANGE=\"{}@0\"\n#EXTINF:0.08,\n#EXT-X-BYTERANGE:{}@{}\nclip.mp4\n#EXT-X-ENDLIST",initialization.len(),fragment.len(),initialization.len());
  let length=match hls::playlist(&text).unwrap(){hls::Playlist::SingleFileMp4{length,..}=>length,_=>panic!("wrong route")};
  let bytes=[initialization,fragment].concat();assert_eq!(length,bytes.len());
  let demuxed=mp4_avc1_to_annexb(&bytes[..length]).unwrap();
  assert_eq!(demuxed.timing.iter().map(|t|(t.dts,t.pts,t.duration,t.timescale)).collect::<Vec<_>>(),vec![(9000,12600,3600,90000),(12600,9000,3600,90000)]);
  for nal in [[0,0,0,1,0x67],[0,0,0,1,0x68],[0,0,0,1,0x65],[0,0,0,1,0x41]]{assert!(demuxed.annexb.windows(nal.len()).any(|w|w==nal));}
 }
 #[test]fn incomplete_or_out_of_bounds_fragment_cannot_reach_decode(){
  let mut bytes=[init(),fragment(None)].concat();bytes.pop();assert!(mp4_avc1_to_annexb(&bytes).is_err());
  assert!(mp4_avc1_to_annexb(&[init(),fragment(Some(i32::MAX))].concat()).is_err());
 }
 #[test]fn captured_single_file_playlist_demuxes_with_production_avc_parser(){
  let Some(playlist)=std::env::var_os("HLS_FIXTURE_PLAYLIST")else{return;};
  let text=std::fs::read_to_string(playlist).unwrap();
  let bytes=std::fs::read(std::env::var_os("HLS_FIXTURE_MEDIA").unwrap()).unwrap();
  let (reference,length)=match hls::playlist(&text).unwrap(){hls::Playlist::SingleFileMp4{reference,length}=>(reference,length),_=>panic!("fixture did not use single-file route")};
  assert!(length<=bytes.len());let demuxed=mp4_avc1_to_annexb(&bytes[..length]).unwrap();
  assert!(!demuxed.annexb.is_empty());assert!(!demuxed.timing.is_empty());
  assert!(demuxed.timing.windows(2).all(|t|t[0].dts<=t[1].dts));
  println!("captured HLS: reference={reference} indexed_bytes={length} pictures={} annexb_bytes={}",demuxed.timing.len(),demuxed.annexb.len());
 }
}
}
'''
env = os.environ.copy()
if args.playlist:
    env['HLS_FIXTURE_PLAYLIST'] = str(args.playlist.resolve())
    env['HLS_FIXTURE_MEDIA'] = str(args.media.resolve())
with tempfile.TemporaryDirectory(prefix='hls-single-file-') as directory:
    path = Path(directory)
    (path / 'test.rs').write_text(head + demux + tail)
    subprocess.run(['rustc', '--edition=2024', '--test', str(path / 'test.rs'), '-o', str(path / 'test')], check=True)
    subprocess.run([str(path / 'test'), '--nocapture'], env=env, check=True)
