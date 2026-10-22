#!/usr/bin/env python3
"""Exercise the production slot-0 BCS copy plan and window damage on the host."""
from pathlib import Path
import re
import subprocess
import tempfile

import test_clip_position3_uv_texture as extract

ROOT = Path(__file__).resolve().parents[2]
extract.ROOT = ROOT


def main():
    surfaces = 'src/intel/gpgpu/types/surfaces.rs'
    surface_source = (ROOT / surfaces).read_text()
    source = '''#![allow(dead_code)]
extern crate alloc;
use alloc::{collections::BTreeMap, vec::Vec};
mod ui4 {
    #[path = "DAMAGE_PATH"] pub mod damage;
    #[path = "ARC_PATH"] pub mod window_arc;
}
mod intel {
pub(crate) use crate::ui4::damage::{DamageRect as CompositionDamageRect, DamageRegion as CompositionDamageRegion};
'''.replace('DAMAGE_PATH', str(ROOT / 'src/ui4/damage.rs')).replace(
        'ARC_PATH', str(ROOT / 'src/ui4/window_arc.rs'))
    source += '\n'.join(extract.item('src/intel/copy/blt.rs', name) for name in (
        'GucBcs0RgbaSurface', 'GucBcs0RgbaCopy'))
    source += '\npub mod gpgpu {\n'
    for name in ('GpgpuRect', 'GpgpuRgba8StorageOrder', 'GpgpuRgba8Surface'):
        source += extract.item(surfaces, name)
        if name != 'GpgpuRgba8StorageOrder':
            source += re.search(rf'^impl {name} \{{.*?^}}', surface_source, re.M | re.S).group()
    source += '''
}}
#[derive(Clone, Copy, Debug)] struct Rgba8 {r:u8,g:u8,b:u8,a:u8}
use ui4::damage::{DamageRect as CompositionDamageRect, DamageRegion as CompositionDamageRegion};
'''
    source += extract.item('src/intel/display.rs', 'RgbaOverlayTile')
    source += extract.item('src/intel/display.rs', 'Ui4AsyncCompositionError')
    source += f'\n#[path = "{ROOT}/src/intel/display/ui4_bcs.rs"] mod ui4_bcs;\n'
    source += '''
#[derive(Clone, Copy)] struct WindowPlacement {x:i32,y:i32,width:u32,height:u32}
'''
    source += '\n'.join(extract.item('src/ui4/compositor_service.rs', name) for name in (
        'add_placement_damage', 'add_placement_change_damage', 'clipped_output_rect'))
    source += r'''
use intel::{GucBcs0RgbaCopy as Copy, GucBcs0RgbaSurface as Surface};
use intel::gpgpu::GpgpuRgba8Surface;
const STRIDE:u64 = 0x10000;
struct Memory {pixels:BTreeMap<u64,Vec<u32>>}
impl Memory {
    fn new()->Self {Self{pixels:BTreeMap::new()}}
    fn surface(&mut self,id:u64,width:u32,height:u32,pixel:u32)->Surface {
        let pitch_bytes = width.div_ceil(16)*64;
        let bytes = pitch_bytes as usize*height as usize;
        self.pixels.insert(id*STRIDE,vec![pixel;bytes/4]);
        Surface{phys:id*STRIDE,gpu:id*STRIDE,bytes,width,height,pitch_bytes}
    }
    fn execute(&mut self,destination:Surface,copies:&[Copy]) {
        for copy in copies {
            assert!(copy.source_x+copy.width<=copy.source.width);
            assert!(copy.source_y+copy.height<=copy.source.height);
            assert!(copy.destination_x+copy.width<=destination.width);
            assert!(copy.destination_y+copy.height<=destination.height);
            let source=self.pixels[&copy.source.phys].clone();
            let dst=self.pixels.get_mut(&destination.phys).unwrap();
            for y in 0..copy.height {
                let src_offset=((copy.source_y+y)*copy.source.pitch_bytes/4+copy.source_x) as usize;
                let dst_offset=((copy.destination_y+y)*destination.pitch_bytes/4+copy.destination_x) as usize;
                dst[dst_offset..dst_offset+copy.width as usize]
                    .copy_from_slice(&source[src_offset..src_offset+copy.width as usize]);
            }
        }
    }
    fn tight(&self,surface:Surface)->Vec<u32> {
        let pixels=&self.pixels[&surface.phys];
        (0..surface.height).flat_map(|y| {
            let start=(y*surface.pitch_bytes/4) as usize;
            pixels[start..start+surface.width as usize].iter().copied()
        }).collect()
    }
    fn expected(&self,base:Surface,tiles:&[RgbaOverlayTile<'_>])->Vec<u32> {
        let mut out=self.tight(base);
        for tile in tiles {
            if tile.opacity==0 {continue;}
            let src=tile.gpgpu_surface.unwrap();
            let pixels=&self.pixels[&src.phys];
            for y in 0..base.height {
                for x in 0..base.width {
                    if x>=tile.x && y>=tile.y && x-tile.x<tile.width && y-tile.y<tile.height
                        && ui4::window_arc::contains(tile.width,tile.height,tile.arc,x-tile.x,y-tile.y) {
                        out[(y*base.width+x) as usize]=pixels[((y-tile.y)*src.pitch_bytes/4+x-tile.x) as usize];
                    }
                }
            }
        }
        out
    }
}
fn tile(source:Surface,x:u32,y:u32)->RgbaOverlayTile<'static> {
    RgbaOverlayTile{x,y,width:source.width,height:source.height,
        source_width:source.width,source_height:source.height,pitch_bytes:source.pitch_bytes as usize,
        pixels:&[],gpgpu_surface:GpgpuRgba8Surface::new(source.phys,source.gpu,source.bytes,
            source.width,source.height,source.pitch_bytes),gpgpu_scanout_cache:false,
        opacity:255,arc:0,expected_rgba:None}
}
fn full(surface:Surface)->CompositionDamageRegion {
    CompositionDamageRegion::from_rect(CompositionDamageRect::new(0,0,surface.width,surface.height))
}
fn placement(tile:&RgbaOverlayTile<'_>)->WindowPlacement {
    WindowPlacement{x:tile.x as i32,y:tile.y as i32,width:tile.width,height:tile.height}
}
fn apply(memory:&mut Memory,base:Surface,destination:Surface,tiles:&[RgbaOverlayTile<'_>],damage:CompositionDamageRegion) {
    let (copies,cache)=ui4_bcs::plan_copies(base,destination,tiles,damage).unwrap();
    assert_eq!(copies.len(),cache.len());
    memory.execute(destination,&copies);
}

#[test]
fn topmost_replaces_all_channels_including_zero_alpha() {
    let mut memory=Memory::new();
    let base=memory.surface(1,8,6,0);
    let destination=memory.surface(2,8,6,0xdeadbeef);
    let lower=memory.surface(3,5,4,0xff804020);
    let upper=memory.surface(4,3,2,0x80402010);
    memory.pixels.get_mut(&upper.phys).unwrap()[1]=0;
    let mut tiles=[tile(lower,1,1),tile(upper,2,2)];
    tiles[1].gpgpu_scanout_cache=true;
    let (copies,cache)=ui4_bcs::plan_copies(base,destination,&tiles,full(base)).unwrap();
    assert_eq!(cache,vec![false,false,true]);
    memory.execute(destination,&copies);
    let actual=memory.tight(destination);
    assert_eq!(actual,memory.expected(base,&tiles));
    assert_eq!(actual[2*8+2],0x80402010);
    assert_eq!(actual[2*8+3],0);
    assert_eq!(memory.tight(lower),vec![0xff804020;20]);
}

#[test]
fn damage_and_output_edges_clip_source_offsets_and_preserve_guards() {
    let mut memory=Memory::new();
    let base=memory.surface(1,8,6,0);
    let destination=memory.surface(2,8,6,0xdeadbeef);
    let source=memory.surface(3,6,5,0xff001122);
    let tiles=[tile(source,5,3)];
    let damage=CompositionDamageRegion::from_rect(CompositionDamageRect::new(6,4,50,50));
    let (copies,_)=ui4_bcs::plan_copies(base,destination,&tiles,damage).unwrap();
    assert_eq!((copies[1].source_x,copies[1].source_y,copies[1].width,copies[1].height),(1,1,2,2));
    memory.execute(destination,&copies);
    for (index,pixel) in memory.tight(destination).iter().enumerate() {
        let (x,y)=(index%8,index/8);
        assert_eq!(*pixel,if x>=6 && y>=4 {0xff001122}else{0xdeadbeef});
    }
    // Surface padding is never part of output damage.
    assert!(memory.pixels[&destination.phys].chunks(16).all(|row|row[8..].iter().all(|p|*p==0xdeadbeef)));
}

#[test]
fn concurrent_moves_and_swap_buffer_debt_repaint_exposed_owners() {
    let mut memory=Memory::new();
    let base=memory.surface(1,32,24,0);
    let buffers=[memory.surface(2,32,24,0),memory.surface(3,32,24,0)];
    let sources=[memory.surface(4,9,7,0xff302010),memory.surface(5,8,6,0x80403020)];
    let scenes=[[(1,1),(5,3)],[(3,2),(8,4)],[(7,6),(11,9)],[(10,7),(6,11)],[(2,12),(18,1)]];
    let mut previous:Option<[(u32,u32);2]>=None;
    let mut debt=[CompositionDamageRegion::EMPTY;2];
    for (step,positions) in scenes.into_iter().enumerate() {
        let tiles=[tile(sources[0],positions[0].0,positions[0].1),tile(sources[1],positions[1].0,positions[1].1)];
        let mut change=CompositionDamageRegion::EMPTY;
        if let Some(old)=previous {
            for index in 0..2 {
                let old_tile=tile(sources[index],old[index].0,old[index].1);
                add_placement_change_damage(&mut change,placement(&old_tile),placement(&tiles[index]),32,24);
            }
        } else {change=full(base);}
        let back=step%2;
        let mut effective=debt[back];
        effective.add_region(change);
        apply(&mut memory,base,buffers[back],&tiles,effective);
        assert_eq!(memory.tight(buffers[back]),memory.expected(base,&tiles),"step {step}");
        debt[back]=CompositionDamageRegion::EMPTY;
        debt[1-back].add_region(change);
        previous=Some(positions);
    }
}

#[test]
fn changing_order_removing_a_frame_and_empty_stack_restore_transparency() {
    let mut memory=Memory::new();
    let base=memory.surface(1,12,9,0);
    let destination=memory.surface(2,12,9,0);
    let a=memory.surface(3,7,6,0xff001122);
    let b=memory.surface(4,6,5,0x80402010);
    for tiles in [vec![tile(a,1,1),tile(b,3,2)],vec![tile(b,3,2),tile(a,1,1)],vec![tile(b,3,2)],vec![]] {
        apply(&mut memory,base,destination,&tiles,full(base));
        assert_eq!(memory.tight(destination),memory.expected(base,&tiles));
    }
}

#[test]
fn rounded_spans_match_the_production_outline_and_coalesce_rows() {
    for width in 1..=24 {
        for height in 1..=24 {
            let mut memory=Memory::new();
            let base=memory.surface(1,width+4,height+4,0);
            let destination=memory.surface(2,width+4,height+4,0);
            let source=memory.surface(3,width,height,0xffc08040);
            for arc in [1,350,1000] {
                let mut tiles=[tile(source,2,2)];tiles[0].arc=arc;
                let (copies,_)=ui4_bcs::plan_copies(base,destination,&tiles,full(base)).unwrap();
                memory.execute(destination,&copies);
                assert_eq!(memory.tight(destination),memory.expected(base,&tiles),"{width}x{height} arc {arc}");
            }
        }
    }
    let mut memory=Memory::new();
    let base=memory.surface(1,100,100,0);
    let destination=memory.surface(2,100,100,0);
    let source=memory.surface(3,80,80,0xffc08040);
    let mut tiles=[tile(source,10,10)];tiles[0].arc=200;
    let (copies,_)=ui4_bcs::plan_copies(base,destination,&tiles,full(base)).unwrap();
    assert!(copies.len()<30,"equal consecutive spans should be one rectangle");
}

#[test]
fn foreground_ownership_skips_unavailable_dimmed_or_scaled_layered_backgrounds() {
    let mut memory=Memory::new();
    let base=memory.surface(1,12,9,0);
    let destination=memory.surface(2,12,9,0);
    let background=memory.surface(3,3,2,0xff001122);
    let foreground=memory.surface(4,7,6,0x80402010);
    memory.pixels.get_mut(&foreground.phys).unwrap()[1]=0;
    for arc in [0,1000] {
        let mut tiles=[tile(background,2,1),tile(foreground,2,1)];
        tiles[0].width=7;tiles[0].height=6;tiles[0].opacity=128;
        tiles[0].gpgpu_surface=None; // An occluded source is never imported.
        tiles[0].arc=arc;tiles[1].arc=arc;
        apply(&mut memory,base,destination,&tiles,full(base));
        assert_eq!(memory.tight(destination),memory.expected(base,&tiles[1..]));
    }
    // A rectangle can hide an enclosed rounded frame with different bounds.
    let mut tiles=[tile(background,3,2),tile(foreground,2,1)];
    tiles[0].opacity=128;tiles[0].arc=1000;tiles[0].gpgpu_surface=None;
    apply(&mut memory,base,destination,&tiles,full(base));
    assert_eq!(memory.tight(destination),memory.expected(base,&tiles[1..]));
    // A rounded foreground leaves lower rectangular corners owned below it.
    tiles[0]=tile(foreground,2,1);tiles[0].opacity=128;tiles[1].arc=1000;
    assert!(ui4_bcs::plan_copies(base,destination,&tiles,full(base)).is_err());
    // A hidden foreground never occludes an unsupported background.
    tiles[1].opacity=0;
    assert!(ui4_bcs::plan_copies(base,destination,&tiles,full(base)).is_err());
}

#[test]
fn unsupported_pixel_operations_are_rejected_and_hidden_frames_are_skipped() {
    let mut memory=Memory::new();
    let base=memory.surface(1,12,9,0);
    let destination=memory.surface(2,12,9,0);
    let source=memory.surface(3,7,6,0xff001122);
    let mut tiles=[tile(source,1,1)];
    tiles[0].width=6;
    assert!(ui4_bcs::plan_copies(base,destination,&tiles,full(base)).is_err());
    tiles[0].width=7;tiles[0].opacity=128;
    assert!(ui4_bcs::plan_copies(base,destination,&tiles,full(base)).is_err());
    tiles[0].opacity=0;tiles[0].gpgpu_surface=None;
    let (copies,_)=ui4_bcs::plan_copies(base,destination,&tiles,full(base)).unwrap();
    assert_eq!(copies.len(),1);
    tiles[0]=tile(destination,0,0);
    assert!(ui4_bcs::plan_copies(base,destination,&tiles,full(base)).is_err());
}
'''
    with tempfile.TemporaryDirectory(prefix='ui4-bcs-stack-') as directory:
        path = Path(directory)
        (path / 'tests.rs').write_text(source)
        subprocess.run(['rustc', '--edition=2024', '--test', str(path / 'tests.rs'),
                        '-o', str(path / 'tests')], cwd=ROOT, check=True)
        subprocess.run([str(path / 'tests')], cwd=ROOT, check=True)


if __name__ == '__main__':
    main()
