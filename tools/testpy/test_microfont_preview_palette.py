#!/usr/bin/env python3
"""Check the real demo palette's row endpoints, padding and hue distribution."""
from pathlib import Path
import re,subprocess,tempfile
from _microfont_build import build_microfont
ROOT=Path(__file__).resolve().parents[2]
def main():
    source=(ROOT/'src/r/services/microfont_line_preview.rs').read_text()
    constants=source[source.index('const COLUMNS:'):source.index('// Font size means')]
    palette=source[source.index('fn preview_inks()'):]
    tests=r'''
#[test] fn rows_have_exact_alpha_endpoints_and_transparent_padding() {
    let inks=preview_inks();assert_eq!(inks.len(),GLYPHS.len());
    for (row,colors) in GLYPHS.chunks(COLUMNS).zip(inks.chunks(COLUMNS)) {
        let visible:Vec<_>=row.iter().zip(colors).filter(|(ch,_)|!ch.is_whitespace() && !ch.is_control()).collect();
        if visible.is_empty() {assert!(colors.iter().all(|c|*c==[0;4]));continue;}
        assert_eq!(visible.first().unwrap().1[3],255);
        assert_eq!(visible.last().unwrap().1[3],128);
        for pair in visible.windows(2) {assert!(pair[0].1[3]>pair[1].1[3]);}
        for (&ch,&color) in row.iter().zip(colors) {if ch.is_whitespace() || ch.is_control() {assert_eq!(color,[0;4]);}}
    }
}
#[test] fn each_named_glyph_has_distinct_saturated_rgb() {
    let inks=preview_inks();let mut seen=std::collections::HashSet::new();
    for (&ch,color) in GLYPHS.iter().zip(inks) {
        if ch.is_whitespace() || ch.is_control() {continue;}
        assert!(seen.insert([color[0],color[1],color[2]]));
        assert!(color[..3].contains(&0)&&color[..3].contains(&255));
    }
    assert_eq!(seen.len(),GLYPHS.iter().filter(|ch|!ch.is_whitespace() && !ch.is_control()).count());
    let expected:Vec<_>=microfont::CP850_CHARACTERS.iter().copied().collect();
    assert_eq!(GLYPHS.as_slice(),expected.as_slice());
    assert_eq!(GLYPHS.len(),256);
    assert_eq!(COLUMNS,16);
    assert_eq!(ROWS,16);
    assert_eq!(GLYPHS[0x20],' ');
    assert_eq!(GLYPHS[0x41],'A');
    assert_eq!(GLYPHS[0x82],'é'); // missing bins must retain their byte position
    assert_eq!(GLYPHS[0xff],'\u{a0}');
}
'''
    with tempfile.TemporaryDirectory(prefix='microfont-palette-') as tmp:
        tmp=Path(tmp);lib=tmp/'libmicrofont.rlib'
        build_microfont(lib)
        (tmp/'test.rs').write_text(constants+'\n'+palette+tests)
        subprocess.run(['rustc','--edition=2024','--test','--extern',f'microfont={lib}',str(tmp/'test.rs'),'-o',str(tmp/'tests')],check=True)
        subprocess.run([str(tmp/'tests')],check=True)
if __name__=='__main__':main()
