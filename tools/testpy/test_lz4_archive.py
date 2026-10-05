#!/usr/bin/env python3
"""Cross-check the production no_std LZ4 frame + tar code against liblz4/tarfile."""
import ctypes as c
import io
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
import zlib
ROOT = Path(__file__).resolve().parents[2]


def main():
    lib = c.CDLL('liblz4.so.1')
    lib.LZ4F_compressFrameBound.argtypes = [c.c_size_t, c.c_void_p]
    lib.LZ4F_compressFrameBound.restype = c.c_size_t
    lib.LZ4F_compressFrame.argtypes = [c.c_void_p, c.c_size_t, c.c_void_p, c.c_size_t, c.c_void_p]
    lib.LZ4F_compressFrame.restype = c.c_size_t
    lib.LZ4F_createDecompressionContext.argtypes = [c.POINTER(c.c_void_p), c.c_uint]
    lib.LZ4F_decompress.argtypes = [c.c_void_p, c.c_void_p, c.POINTER(c.c_size_t), c.c_void_p, c.POINTER(c.c_size_t), c.c_void_p]
    lib.LZ4F_decompress.restype = c.c_size_t
    lib.LZ4F_freeDecompressionContext.argtypes = [c.c_void_p]
    lib.LZ4F_isError.argtypes = [c.c_size_t]
    def compress(data):
        cap = lib.LZ4F_compressFrameBound(len(data), None)
        out = c.create_string_buffer(cap)
        size = lib.LZ4F_compressFrame(out, cap, data, len(data), None)
        assert not lib.LZ4F_isError(size)
        return out.raw[:size]
    def decompress(data):
        ctx = c.c_void_p()
        assert lib.LZ4F_createDecompressionContext(c.byref(ctx), 100) == 0
        out = c.create_string_buffer(8 * 1024 * 1024)
        dst = c.c_size_t(len(out)); src = c.c_size_t(len(data))
        try:
            result = lib.LZ4F_decompress(ctx, out, c.byref(dst), data, c.byref(src), None)
            assert result == 0 and src.value == len(data), result
            return out.raw[:dst.value]
        finally: lib.LZ4F_freeDecompressionContext(ctx)
    with tempfile.TemporaryDirectory(prefix='trueos-lz4-archive-') as td:
        folder = Path(td); (folder / 'src').mkdir()
        (folder / 'Cargo.toml').write_text('''[package]
name = "trueos-lz4-archive-test"
version = "0.0.0"
edition = "2024"
[workspace]
[dependencies]
lz4_flex = { version = "0.11.6", default-features = false, features = ["safe-encode", "safe-decode", "checked-decode"] }
twox-hash = { version = "2.1.2", default-features = false, features = ["xxhash32"] }
lzma-rust2 = { version = "0.16.2", default-features = false, features = ["encoder"] }
crc32fast = "1.5.0"
''')
        (folder / 'src/main.rs').write_text(f'''
extern crate alloc;
#[macro_export] macro_rules! log_info {{ ($($args:tt)*) => {{}}; }}
#[macro_export] macro_rules! log_warn {{ ($($args:tt)*) => {{}}; }}
#[path="{ROOT / 'src/z7.rs'}"] mod z7;
mod intel {{ pub mod gpgpu {{
    #[derive(Debug)] pub enum Lz4GpuError {{ Unavailable, Submission }}
    std::thread_local! {{ pub static MODE: std::cell::Cell<u8> = const {{ std::cell::Cell::new(0) }}; }}
    pub fn lz4_gpu_available() -> bool {{ MODE.get() != 0 }}
    pub async fn lz4_gpu_blocks(input: &[(&[u8],usize)], encode: bool) -> Result<Vec<Vec<u8>>,Lz4GpuError> {{
        if MODE.get() == 3 && !encode {{
            return Ok(input.iter().map(|(bytes, cap)| {{
                let mut out = vec![0; *cap];
                let len = lz4_flex::block::decompress_into(bytes, &mut out).unwrap();
                out.truncate(len); out
            }}).collect());
        }}
        Err(if MODE.get() == 2 {{ Lz4GpuError::Submission }} else {{ Lz4GpuError::Unavailable }})
    }}
}} }}
mod r {{
    #[path="{ROOT / 'src/r/archive_limits.rs'}"] pub mod archive_limits;
    pub mod codec {{
        pub struct Pool;
        pub static CODEC_COMPUTE: Pool = Pool;
        impl Pool {{ pub async fn run<F,T>(&self,_: &str,f:F) -> Result<T,()> where F:FnOnce(())->T {{ Ok(f(())) }} }}
    }}
    #[path="{ROOT / 'src/r/lz4.rs'}"] pub mod lz4;
    #[path="{ROOT / 'src/r/tar.rs'}"] pub mod tar;
}}
fn block_on<T>(future: impl std::future::Future<Output=T>) -> T {{
    let mut future=std::pin::pin!(future);
    let mut context=std::task::Context::from_waker(std::task::Waker::noop());
    match future.as_mut().poll(&mut context) {{ std::task::Poll::Ready(value)=>value, _=>panic!("mock pool must finish synchronously") }}
}}
fn main() {{
    let args:Vec<_>=std::env::args().collect();
    let source=std::fs::read(&args[2]).unwrap();
    match args[1].as_str() {{
        "asset-check" => {{
            use r::archive_limits::*;
            assert!(source.len() <= MAX_ARCHIVE_BYTES);
            intel::gpgpu::MODE.set(1);
            let tar=block_on(r::lz4::decompress_frame(source,MAX_TAR_BYTES)).unwrap();
            let entries=r::tar::unpack(&tar,MAX_ARCHIVE_ENTRIES,MAX_SOURCE_FILE_BYTES,MAX_SOURCE_TOTAL_BYTES).unwrap();
            let manifest=std::fs::read_to_string(&args[3]).unwrap();
            let expected:Vec<_>=manifest.lines().collect();
            assert_eq!(entries.len(),expected.len());
            for (entry, line) in entries.iter().zip(expected) {{
                let fields:Vec<_>=line.splitn(3,'\\t').collect();
                assert_eq!(entry.name,fields[2]);
                assert_eq!(entry.bytes.len(),fields[0].parse::<usize>().unwrap());
                assert_eq!(crc32fast::hash(&entry.bytes),fields[1].parse::<u32>().unwrap(),"asset mismatch: {{}}",entry.name);
            }}
            println!("Production decoder verified {{}} asset files / {{}} bytes",entries.len(),entries.iter().map(|entry|entry.bytes.len()).sum::<usize>());
        }}
        "encode" => std::fs::write(&args[3],r::lz4::compress_frame_cpu(&source)).unwrap(),
        "decode" => std::fs::write(&args[3],r::lz4::decompress_frame_cpu(&source,8*1024*1024).unwrap()).unwrap(),
        "unpack" => {{
            let entries=r::tar::unpack(&source,32,200000,4*1024*1024).unwrap();
            assert_eq!(entries.len(),25);
            for (i,e) in entries.iter().enumerate() {{ assert_eq!(e.content_type_raw,Some(1)); assert_eq!(e.bytes,vec![i as u8;100000]); }}
        }}
        "repack" => {{
            let entries=r::tar::unpack(&source,32,200000,4*1024*1024).unwrap();
            let borrowed:Vec<_>=entries.iter().map(|e|z7::SevenZSourceEntry{{name:&e.name,bytes:&e.bytes,content_type_raw:e.content_type_raw}}).collect();
            std::fs::write(&args[3],r::tar::pack(&borrowed,4*1024*1024).unwrap()).unwrap();
        }}
        _ => panic!(),
    }}
}}
#[test] fn gpu_unavailable_uses_cpu_but_submission_errors_do_not() {{
    let data:Vec<_>=(0..150000).map(|i|(i*31) as u8).collect();
    intel::gpgpu::MODE.set(1);
    let frame=block_on(r::lz4::compress_frame(data.clone())).unwrap();
    assert_eq!(block_on(r::lz4::decompress_frame(frame.clone(),data.len())).unwrap(),data);
    let encoded=block_on(r::lz4::blocks(vec![(data[..4096].to_vec(),8192)],true)).unwrap();
    assert_eq!(block_on(r::lz4::blocks(vec![(encoded[0].clone(),4096)],false)).unwrap()[0],data[..4096]);
    intel::gpgpu::MODE.set(2);
    assert_eq!(block_on(r::lz4::decompress_frame(frame,data.len())),Err(r::lz4::Error::Gpu));
    assert_eq!(block_on(r::lz4::compress_frame(data)),Err(r::lz4::Error::Gpu));
}}
#[test] fn decode_progress_tracks_cpu_and_gpu_blocks_without_changing_output() {{
    let data = vec![17; 10 * 65536];
    let frame = r::lz4::compress_frame_cpu(&data);
    for mode in [0, 1, 3] {{
        intel::gpgpu::MODE.set(mode);
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = seen.clone();
        let out = block_on(r::lz4::decompress_frame_with_progress(frame.clone(), data.len(), move |done, total| {{
            captured.lock().unwrap().push((done, total));
        }})).unwrap();
        assert_eq!(out, data);
        let seen = seen.lock().unwrap();
        assert!(!seen.is_empty());
        assert!(seen.iter().all(|(done,total)| *done > 0 && done <= total));
        assert!(seen.windows(2).all(|w| w[0].0 < w[1].0 && w[0].1 == w[1].1));
        assert_eq!(seen.last().unwrap().0, seen.last().unwrap().1);
    }}
}}
#[test] fn bounded_and_corrupt_frames() {{
    for data in [vec![],vec![0;100000],(0..100000).map(|n|(n*31) as u8).collect()] {{
        let frame=r::lz4::compress_frame_cpu(&data);
        assert_eq!(r::lz4::decompress_frame_cpu(&frame,data.len()).unwrap(),data);
        if !data.is_empty() {{ assert!(r::lz4::decompress_frame_cpu(&frame,data.len()-1).is_err()); }}
        for length in 0..frame.len() {{ assert!(r::lz4::decompress_frame_cpu(&frame[..length],data.len()).is_err()); }}
        let mut bad=frame.clone(); bad[6]^=1; assert!(r::lz4::decompress_frame_cpu(&bad,data.len()).is_err());
        let last=bad.len()-1; bad=frame; bad[last]^=1; assert!(r::lz4::decompress_frame_cpu(&bad,data.len()).is_err());
    }}
}}
#[test] fn tar_limits_and_corruption() {{
    let entries=[z7::SevenZSourceEntry{{name:"nested/empty",bytes:b"",content_type_raw:None}},z7::SevenZSourceEntry{{name:"long",bytes:b"abc",content_type_raw:Some(1)}}];
    let tar=r::tar::pack(&entries,8192).unwrap();
    assert_eq!(r::tar::unpack(&tar,2,3,3).unwrap().len(),2);
    assert!(r::tar::unpack(&tar,1,3,3).is_err());
    assert!(r::tar::unpack(&tar,2,2,3).is_err());
    assert!(r::tar::unpack(&tar,2,3,2).is_err());
    assert!(r::tar::pack(&entries,100).is_err());
    let mut bad=tar.clone();bad[0]^=1;assert!(r::tar::unpack(&bad,2,3,3).is_err());
    assert!(r::tar::unpack(&tar[..tar.len()-512],2,3,3).is_err());
}}
''')
        env = os.environ.copy(); env['CARGO_TARGET_DIR'] = str(ROOT/'bld/lz4-host-tests')
        cargo = ['cargo', '+nightly-2026-07-10']
        subprocess.run(cargo+['test','--quiet'],cwd=folder,env=env,check=True)
        subprocess.run(cargo+['build','--quiet','--release'],cwd=folder,env=env,check=True)
        binary = ROOT/'bld/lz4-host-tests/release/trueos-lz4-archive-test'
        def run(mode, source, dest=None):
            subprocess.run([str(binary),mode,str(source),str(dest or source)],check=True)
        for index,data in enumerate([b'',bytes(range(256)),b'hello world'*250000,os.urandom(3*1024*1024)]):
            source=folder/f'{index}.raw';source.write_bytes(data)
            encoded=folder/f'{index}.lz4';run('encode',source,encoded)
            assert decompress(encoded.read_bytes()) == data
            encoded.write_bytes(compress(data));restored=folder/f'{index}.out';run('decode',encoded,restored)
            assert restored.read_bytes()==data
        archive=folder/'input.tar'
        with tarfile.open(archive,'w',format=tarfile.PAX_FORMAT) as tf:
            for i in range(25):
                info=tarfile.TarInfo(f'{i:02}/'+('long-path-'*15)+'.bin');info.size=100000
                info.pax_headers={'TRUEOS.content_type':'1'}
                tf.addfile(info,io.BytesIO(bytes([i])*info.size))
        run('unpack',archive)
        repacked=folder/'output.tar';run('repack',archive,repacked)
        with tarfile.open(repacked) as tf:
            for i,member in enumerate(tf):
                assert member.pax_headers['TRUEOS.content_type']=='1'
                assert tf.extractfile(member).read()==bytes([i])*100000
        encoded=folder/'archive.tar.lz4';run('encode',repacked,encoded)
        with tarfile.open(fileobj=io.BytesIO(decompress(encoded.read_bytes()))) as tf: assert len(tf.getmembers())==25
        if os.environ.get('TRUEOS_LZ4_ASSET_ARCHIVE'):
            asset_archive=Path(os.environ['TRUEOS_LZ4_ASSET_ARCHIVE'])
            manifest=folder/'assets.manifest'
            with subprocess.Popen(['lz4','-dc',str(asset_archive)],stdout=subprocess.PIPE) as process:
                with tarfile.open(fileobj=process.stdout,mode='r|') as tf, manifest.open('w') as reference:
                    for member in tf:
                        if member.isfile():
                            crc=0
                            with tf.extractfile(member) as content:
                                while chunk:=content.read(1024*1024):
                                    crc=zlib.crc32(chunk,crc)
                            reference.write(f'{member.size}\t{crc}\t{member.name}\n')
                assert process.wait()==0
            run('asset-check',asset_archive,manifest)
        print('LZ4 frames interoperate with liblz4; tar/PAX interoperates with tarfile; 25 files / 2.5 MB checked')
if __name__=='__main__': main()
