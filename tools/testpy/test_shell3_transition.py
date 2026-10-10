#!/usr/bin/env python3
"""Host checks of production strip transitions and snapshot damage."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    shell = (ROOT / 'src/shell3/shell3.rs').read_text()
    palette = shell[shell.index('#[repr(u32)]'):shell.index('#[derive(Clone, Copy, Debug, PartialEq, Eq)]\npub enum Shell3Error')]
    source = 'extern crate alloc;\nmod chronos {pub fn monotonic_nanos()->u64 {0}}\nmod shell3 {\n' + palette
    for module in ['metafmtstr', 'transition', 'update']:
        source += f'#[path = "{ROOT / "src/shell3" / (module + ".rs")}"] mod {module};\n'
    source += "use metafmtstr::MetaFmtStr;\n"
    source += "struct Shell3; struct RowStrips {left:Vec<MetaFmtStr>,right:Vec<MetaFmtStr>} fn matrix_slots(){}\n"
    source += shell[shell.index('fn matrix_slots_meta('):shell.index('fn matrix_slots_text(')]
    source += "const OPERATOR:char='§';\n"
    source += "mod status {\n" + (ROOT/'src/shell3/status.rs').read_text().split('impl Shell3 {')[0]
    source += r'''
    #[test] fn moving_aliases_do_not_capture_slot_clicks() {
        let ids=alloc::vec!["one".into(),"two".into()];
        let aliases=alloc::vec!["newapp".into()];
        assert_eq!(hit_with_width(&ids,&aliases,20,2,&[],Some(8)),Some(Target::Slot("one".into())));
        for column in 12..20 {assert_eq!(hit_with_width(&ids,&aliases,20,column,&[],Some(8)),None);}
        assert_eq!(hit(&ids,&aliases,20,14,&[]),Some(Target::Alias("newapp".into())));
    }
    }

'''
    source += r'''
#[cfg(test)] mod tests {
    use super::*;
    use alloc::vec;
    fn runs(text:&str)->Vec<MetaFmtStr> {vec![MetaFmtStr::new(text)]}
    fn frame(change:&transition::RetractReveal, ms:u64, clock:&str, columns:usize)->update::Snapshot {
        let left = runs(clock);
        update::Snapshot::new((columns,6),0,[(&left,&runs("UVWXYZ")),(&runs("§ slots"),&runs("Aka[test]")),(&runs("prompt"),&[])],columns)
            .with_reveal(0,&left,change.frame(ms*1_000_000))
            .with_matrix(&vec!["transcript".into()],1)
    }
    #[test] fn six_steps_and_normal_diffs() {
        let mut change=transition::RetractReveal::default();
        change.start(&runs("abcdef"),&runs("UVWXYZ"),0);
        let expected=["abcdef","  cdef","    ef","      ","    YZ","  WXYZ","UVWXYZ"];
        let mut previous=frame(&change,0,"clock",24);
        for (step,text) in expected.iter().enumerate() {
            let snapshot=frame(&change,step as u64*40,"clock",24);
            let lines=snapshot.rendered_lines();
            let rendered:String=lines[0][18..].iter().map(|c|c.0).collect();
            assert_eq!(&rendered,text);
            assert_eq!(lines[1],previous.rendered_lines()[1]);
            assert_eq!(lines[2],previous.rendered_lines()[2]);
            assert_eq!(lines[3],previous.rendered_lines()[3]);
            let batch=update::build_updates(&previous,&snapshot,&[]);
            assert!(batch.segments.iter().all(|segment|segment.row==SpecialRows::TitleRow));
            previous=snapshot;
        }
        assert!(change.frame(120_000_000).is_some());
        assert!(change.frame(240_000_000).is_none());
        assert!(change.frame(9_000_000_000).is_none());
        let clock_change=frame(&change,400,"newclock",24);
        assert!(update::build_updates(&previous,&clock_change,&[]).segments.iter().any(|segment|segment.row==SpecialRows::TitleRow));
    }
    #[test] fn retarget_clipping_unicode_and_cancellation() {
        let mut change=transition::RetractReveal::default();
        change.start(&runs("abcdef"),&runs("UVWXYZ"),0);
        let before=frame(&change,80,"clock",24).rendered_lines()[0].clone();
        change.start(&runs("UVWXYZ"),&runs("123456"),80_000_000);
        assert_eq!(frame(&change,80,"clock",24).rendered_lines()[0],before);
        for columns in [0,1,3,7,24] {
            for ms in [80,120,160,200,240,280] {
                let snapshot=frame(&change,ms,"long clock",columns);
                assert!(snapshot.rendered_lines()[0].len()<=columns);
                for reveal in snapshot.reveals() {
                    assert!(reveal.start+reveal.cells.len()<=columns);
                    assert!(reveal.hidden<=reveal.cells.len());
                }
            }
        }
        change.finish();
        assert!(change.frame(100_000_000).is_none());
        let styled=vec![MetaFmtStr::new("äβ😀def").underline()];
        change.start(&styled,&runs("UVWXYZ"),0);
        let snapshot=frame(&change,40,"clock",24);
        let lines=snapshot.rendered_lines();
        assert_eq!(lines[0][20].0,'😀');
        assert!(!lines[0][18].1.unwrap().underline());
        assert!(lines[0][20].1.unwrap().underline());
        // Source pixels stay fixed between steps, so the raster can be reused.
        assert_eq!(snapshot.reveals()[0].cells,frame(&change,80,"clock",24).reveals()[0].cells);
        change.start(&runs("x"),&runs("x"),400_000_000);
        assert!(change.frame(400_000_000).is_none());
    }
}
}
'''
    with tempfile.TemporaryDirectory(prefix='shell3-transition-') as directory:
        path = Path(directory)
        (path / 'test.rs').write_text(source)
        subprocess.run(['rustc', '--edition=2024', '--test', '-Awarnings', str(path/'test.rs'), '-o', str(path/'test')], check=True)
        subprocess.run([str(path/'test')], check=True)


if __name__ == '__main__':
    main()
