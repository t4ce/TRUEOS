#!/usr/bin/env python3
"""Exercise the production binary reader after source add/replace/remove edits."""
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import unittest
import unicodedata
from _microfont_build import build_microfont

ROOT = Path(__file__).resolve().parents[2]
DATA = ROOT / 'vendor/microfont/data/line-v1'
TOPPERS = ('diaeresis', 'acute', 'grave', 'circumflex', 'ring', 'tilde')
ROWS = {'a':'äáàâåã','e':'ëéèê ẽ','i':'ïíìî ĩ','o':'öóòô õ',
        'u':'üúùûůũ','y':'ÿýỳŷẙỹ','n':' ńǹ  ñ'}
COMPOSITIONS = {ch:(base,topper) for base,row in ROWS.items()
                for ch,topper in zip(row,TOPPERS) if ch != ' '}
ALIASES = {
    '¡':('!',2),'9':('6',4),'d':('b',1),'q':('p',1),'u':('n',4),'f':('t',2),
    ')':('(',1),'>':('<',1),']':('[',1),'}':('{',1),'\\':('/',1),'¿':('?',4),
    '»':('«',1),'←':('→',1),'↑':('→',5),'↓':('→',3),
    '►':('▲',3),'◄':('▲',5),'▼':('▲',4),
}

def write_record(path, payload, kind=1):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(bytes(16) + bytes([1,kind,0,0]) + payload)

def compile_and_run(folder, body):
    output = folder.parent/'build'
    output.mkdir(exist_ok=True)
    lib = build_microfont(output/'libmicrofont.rlib', features=('hex','raster'), source_dir=folder)
    check = output/'check.rs'
    check.write_text('use microfont::raster_line;\nuse raster_line::glyph_strips;\nfn main() {\n' + body + '\n}')
    subprocess.run(['rustc','--edition=2024','-A','unused_imports','--extern',f'microfont={lib}',
                    str(check),'-o',str(output/'check')], check=True)
    subprocess.run([str(output/'check')], check=True)

class GlyphIndex(unittest.TestCase):
    def test_cp850_solid_symbols_are_single_filled_quads(self):
        with tempfile.TemporaryDirectory(prefix='cp850-solid-quads-') as tmp:
            root = Path(tmp)
            data = root/'data'
            shutil.copytree(DATA, data)
            compile_and_run(data, r'''
use microfont::raster_quad::{glyph_quads,GLYPH_CHARACTERS};
assert_eq!(microfont::CP850_CHARACTERS[0x16],'▬');
assert_eq!(microfont::CP850_CHARACTERS[0xfe],'■');
for (ch,top,bottom,reference) in [('▬',4.0,7.0,'━'),('■',2.0,7.0,'■')] {
    assert!(GLYPH_CHARACTERS.contains(&ch));
    assert!(glyph_strips(ch).is_none());
    let quads=glyph_quads(ch).unwrap();
    assert_eq!(quads.len(),1);
    let quad=quads.get(0).unwrap();
    assert_eq!(quad.vertices,[[0.0,top],[6.0,top],[6.0,bottom],[0.0,bottom]]);
    let mut pixels=0u128;
    for y in top as usize..bottom as usize {
        for x in 0..6 { pixels |= 1u128 << (65-y*6-x); }
    }
    assert_eq!(pixels,microfont::bundle::bitmap(reference).unwrap());
}
// Raster additions leave the independently authored bitmap fields intact.
assert_eq!(microfont::bundle::bitmap('▬'),Some(0));
''')

    def test_lowercase_grid_matches_unicode_and_covers_cp850(self):
        marks = '\u0308\u0301\u0300\u0302\u030a\u0303'
        expected = {}
        for base in 'aeiouyn':
            for mark, topper in zip(marks, TOPPERS):
                ch = unicodedata.normalize('NFC', base + mark)
                if len(ch) == 1:
                    expected[ch] = (base, topper)
        self.assertEqual(COMPOSITIONS, expected)
        self.assertEqual(len(expected), 36)
        cp850 = set(bytes(range(256)).decode('cp850'))
        accented = {ch for ch in cp850 if len(unicodedata.normalize('NFD', ch)) == 2
                    and unicodedata.normalize('NFD', ch)[0] in 'aeiouyn'
                    and unicodedata.normalize('NFD', ch)[1] in marks}
        self.assertEqual(len(accented), 26)
        self.assertTrue(accented <= expected.keys())
        self.assertIn('ã', accented)
        for ch, (base, topper) in expected.items():
            record = (DATA/f'u{ord(ch):04X}.bin').read_bytes()
            self.assertEqual(record[17], 4)
            self.assertEqual(struct.unpack('<IIB', record[20:]),
                             (ord(base), 0x110000 + TOPPERS.index(topper), int(base == 'i')))

    def test_lowercase_compositions_borrow_bases_and_independent_toppers(self):
        with tempfile.TemporaryDirectory(prefix='line-toppers-') as tmp:
            data = Path(tmp)/'data'
            shutil.copytree(DATA, data)
            pairs = ','.join(f"('\\u{{{ord(ch):X}}}','{base}')" for ch,(base,_) in COMPOSITIONS.items())
            compile_and_run(data, 'for (ch,base) in [' + pairs + r'''
] {
    let plain = glyph_strips(base).unwrap();
    let body:Vec<_> = plain.iter().filter(|s|base!='i'||s.iter_points().any(|p|p.y()>=4)).collect();
    let composed = glyph_strips(ch).unwrap();
    assert!(composed.len()>body.len());
    assert!(raster_line::GLYPH_CHARACTERS.contains(&ch));
    for (a,b) in body.iter().zip(composed) {
        assert_eq!(a.points.as_ptr(),b.points.as_ptr());
        assert_eq!((a.start,a.end),(b.start,b.end));
        assert!(a.iter_points().eq(b.iter_points()));
    }
    assert!(composed.iter().skip(body.len()).flat_map(|s|s.iter_points()).all(|p|p.y()<4));
}
assert!(glyph_strips('i').unwrap().iter().any(|s|s.iter_points().all(|p|p.y()<4)));
let acute=glyph_strips('á').unwrap();
let last=acute.get(acute.len()-1).unwrap();
assert_ne!(last.points.as_ptr(),glyph_strips('´').unwrap().get(0).unwrap().points.as_ptr());
for ch in ['é','í','ó','ú','ý','ń'] {
    let glyph=glyph_strips(ch).unwrap();
    assert_eq!(last.points.as_ptr(),glyph.get(glyph.len()-1).unwrap().points.as_ptr());
}
for ch in ['\u{0308}','\u{0301}','\u{0300}','\u{0302}','\u{030a}','\u{0303}'] {
    assert!(glyph_strips(ch).is_none());
}
use raster_line::{lowercase_topped_strips,LowercaseTopper::*};
for base in ['a','e','i','o','u','y','n'] {
    for topper in [Diaeresis,Acute,Grave,Circumflex,Ring,Tilde] {
        let glyph=lowercase_topped_strips(base,topper).unwrap();
        assert!(!glyph.is_empty());
        assert_eq!(glyph.get(0).unwrap().points.as_ptr(),glyph_strips(base).unwrap().get(0).unwrap().points.as_ptr());
    }
}
for base in ['A','E','I','O','U','Y','N','z'] {
    assert!(lowercase_topped_strips(base,Acute).is_none());
}
assert!(glyph_strips('a').unwrap().get(99).is_none());
''')
            for relative,payload in [('lowercase-toppers/acute.bin','C0 00 04'),('u0061.bin','C0 20 44')]:
                path=data/relative
                header=path.read_bytes()[:20]
                path.write_bytes(header+bytes.fromhex(payload))
            compile_and_run(data, r'''
for ch in ['á','é','í','ó','ú','ý','ń'] {
    let glyph=glyph_strips(ch).unwrap();
    let topper=glyph.get(glyph.len()-1).unwrap();
    assert_eq!(topper.points[0].packed(),0);
    assert_eq!(topper.points[1].packed(),4);
}
for ch in ['ä','á','à','â','å','ã'] {
    assert_eq!(glyph_strips(ch).unwrap().get(0).unwrap().points[0].packed(),0x20);
}
''')

    def test_new_changed_and_removed_bins_are_reflected_in_compiled_lookup(self):
        with tempfile.TemporaryDirectory(prefix='line-index-') as tmp:
            data=Path(tmp)/'data'
            data.mkdir()
            write_record(data/'u0021.bin',bytes.fromhex('C0 02 32'))
            for payload,count in [(None,0),('C0 00 40',1),('C0 00 40 80 20 44',2),(None,0)]:
                path=data/'u005A.bin'
                if payload is None:
                    path.unlink(missing_ok=True)
                else:
                    write_record(path,bytes.fromhex(payload))
                compile_and_run(data,f"assert_eq!(glyph_strips('Z').map_or(0,|s|s.len()),{count});"
                                     "assert!(glyph_strips('D').is_none());")

    def test_flipped_aliases_share_bytes_and_apply_to_all_strips(self):
        with tempfile.TemporaryDirectory(prefix='line-alias-') as tmp:
            data=Path(tmp)/'data'
            data.mkdir()
            canonical={
                '0036':'C0 00 44','0062':'40 11 34 80 00 44','0070':'C1 21 51 3C',
                '006E':'C0 40 20 83 28 21 23 2C 44','0074':'C0 21 23 C2 02 3A 43 44',
                '0021':'C0 02 32 C0 42 42','0028':'C2 04 0B 3B 44','003C':'C1 04 20 44',
                '005B':'C2 04 00 40 44','007B':'C1 04 22 44','002F':'C1 04 22 40',
                '003F':'C1 00 24 42','2192':'C1 1B 2D 3B C0 28 2D','25B2':'02 30 34 12 30',
            }
            for code,payload in canonical.items():
                write_record(data/f'u{code}.bin',bytes.fromhex(payload))
            for ch,(source,transform) in ALIASES.items():
                if (data/f'u{ord(source):04X}.bin').exists():
                    write_record(data/f'u{ord(ch):04X}.bin',struct.pack('<IB',ord(source),transform),3)
            compile_and_run(data,r'''
use raster_line::Boundary::{End,Junction};
    let six=glyph_strips('6').unwrap();let nine=glyph_strips('9').unwrap();
    assert_eq!(six.get(0).unwrap().points.as_ptr(),nine.get(0).unwrap().points.as_ptr());
    let p=nine.get(0).unwrap().point(0).unwrap();assert_eq!((p.x(),p.y()),(4,8));
    let p=nine.get(0).unwrap().point(1).unwrap();assert_eq!((p.x(),p.y()),(0,0));
    assert_eq!(nine.get(0).unwrap().adjacency_vertex(0),Some([4.0,8.0,1.0]));
    let b=glyph_strips('b').unwrap();let d=glyph_strips('d').unwrap();
    for i in 0..2 {assert_eq!(b.get(i).unwrap().points.as_ptr(),d.get(i).unwrap().points.as_ptr());}
    let actual:Vec<_>=d.get(0).unwrap().iter_points().map(|p|(p.x(),p.y())).collect();
    assert_eq!(actual,[(3,2),(0,6)]);
    let actual:Vec<_>=d.get(1).unwrap().iter_points().map(|p|(p.x(),p.y())).collect();
    assert_eq!(actual,[(4,0),(0,8)]);
    assert_eq!((d.get(0).unwrap().start,d.get(0).unwrap().end),(End,Junction));
    assert_eq!((d.get(1).unwrap().start,d.get(1).unwrap().end),(Junction,End));
    assert_eq!(b.get(0).unwrap().point(0).unwrap().packed(),0x11);
    let p=glyph_strips('p').unwrap();let q=glyph_strips('q').unwrap();
    assert_eq!(p.get(0).unwrap().points.as_ptr(),q.get(0).unwrap().points.as_ptr());
    let actual:Vec<_>=q.get(0).unwrap().iter_points().map(|p|(p.x(),p.y())).collect();
    assert_eq!(actual,[(4,4),(4,10),(1,7)]);
    assert_eq!((q.get(0).unwrap().start,q.get(0).unwrap().end),(p.get(0).unwrap().start,p.get(0).unwrap().end));
    assert_eq!(q.get(0).unwrap().adjacency_vertex(0),Some([4.0,4.0,1.0]));
    let n=glyph_strips('n').unwrap();let u=glyph_strips('u').unwrap();
    for i in 0..2 {
        assert_eq!(n.get(i).unwrap().points.as_ptr(),u.get(i).unwrap().points.as_ptr());
        assert_eq!((n.get(i).unwrap().start,n.get(i).unwrap().end),(u.get(i).unwrap().start,u.get(i).unwrap().end));
    }
    let actual:Vec<_>=u.get(0).unwrap().iter_points().map(|p|(p.x(),p.y())).collect();
    assert_eq!(actual,[(4,4),(4,8)]);
    let actual:Vec<_>=u.get(1).unwrap().iter_points().map(|p|(p.x(),p.y())).collect();
    assert_eq!(actual,[(4,7),(3,8),(1,8),(0,7),(0,4)]);
    let t=glyph_strips('t').unwrap();let f=glyph_strips('f').unwrap();
    for i in 0..2 {
        assert_eq!(t.get(i).unwrap().points.as_ptr(),f.get(i).unwrap().points.as_ptr());
        assert_eq!((t.get(i).unwrap().start,t.get(i).unwrap().end),(f.get(i).unwrap().start,f.get(i).unwrap().end));
    }
    let actual:Vec<_>=f.get(0).unwrap().iter_points().map(|p|(p.x(),p.y())).collect();
    assert_eq!(actual,[(1,4),(3,4)]);
    let actual:Vec<_>=f.get(1).unwrap().iter_points().map(|p|(p.x(),p.y())).collect();
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
    assert_eq!(glyph_strips('/').unwrap().get(0).unwrap().points.len(),3); // bin overrides old slash
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
        assert_eq!(glyph.get(0).unwrap().iter_points().map(|p|(p.x(),p.y())).collect::<Vec<_>>(),head);
        assert_eq!(glyph.get(1).unwrap().iter_points().map(|p|(p.x(),p.y())).collect::<Vec<_>>(),stem);
        assert_eq!(glyph.get(1).unwrap().adjacency_vertex(1).unwrap(),[stem[0].0 as f32,stem[0].1 as f32,0.5]);
    }
    let up=glyph_strips('▲').unwrap();
    for (ch,expected) in [
        ('►',[(0,2),(0,6),(4,4),(0,2)]),
        ('◄',[(4,6),(4,2),(0,4),(4,6)]),
        ('▼',[(4,2),(0,2),(2,6),(4,2)]),
    ] {
        let alias=glyph_strips(ch).unwrap();
        assert_eq!(up.get(0).unwrap().points.as_ptr(),alias.get(0).unwrap().points.as_ptr());
        assert_eq!((alias.get(0).unwrap().start,alias.get(0).unwrap().end),(Junction,Junction));
        let points:Vec<_>=alias.get(0).unwrap().iter_points().map(|p|(p.x(),p.y())).collect();
        assert_eq!(points,expected);
        assert_eq!(points.first(),points.last());
        assert_eq!(points.iter().map(|p|p.0).min(),Some(0));
        assert_eq!(points.iter().map(|p|p.0).max(),Some(4));
        assert_eq!(points.iter().map(|p|p.1).min(),Some(2));
        assert_eq!(points.iter().map(|p|p.1).max(),Some(6));
    }
    assert_eq!(core::mem::size_of::<raster_line::Strip>(),24);
''')

    def test_invalid_source_records_fail_the_real_packer(self):
        with tempfile.TemporaryDirectory(prefix='line-invalid-') as tmp:
            data=Path(tmp)/'data';data.mkdir()
            for payload in [bytes.fromhex('C1 00 40'),bytes.fromhex('C0 06 40'),bytes.fromhex('C0 80 40')]:
                write_record(data/'u005A.bin',payload)
                with self.assertRaises(subprocess.CalledProcessError):
                    compile_and_run(data,'')

if __name__ == '__main__':
    unittest.main()
