#!/usr/bin/env python3
"""Compile a discovered glyph index after add/replace/remove, without Cargo."""
from pathlib import Path
import importlib.util
import subprocess
import tempfile
import unittest

ROOT=Path(__file__).resolve().parents[2]
PATH=ROOT/'vendor/microfont/tools/generate_line_glyphs.py'
spec=importlib.util.spec_from_file_location('glyph_index',PATH)
index=importlib.util.module_from_spec(spec)
spec.loader.exec_module(index)

class GlyphIndex(unittest.TestCase):
    def test_new_changed_and_removed_bins_are_reflected_in_compiled_lookup(self):
        for character in ['Z','0','a','@','%','$']:
            with tempfile.TemporaryDirectory(prefix='line-index-') as tmp:
                root=Path(tmp);data=root/'data';data.mkdir();src=root/'src';src.mkdir()
                output=src/'generated_line_glyphs.rs'
                (src/'raster_line.rs').write_text((ROOT/'vendor/microfont/src/raster_line.rs').read_text())
                check=root/'check.rs'
                # Probe missing letters first, then add Z, change its strip count,
                # then remove it. The production decoder and generated lookup run.
                for contents,expected in [(None,0),(bytes.fromhex('C0 00 40'),1),
                                          (bytes.fromhex('C0 00 40 80 20 44'),2),(None,0)]:
                    z=data/f'u{ord(character):04X}.bin'
                    if contents is None:
                        z.unlink(missing_ok=True)
                    else:
                        z.write_bytes(contents)
                    self.assertEqual(index.generate(data,output),int(expected>0))
                    check.write_text(f'#[path="src/raster_line.rs"] mod raster_line;\n'
                        f'fn main() {{ assert_eq!(raster_line::glyph_strips(\'{character}\').map_or(0, |s|s.len()),{expected});'
                        'assert!(raster_line::glyph_strips(\'D\').is_none()); }\n')
                    subprocess.run(['rustc','--edition=2024','-A','dead_code',str(check),'-o',str(root/'check')],check=True)
                    subprocess.run([str(root/'check')],check=True)

    def test_flipped_aliases_share_bytes_and_apply_to_all_strips(self):
        with tempfile.TemporaryDirectory(prefix='line-alias-') as tmp:
            root=Path(tmp);data=root/'data';data.mkdir();src=root/'src';src.mkdir()
            (src/'raster_line.rs').write_text((ROOT/'vendor/microfont/src/raster_line.rs').read_text())
            (data/'u0036.bin').write_bytes(bytes.fromhex('C0 00 44'))
            (data/'u0062.bin').write_bytes(bytes.fromhex('40 11 34 80 00 44'))
            (data/'u0070.bin').write_bytes(bytes.fromhex('C1 21 51 3C'))
            (data/'u006E.bin').write_bytes(bytes.fromhex('C0 40 20 83 28 21 23 2C 44'))
            (data/'u0074.bin').write_bytes(bytes.fromhex('C0 21 23 C2 02 3A 43 44'))
            for name, payload in {
                '0028':'C2 04 0B 3B 44', '003C':'C1 04 20 44',
                '005B':'C2 04 00 40 44', '007B':'C1 04 22 44',
                '002F':'C1 04 22 40', '003F':'C1 00 24 42',
            }.items():
                (data/f'u{name}.bin').write_bytes(bytes.fromhex(payload))
            self.assertEqual(index.generate(data,src/'generated_line_glyphs.rs'),22)
            check=root/'check.rs'
            check.write_text(r'''#[path="src/raster_line.rs"] mod raster_line;
use raster_line::{glyph_strips,Boundary::{End,Junction}};
fn main() {
    let six=glyph_strips('6').unwrap();let nine=glyph_strips('9').unwrap();
    assert_eq!(six[0].points.as_ptr(),nine[0].points.as_ptr());
    let p=nine[0].point(0).unwrap();assert_eq!((p.x(),p.y()),(4,8));
    let p=nine[0].point(1).unwrap();assert_eq!((p.x(),p.y()),(0,0));
    assert_eq!(nine[0].adjacency_vertex(0),Some([4.0,8.0,1.0]));
    let b=glyph_strips('b').unwrap();let d=glyph_strips('d').unwrap();
    for i in 0..2 {assert_eq!(b[i].points.as_ptr(),d[i].points.as_ptr());}
    let actual:Vec<_>=d[0].iter_points().map(|p|(p.x(),p.y())).collect();
    assert_eq!(actual,[(3,2),(0,6)]);
    let actual:Vec<_>=d[1].iter_points().map(|p|(p.x(),p.y())).collect();
    assert_eq!(actual,[(4,0),(0,8)]);
    assert_eq!((d[0].start,d[0].end),(End,Junction));
    assert_eq!((d[1].start,d[1].end),(Junction,End));
    assert_eq!(b[0].point(0).unwrap().packed(),0x11);
    let p=glyph_strips('p').unwrap();let q=glyph_strips('q').unwrap();
    assert_eq!(p[0].points.as_ptr(),q[0].points.as_ptr());
    let actual:Vec<_>=q[0].iter_points().map(|p|(p.x(),p.y())).collect();
    assert_eq!(actual,[(4,4),(4,10),(1,7)]);
    assert_eq!((q[0].start,q[0].end),(p[0].start,p[0].end));
    assert_eq!(q[0].adjacency_vertex(0),Some([4.0,4.0,1.0]));
    let n=glyph_strips('n').unwrap();let u=glyph_strips('u').unwrap();
    for i in 0..2 {
        assert_eq!(n[i].points.as_ptr(),u[i].points.as_ptr());
        assert_eq!((n[i].start,n[i].end),(u[i].start,u[i].end));
    }
    let actual:Vec<_>=u[0].iter_points().map(|p|(p.x(),p.y())).collect();
    assert_eq!(actual,[(4,4),(4,8)]);
    let actual:Vec<_>=u[1].iter_points().map(|p|(p.x(),p.y())).collect();
    assert_eq!(actual,[(4,7),(3,8),(1,8),(0,7),(0,4)]);
    let t=glyph_strips('t').unwrap();let f=glyph_strips('f').unwrap();
    for i in 0..2 {
        assert_eq!(t[i].points.as_ptr(),f[i].points.as_ptr());
        assert_eq!((t[i].start,t[i].end),(f[i].start,f[i].end));
    }
    let actual:Vec<_>=f[0].iter_points().map(|p|(p.x(),p.y())).collect();
    assert_eq!(actual,[(1,4),(3,4)]);
    let actual:Vec<_>=f[1].iter_points().map(|p|(p.x(),p.y())).collect();
    assert_eq!(actual,[(2,8),(2,1),(3,0),(4,0)]);
    for (canonical,alias,rotate) in [('(',')',false),('<','>',false),('[',']',false),
                                    ('{','}',false),('/','\\',false),('?','¿',true)] {
        let source=glyph_strips(canonical).unwrap();let flipped=glyph_strips(alias).unwrap();
        let all:Vec<_>=source.iter().flat_map(|s|s.iter_points()).collect();
        let sx=all.iter().map(|p|p.x()).min().unwrap()+all.iter().map(|p|p.x()).max().unwrap();
        let sy=all.iter().map(|p|p.y()).min().unwrap()+all.iter().map(|p|p.y()).max().unwrap();
        for (a,b) in source.iter().zip(flipped) {
            assert_eq!(a.points.as_ptr(),b.points.as_ptr());
            assert_eq!((a.start,a.end),(b.start,b.end));
            for (p,q) in a.iter_points().zip(b.iter_points()) {
                assert_eq!((q.x(),q.y()),(sx-p.x(),if rotate {sy-p.y()} else {p.y()}));
            }
        }
    }
    assert_eq!(glyph_strips('/').unwrap()[0].points.len(),3); // bin overrides old slash
    assert_eq!(core::mem::size_of::<raster_line::Strip>(),24);
}
''')
            subprocess.run(['rustc','--edition=2024','-A','dead_code',str(check),'-o',str(root/'check')],check=True)
            subprocess.run([str(root/'check')],check=True)

    def test_only_canonical_codepoint_filenames_are_accepted(self):
        with tempfile.TemporaryDirectory(prefix='line-names-') as tmp:
            folder=Path(tmp)
            for name in ['A.bin','u006e.bin','U0041.bin','u00041.bin','uD800.bin','u110000.bin']:
                path=folder/name;path.write_bytes(bytes.fromhex('C0 00 40'))
                with self.assertRaises(ValueError):
                    index.generate(folder,folder/'index.rs')
                path.unlink()

    def test_invalid_bins_report_the_offending_file(self):
        with tempfile.TemporaryDirectory(prefix='line-index-bad-') as tmp:
            folder=Path(tmp);z=folder/'u005A.bin'
            for contents in [b'',bytes.fromhex('C1 00 40'),bytes.fromhex('C0 06 40'),bytes.fromhex('C0 80 40')]:
                z.write_bytes(contents)
                with self.assertRaisesRegex(ValueError,'u005A.bin:'):
                    index.generate(folder,folder/'index.rs')

if __name__=='__main__':
    unittest.main()
