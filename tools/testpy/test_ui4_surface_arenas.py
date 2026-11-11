#!/usr/bin/env python3
"""Exercise the real producer allocator across fullscreen resize generations."""

from pathlib import Path
import subprocess
import tempfile

from test_clip_position3_uv_texture import item, constant

ROOT = Path(__file__).resolve().parents[2]
SURFACE = "src/r/ui_surface.rs"
source = r'''
#![allow(dead_code)]
mod graphics {pub mod primitives {
    #[derive(Clone,Copy)]pub enum UiSurfaceFormat{Rgba8888}
    #[derive(Clone,Copy)]pub struct UiSurface{pub gpu:u64,pub width:u32,pub height:u32,
        pub pitch:u32,pub format:UiSurfaceFormat}
}}
use graphics::primitives::{UiSurface,UiSurfaceFormat};
#[derive(Clone,Copy,Debug)]enum Error{Invalid}
type Result<T>=core::result::Result<T,Error>;
mod intel {
    pub const WARM_ALIGN:usize=4096;
    pub fn align_up(value:usize,align:usize)->Option<usize>{
        value.checked_add(align-1).map(|v|v&!(align-1))
    }
}
'''
for name in ("MAX_UI_SURFACES", "UI_SURFACE_GPU_BASE", "UI_SURFACE_GPU_LIMIT",
             "UI_SURFACE_GPU_SPILL_BASE", "UI_SURFACE_GPU_SPILL_LIMIT",
             "UI_SURFACE_MAX_BYTES", "UI_SURFACE_BYTES_PER_PIXEL"):
    source += constant(SURFACE, name)
for name in ("TrustedUiSurface", "allocate_surface_gpu_va", "allocate_surface_gpu_va_in_range",
             "aligned_pitch_bytes"):
    source += item(SURFACE, name)

# Include all existing production allocator tests too.
text = (ROOT / SURFACE).read_text()
source += text[text.index("#[cfg(test)]\nmod tests {"):]
source += r'''
fn allocated(surfaces:&mut[Option<TrustedUiSurface>],slot:usize,w:u32,h:u32)->u64 {
    let pitch=aligned_pitch_bytes(w).unwrap();
    let bytes=intel::align_up(pitch as usize*h as usize,intel::WARM_ALIGN).unwrap();
    let gpu=allocate_surface_gpu_va(surfaces,bytes).expect("admitted producer allocation");
    surfaces[slot]=Some(TrustedUiSurface{desc:UiSurface{gpu,width:w,height:h,pitch,
        format:UiSurfaceFormat::Rgba8888},phys:slot as u64*0x2000000,
        virt:core::ptr::null_mut(),byte_len:bytes});
    gpu
}
#[test]fn fills_legacy_bank_before_spill_and_never_allocates_reserved_gap() {
    let mut surfaces=[None;MAX_UI_SURFACES];
    for slot in 0..7 {
        assert_eq!(allocated(&mut surfaces,slot,4096,2048),
            UI_SURFACE_GPU_BASE+slot as u64*0x02000000);
    }
    assert_eq!(allocated(&mut surfaces,7,4096,2048),UI_SURFACE_GPU_SPILL_BASE);
    assert_eq!(allocated(&mut surfaces,8,4096,2048),UI_SURFACE_GPU_SPILL_BASE+0x02000000);
    let first=surfaces[2].unwrap();surfaces[2]=None;
    assert_eq!(allocated(&mut surfaces,2,4096,2048),first.desc.gpu,
        "a reclaimed legacy hole takes priority over extending spill");
    let spill=surfaces[7].unwrap();surfaces[7]=None;
    assert_eq!(allocated(&mut surfaces,7,4096,2048),spill.desc.gpu);
    for surface in surfaces.iter().flatten() {
        let start=surface.desc.gpu;let end=start+surface.byte_len as u64;
        assert!((start>=UI_SURFACE_GPU_BASE&&end<=UI_SURFACE_GPU_LIMIT)||
            (start>=UI_SURFACE_GPU_SPILL_BASE&&end<=UI_SURFACE_GPU_SPILL_LIMIT));
        assert!(end<1u64<<31);
    }
}
#[test]fn native_link3d_resize_peak_keeps_old_new_shell_and_interaction_live() {
    let mut surfaces=[None;MAX_UI_SURFACES];
    // Two full-output interaction buffers plus the observed2700x1250 shell
    // pair make this conservative: interaction is normally display-owned.
    for slot in 0..2 {allocated(&mut surfaces,slot,3840,2160);}
    for slot in 2..4 {allocated(&mut surfaces,slot,2700,1250);}
    // Link3D foreground triple and background double at75%output, all retained
    // while the replacement pair is prepared before atomic resize commit.
    for slot in 4..9 {allocated(&mut surfaces,slot,2880,1620);}
    let old:Vec<_>=surfaces[4..9].iter().map(|s|s.unwrap()).collect();
    for slot in 9..14 {allocated(&mut surfaces,slot,3840,2160);}
    assert_eq!(old.iter().map(|s|s.byte_len).sum::<usize>(),93_327_360);
    assert_eq!(surfaces[9..14].iter().map(|s|s.unwrap().byte_len).sum::<usize>(),165_888_000);
    assert!(surfaces.iter().flatten().any(|s|s.desc.gpu>=UI_SURFACE_GPU_SPILL_BASE));
    for (slot,expected) in (4..9).zip(old.iter()) {
        let current=surfaces[slot].unwrap();assert_eq!(current.desc.gpu,expected.desc.gpu);
        assert_eq!(current.byte_len,expected.byte_len);
    }
    for (i,a) in surfaces.iter().enumerate().filter_map(|(i,s)|s.map(|s|(i,s))) {
        for b in surfaces[i+1..].iter().flatten() {
            assert!(a.desc.gpu+a.byte_len as u64<=b.desc.gpu||
                b.desc.gpu+b.byte_len as u64<=a.desc.gpu);
        }
    }
    let old_first=surfaces[4].unwrap().desc.gpu;
    for slot in 4..9 {surfaces[slot]=None;}
    assert_eq!(allocated(&mut surfaces,4,2880,1620),old_first,
        "retired old generation is reusable without relocating live replacement");
}
#[test]fn surface_cap_and_overflow_remain_fail_closed() {
    let empty=[None;MAX_UI_SURFACES];
    for bytes in [0,UI_SURFACE_MAX_BYTES as usize+1,usize::MAX] {
        assert_eq!(allocate_surface_gpu_va(&empty,bytes),None);
    }
    assert_eq!(allocate_surface_gpu_va_in_range(&empty,4096,u64::MAX-2047,u64::MAX),None);
}
'''

with tempfile.TemporaryDirectory(prefix="trueos-ui4-surface-arenas-") as directory:
    path = Path(directory)
    (path / "tests.rs").write_text(source)
    subprocess.run(["rustc", "--edition=2024", "--test", str(path / "tests.rs"),
                    "-o", str(path / "tests")], check=True, cwd=ROOT)
    subprocess.run([str(path / "tests")], check=True)
