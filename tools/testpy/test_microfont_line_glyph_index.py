#!/usr/bin/env python3
"""Compile a discovered glyph index after add/replace/remove, without Cargo."""
from pathlib import Path
import importlib.util
import shutil
import subprocess
import tempfile
import unittest
import unicodedata

ROOT=Path(__file__).resolve().parents[2]
PATH=ROOT/'vendor/microfont/tools/generate_line_glyphs.py'
spec=importlib.util.spec_from_file_location('glyph_index',PATH)
index=importlib.util.module_from_spec(spec)
spec.loader.exec_module(index)

class GlyphIndex(unittest.TestCase):
    def test_lowercase_grid_matches_unicode_and_covers_cp850(self):
        marks = '\u0308\u0301\u0300\u0302\u030a\u0303'
        expected = {}
        for base in 'aeiouyn':
            for mark, topper in zip(marks, index.TOPPERS):
                ch = unicodedata.normalize('NFC', base + mark)
                if len(ch) == 1:
                    expected[ch] = (base, topper)
        self.assertEqual(index.COMPOSITIONS, expected)
        self.assertEqual(len(expected), 36)
        cp850 = set(bytes(range(256)).decode('cp850'))
        accented = {ch for ch in cp850 if len(unicodedata.normalize('NFD', ch)) == 2
                    and unicodedata.normalize('NFD', ch)[0] in 'aeiouyn'
                    and unicodedata.normalize('NFD', ch)[1] in marks}
        self.assertEqual(len(accented), 26)
        self.assertTrue(accented <= expected.keys())
        self.assertIn('ã', accented)
        self.assertTrue(all(ch.islower() for ch in expected))

    def test_lowercase_compositions_borrow_bases_and_independent_toppers(self):
        with tempfile.TemporaryDirectory(prefix='line-toppers-') as tmp:
            root = Path(tmp)
            data = root/'data'
            src = root/'src'
            src.mkdir()
            shutil.copytree(ROOT/'vendor/microfont/data/line-v1', data)
            for name in ['raster_line.rs', '7seg.rs']:
                shutil.copyfile(ROOT/'vendor/microfont/src'/name, src/name)
            output = src/'generated_line_glyphs.rs'
            pairs = ','.join(f"('\\u{{{ord(ch):X}}}','{base}')"
                             for ch, (base, _) in index.COMPOSITIONS.items())
            by_topper = [[ch for ch, (_, name) in index.COMPOSITIONS.items() if name == topper]
                         for topper in index.TOPPERS]
            topper_groups = ','.join('&[' + ','.join(f"'\\u{{{ord(ch):X}}}'" for ch in group) + ']'
                                    for group in by_topper)
            check = root/'check.rs'
            def compile_and_run(body):
                index.generate(data, output)
                check.write_text('#[path="src/raster_line.rs"] mod raster_line;\n'
                    '#[path="src/7seg.rs"] mod seven_segment;\n'
                    'use raster_line::glyph_strips;\nfn main() {\n' + body + '\n}')
                subprocess.run(['rustc', '--edition=2024', '--cfg', 'feature="raster"',
                                '-A', 'dead_code', str(check), '-o', str(root/'check')], check=True)
                subprocess.run([str(root/'check')], check=True)
            compile_and_run('for (ch, base) in [' + pairs + r'''] {
    let plain = glyph_strips(base).unwrap();
    let body: Vec<_> = plain.iter().filter(|s| base != 'i' || s.iter_points().any(|p|p.y() >= 4)).collect();
    let composed = glyph_strips(ch).unwrap();
    assert!(composed.len() > body.len());
    assert!(raster_line::GLYPH_CHARACTERS.contains(&ch));
    for (a,b) in body.iter().zip(composed) {
        assert_eq!(a.points.as_ptr(), b.points.as_ptr());
        assert_eq!((a.start,a.end), (b.start,b.end));
        assert!(a.iter_points().eq(b.iter_points()));
    }
    assert!(composed.iter().skip(body.len()).flat_map(|s|s.iter_points()).all(|p|p.y() < 4));
}
assert!(glyph_strips('i').unwrap().iter().any(|s|s.iter_points().all(|p|p.y() < 4)));
let groups: &[&[char]] = &[''' + topper_groups + r'''];
for group in groups {
    let canonical = glyph_strips(group[0]).unwrap();
    let last = canonical[canonical.len()-1];
    for &ch in *group {
        let glyph = glyph_strips(ch).unwrap();
        assert_eq!(last.points.as_ptr(), glyph[glyph.len()-1].points.as_ptr());
    }
}
// The six private assets never create standalone accent/combining characters.
for ch in ['\u{0308}','\u{0301}','\u{0300}','\u{0302}','\u{030a}','\u{0303}'] {
    assert!(glyph_strips(ch).is_none());
}
let acute = glyph_strips('á').unwrap();
assert_ne!(acute[acute.len()-1].points.as_ptr(), glyph_strips('´').unwrap()[0].points.as_ptr());
assert!(glyph_strips('a').unwrap().get(99).is_none());
// Explicit component composition supports all 42 pairs, even where no single
// precomposed character exists. Ordinary character lookup stays unambiguous.
use raster_line::{lowercase_topped_strips, LowercaseTopper::*};
for base in ['a','e','i','o','u','y','n'] {
    for topper in [Diaeresis,Acute,Grave,Circumflex,Ring,Tilde] {
        let glyph = lowercase_topped_strips(base,topper).unwrap();
        assert!(!glyph.is_empty());
        let plain = glyph_strips(base).unwrap();
        assert_eq!(glyph[0].points.as_ptr(),plain[0].points.as_ptr());
    }
}
for base in ['A','E','I','O','U','Y','N','z'] {
    assert!(lowercase_topped_strips(base,Acute).is_none());
}
''')
            # Editing one topper changes every mapped base; editing a base
            # changes every composed form without writing any composed bin.
            (data/'lowercase-toppers/acute.bin').write_bytes(bytes.fromhex('C0 00 04'))
            (data/'u0061.bin').write_bytes(bytes.fromhex('C0 20 44'))
            compile_and_run(r'''
for ch in ['á','é','í','ó','ú','ý','ń'] {
    let glyph = glyph_strips(ch).unwrap();
    assert_eq!(glyph[glyph.len()-1].points[0].packed(), 0);
    assert_eq!(glyph[glyph.len()-1].points[1].packed(), 4);
}
for ch in ['ä','á','à','â','å','ã'] {
    assert_eq!(glyph_strips(ch).unwrap()[0].points[0].packed(), 0x20);
}
''')
            (data/'lowercase-toppers/acute.bin').unlink()
            compile_and_run("for ch in ['á','é','í','ó','ú','ý','ń'] { assert!(glyph_strips(ch).is_none()); }"
                            "assert!(glyph_strips('ä').is_some());")
            # Don't silently accept a duplicate full-glyph bin later.
            (data/'u00E4.bin').write_bytes(bytes.fromhex('C0 00 40'))
            with self.assertRaisesRegex(ValueError, 'composed lowercase glyph'):
                index.generate(data, output)

    def test_new_changed_and_removed_bins_are_reflected_in_compiled_lookup(self):
        for character in ['Z','0','a','@','%','$']:
            with tempfile.TemporaryDirectory(prefix='line-index-') as tmp:
                root=Path(tmp);data=root/'data';data.mkdir();src=root/'src';src.mkdir()
                output=src/'generated_line_glyphs.rs'
                (src/'raster_line.rs').write_text((ROOT/'vendor/microfont/src/raster_line.rs').read_text())
                (src/'7seg.rs').write_text((ROOT/'vendor/microfont/src/7seg.rs').read_text())
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
                    self.assertEqual(index.generate(data,output),10+int(expected>0))
                    check.write_text(f'#[path="src/raster_line.rs"] mod raster_line;\n'
                        '#[path="src/7seg.rs"] mod seven_segment;\n'
                        f'fn main() {{ assert_eq!(raster_line::glyph_strips(\'{character}\').map_or(0, |s|s.len()),{expected});'
                        'assert!(raster_line::glyph_strips(\'D\').is_none()); }\n')
                    subprocess.run(['rustc','--edition=2024','--cfg','feature="raster"','-A','dead_code',str(check),'-o',str(root/'check')],check=True)
                    subprocess.run([str(root/'check')],check=True)

    def test_flipped_aliases_share_bytes_and_apply_to_all_strips(self):
        with tempfile.TemporaryDirectory(prefix='line-alias-') as tmp:
            root=Path(tmp);data=root/'data';data.mkdir();src=root/'src';src.mkdir()
            (src/'raster_line.rs').write_text((ROOT/'vendor/microfont/src/raster_line.rs').read_text())
            (src/'7seg.rs').write_text((ROOT/'vendor/microfont/src/7seg.rs').read_text())
            (data/'u0036.bin').write_bytes(bytes.fromhex('C0 00 44'))
            (data/'u0062.bin').write_bytes(bytes.fromhex('40 11 34 80 00 44'))
            (data/'u0070.bin').write_bytes(bytes.fromhex('C1 21 51 3C'))
            (data/'u006E.bin').write_bytes(bytes.fromhex('C0 40 20 83 28 21 23 2C 44'))
            (data/'u0074.bin').write_bytes(bytes.fromhex('C0 21 23 C2 02 3A 43 44'))
            for name, payload in {
                '0021':'C0 02 32 C0 42 42', '0028':'C2 04 0B 3B 44', '003C':'C1 04 20 44',
                '005B':'C2 04 00 40 44', '007B':'C1 04 22 44',
                '002F':'C1 04 22 40', '003F':'C1 00 24 42',
                '2192':'C1 1B 2D 3B C0 28 2D', '25B2':'02 30 34 12 30',
            }.items():
                (data/f'u{name}.bin').write_bytes(bytes.fromhex(payload))
            self.assertEqual(index.generate(data,src/'generated_line_glyphs.rs'),42)
            check=root/'check.rs'
            check.write_text(r'''#[path="src/raster_line.rs"] mod raster_line;
#[path="src/7seg.rs"] mod seven_segment;
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
    let upright=glyph_strips('!').unwrap();
    let inverted=glyph_strips('¡').unwrap();
    assert_eq!(upright.len(),inverted.len());
    let points:Vec<_>=upright.iter().flat_map(|s|s.iter_points()).collect();
    let sy=points.iter().map(|p|p.y()).min().unwrap()+points.iter().map(|p|p.y()).max().unwrap();
    for (a,b) in upright.iter().zip(inverted) {
        assert_eq!(a.points.as_ptr(),b.points.as_ptr());
        assert_eq!((a.start,a.end),(b.start,b.end));
        for (p,q) in a.iter_points().zip(b.iter_points()) {
            assert_eq!((q.x(),q.y()),(p.x(),sy-p.y()));
        }
    }
    let right=glyph_strips('→').unwrap();
    for ch in ['←','↑','↓'] {
        let alias=glyph_strips(ch).unwrap();
        for (a,b) in right.iter().zip(alias) {
            assert_eq!(a.points.as_ptr(),b.points.as_ptr());
            assert_eq!((a.start,a.end),(b.start,b.end));
        }
    }
    for (ch,head,stem) in [
        ('←',[(2,3),(0,5),(2,7)],[(5,5),(0,5)]),
        ('↑',[(0,4),(2,2),(4,4)],[(2,7),(2,2)]),
        ('↓',[(4,5),(2,7),(0,5)],[(2,2),(2,7)]),
    ] {
        let glyph=glyph_strips(ch).unwrap();
        assert_eq!(glyph[0].iter_points().map(|p|(p.x(),p.y())).collect::<Vec<_>>(),head);
        assert_eq!(glyph[1].iter_points().map(|p|(p.x(),p.y())).collect::<Vec<_>>(),stem);
        assert_eq!(glyph[1].adjacency_vertex(1).unwrap(),[stem[0].0 as f32,stem[0].1 as f32,0.5]);
    }
    let up=glyph_strips('▲').unwrap();
    for (ch,expected) in [
        ('►',[(0,2),(0,6),(4,4),(0,2)]),
        ('◄',[(4,6),(4,2),(0,4),(4,6)]),
        ('▼',[(4,2),(0,2),(2,6),(4,2)]),
    ] {
        let alias=glyph_strips(ch).unwrap();
        assert_eq!(up[0].points.as_ptr(),alias[0].points.as_ptr());
        assert_eq!((alias[0].start,alias[0].end),(Junction,Junction));
        let points:Vec<_>=alias[0].iter_points().map(|p|(p.x(),p.y())).collect();
        assert_eq!(points,expected);
        assert_eq!(points.first(),points.last());
        assert_eq!(points.iter().map(|p|p.0).min(),Some(0));
        assert_eq!(points.iter().map(|p|p.0).max(),Some(4));
        assert_eq!(points.iter().map(|p|p.1).min(),Some(2));
        assert_eq!(points.iter().map(|p|p.1).max(),Some(6));
    }
    assert_eq!(core::mem::size_of::<raster_line::Strip>(),24);
}
''')
            subprocess.run(['rustc','--edition=2024','--cfg','feature="raster"','-A','dead_code',str(check),'-o',str(root/'check')],check=True)
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

    def test_private_toppers_validate_the_same_binary_format(self):
        with tempfile.TemporaryDirectory(prefix='line-topper-bad-') as tmp:
            folder = Path(tmp)
            toppers = folder/'lowercase-toppers'
            toppers.mkdir()
            path = toppers/'acute.bin'
            for contents in [b'', bytes.fromhex('C1 00 40'), bytes.fromhex('C0 06 40')]:
                path.write_bytes(contents)
                with self.assertRaisesRegex(ValueError, 'acute.bin:'):
                    index.generate(folder, folder/'index.rs')
            path.unlink()
            (toppers/'u0301.bin').write_bytes(bytes.fromhex('C0 00 40'))
            with self.assertRaisesRegex(ValueError, 'six lowercase topper names'):
                index.generate(folder, folder/'index.rs')

if __name__=='__main__':
    unittest.main()
