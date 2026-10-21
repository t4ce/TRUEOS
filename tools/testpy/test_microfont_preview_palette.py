#!/usr/bin/env python3
"""Check the real demo palette's row endpoints, padding and hue distribution."""
from pathlib import Path
import re,subprocess,tempfile
ROOT=Path(__file__).resolve().parents[2]
def main():
    source=(ROOT/'src/r/services/microfont_line_preview.rs').read_text()
    constants=source[source.index('const fn cp850_preview_character'):source.index('// Font size means')]
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
    let expected:Vec<_>=microfont::CP850_SYMBOLS.iter().map(|&(ch,_)|ch)
        .filter(|&ch|cp850_preview_character(ch)).collect();
    assert_eq!(&GLYPHS[..CP850_GLYPH_COUNT],expected.as_slice());
    assert_eq!(CP850_GLYPH_COUNT,132);
    assert!(GLYPHS[CP850_GLYPH_COUNT..CP850_PADDED].iter().all(|&ch|ch=='\0'));
    let digits:Vec<_>=microfont::seven_segment::SYMBOLS.iter().map(|&(ch,_)|ch).collect();
    assert_eq!(&GLYPHS[CP850_PADDED..],digits.as_slice());
    assert_eq!(ROWS,10);
    assert!(GLYPHS.contains(&'¿'));
    assert!(GLYPHS.contains(&'é')); // missing bins must retain a slot
    assert!(GLYPHS[..CP850_GLYPH_COUNT].iter().all(|&ch|ch as u32>127 && !(0x2500..=0x259f).contains(&(ch as u32))));
}
'''
    with tempfile.TemporaryDirectory(prefix='microfont-palette-') as tmp:
        tmp=Path(tmp);lib=tmp/'libmicrofont.rlib'
        subprocess.run(['rustc','--edition=2024','--crate-name','microfont','--crate-type','rlib',str(ROOT/'vendor/microfont/src/lib.rs'),'-o',str(lib)],check=True)
        (tmp/'test.rs').write_text(constants+'\n'+palette+tests)
        subprocess.run(['rustc','--edition=2024','--test','--extern',f'microfont={lib}',str(tmp/'test.rs'),'-o',str(tmp/'tests')],check=True)
        subprocess.run([str(tmp/'tests')],check=True)
if __name__=='__main__':main()
