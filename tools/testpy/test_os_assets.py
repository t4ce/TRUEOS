#!/usr/bin/env python3
"""Exercise production kernel asset registry and Blueprint API over a host ABI."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
API = ROOT.parent / 'TRUEOS-Blueprints/api/src'


def main():
    kernel = (ROOT/'src/ui4/blueprint_text.rs').read_text()
    start = kernel.index('#[repr(C)]\n#[derive(Clone, Copy, Debug, Default)]\npub(crate) struct BlueprintImageSourceInfo')
    end = kernel.index('/// Longest UTF-8 label', start)
    source = f'''#![allow(dead_code)]
extern crate alloc;
extern crate self as v;
#[path="{ROOT}/src/os_assets.rs"] mod os_assets;
#[path="{API}/image_source.rs"] mod image_source;
#[path="{API}/os_assets.rs"] mod api_os_assets;
const ERROR_NOT_FOUND:i32=-3;
const ERROR_INVALID:i32=-1;
const IMAGE_SOURCE_READ_CHUNK_BYTES:usize=16384;
const IMAGE_SOURCE_FORMAT_RGBA8:u32=2;
mod efi {{pub mod acpi {{pub mod bgrt {{
pub static PRESENT:std::sync::atomic::AtomicBool=std::sync::atomic::AtomicBool::new(false);
pub fn decoded_logo_rgba()->Option<(usize,usize,&'static [u32])> {{
PRESENT.load(std::sync::atomic::Ordering::Relaxed).then_some((2,1,&[0x112233,0x445566]))
}}
}}}}}}
mod virtio_gpu_logo {{
pub fn live_update_triptych_panel_info(_:usize)->Option<(u32,u32,usize)> {{Some((1,1,4))}}
pub fn copy_live_update_triptych_panel(_:usize,offset:usize,out:&mut [u8])->Option<usize> {{let data=[1,2,3,255];let rest=data.get(offset..)?;let len=rest.len().min(out.len());out[..len].copy_from_slice(&rest[..len]);Some(len)}}
}}
'''+kernel[start:end]+'''
mod bp_abi {
pub type TrueosImageSourceInfo=crate::BlueprintImageSourceInfo;
pub unsafe fn trueos_cabi_image_source_info(name:*const u8,len:usize,out:*mut TrueosImageSourceInfo)->i32 {
let name=unsafe {core::str::from_utf8_unchecked(core::slice::from_raw_parts(name,len))};
match crate::blueprint_image_source_info(name) {Ok(info)=>{unsafe {out.write(info)};0},Err(error)=>error}
}
pub unsafe fn trueos_cabi_image_source_read(name:*const u8,len:usize,offset:usize,out:*mut u8,capacity:usize)->isize {
let name=unsafe {core::str::from_utf8_unchecked(core::slice::from_raw_parts(name,len))};
let out=unsafe {core::slice::from_raw_parts_mut(out,capacity)};
match crate::copy_blueprint_image_source(name,offset,out) {Ok(len)=>len as isize,Err(error)=>error as isize}
}
}
#[test] fn all_embedded_assets_have_exact_bytes_metadata_and_bounded_reads() {
for asset in api_os_assets::EMBEDDED {
let expected=os_assets::image(asset.source()).unwrap();let actual=api_os_assets::read(asset).unwrap();
assert_eq!(actual.bytes.as_ref(),expected.bytes);assert_eq!((actual.info.width,actual.info.height),(expected.width,expected.height));assert_eq!(actual.info.format,expected.format);
let mut out=[0;1];assert_eq!(copy_blueprint_image_source(asset.source(),expected.bytes.len(),&mut out),Ok(0));
assert_eq!(copy_blueprint_image_source(asset.source(),expected.bytes.len()+1,&mut out),Err(-1));
assert_eq!(copy_blueprint_image_source(asset.source(),0,&mut vec![0;16385]),Err(-1));
}
assert_eq!(api_os_assets::logo_jpeg().unwrap(),os_assets::LOGO_JPEG);
assert_eq!(image_source::read("kernel:live-update").unwrap().1,os_assets::LIVE_UPDATE_NOWAY_PNG);
assert_eq!(blueprint_image_source_info("no-such-asset").unwrap_err(),-3);
for name in ["kernel:live-update-nway-left","kernel:live-update-nway-center","kernel:live-update-nway-right"] {assert_eq!(image_source::read(name).unwrap().1,[1,2,3,255]);}
}
#[test] fn bgrt_is_opt_in_and_converts_firmware_pixels_to_rgba() {
assert!(api_os_assets::bgrt().unwrap().is_none());
efi::acpi::bgrt::PRESENT.store(true,std::sync::atomic::Ordering::Relaxed);
let data=api_os_assets::bgrt().unwrap().unwrap();assert_eq!((data.info.width,data.info.height),(2,1));
assert_eq!(data.bytes.as_ref(),[0x11,0x22,0x33,255,0x44,0x55,0x66,255]);
}
'''
    with tempfile.TemporaryDirectory(prefix='os-assets-check-') as directory:
        folder=Path(directory)
        (folder/'check.rs').write_text(source)
        subprocess.run(['rustc','--edition=2024','--test',str(folder/'check.rs'),'-o',str(folder/'check')],check=True)
        subprocess.run([str(folder/'check'),'--test-threads=1'],check=True)


if __name__ == '__main__':
    main()
