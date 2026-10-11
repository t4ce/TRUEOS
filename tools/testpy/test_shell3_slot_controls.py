#!/usr/bin/env python3
"""Host checks for production slot runs, clipping, and title-slot styling."""
from pathlib import Path
import re
import subprocess
import tempfile
import test_clip_position3_uv_texture as extract

ROOT = Path(__file__).resolve().parents[2]
extract.ROOT = ROOT


def main():
    source = '''#![allow(dead_code)]
extern crate alloc;
use alloc::{string::{String,ToString},vec::Vec};
const OPERATOR:char='§';
mod chronos {pub fn monotonic_nanos()->u64 {0}}
struct RowStrips {left:Vec<MetaFmtStr>,right:Vec<MetaFmtStr>}
'''
    source += extract.item('src/shell3/shell3.rs', 'RgbaColor')
    shell = (ROOT/'src/shell3/shell3.rs').read_text()
    source += re.search(r'^impl RgbaColor \{.*?^}', shell, re.M | re.S).group()
    source += f'#[path="{ROOT}/src/shell3/metafmtstr.rs"] mod metafmtstr;\nuse metafmtstr::MetaFmtStr;\n'
    for name in ('matrix_slots_meta', 'matrix_slots_text'):
        source += extract.item('src/shell3/shell3.rs', name)
    source += 'mod update {use super::*;\n' + extract.item('src/shell3/update.rs', 'fit_strips') + '}\n'
    status = (ROOT/'src/shell3/status.rs').read_text()
    status = status[status.index('#[derive'):status.index('impl Shell3')]
    source += 'mod status {use super::*;\n' + status + '}\n'
    # Exercise the actual title composition block without kernel dependencies.
    block = shell[shell.index('            // The title operator'):shell.index('            if let Some(app)', shell.index('            // The title operator'))]
    pointer = re.search(r'^    pub\(super\) fn handle_controls_pointer.*?^    }', (ROOT/'src/shell3/status.rs').read_text(), re.M | re.S).group().replace('pub(super) fn', 'fn').replace('super::OPERATOR', 'OPERATOR').replace('Target::', 'status::Target::')
    source += '''struct Snapshot {lines:Vec<Vec<(char,)>>}
impl Snapshot {fn rendered_lines(&self)->&[Vec<(char,)>] {&self.lines}}
struct Shell3 {active:Option<String>,status_hover:Option<status::Target>}
impl Shell3 {
fn active_matrix_slot_name(&self)->Option<String> {self.active.clone()}
fn title(&self)->Vec<MetaFmtStr> {
let mut strips=RowStrips {left:vec![MetaFmtStr::new("TrueOS § 02:03")],right:vec![]};
''' + block + '''strips.left
}
fn capture_controls_snapshot(&self)->Snapshot {Snapshot {lines:vec![self.title().iter().flat_map(|run|run.text.chars().map(|ch|(ch,))).collect()]}}
fn select_matrix_slot_index(&mut self,index:usize) {assert_eq!(index,0);self.active=None;}
fn handle_status_pointer(&mut self,column:Option<usize>,pressed:bool)->bool {
let target=column.and_then(|column|status::hit(&["sh1".into()],&[],80,column,&[]));
let changed=self.status_hover!=target;self.status_hover=target.clone();
if pressed {if let Some(status::Target::Slot(name))=target {self.active=Some(name);return true;}}changed
}
''' + pointer + '\n}\n'
    source += r'''
#[test] fn title_operator_tracks_default_selection_and_hover() {
    let mut shell=Shell3 {active:None,status_hover:None};
    let title=shell.title();assert_eq!(title.iter().map(|run|run.text.as_str()).collect::<String>(),"TrueOS § 02:03");
    assert_eq!(title[7].color,Some(RgbaColor::Pink));assert!(title[7].bold);
    shell.active=Some("sh1".into());assert_eq!(shell.title()[7].color,Some(RgbaColor::White));
    shell.status_hover=Some(status::Target::Default);assert!(shell.title()[7].underline);
}
#[test] fn title_click_selects_default_and_strip_click_selects_named_slot() {
    let mut shell=Shell3 {active:Some("sh1".into()),status_hover:None};
    assert!(shell.handle_controls_pointer(Some((0,7)),true));assert_eq!(shell.active,None);
    assert_eq!(shell.title()[7].color,Some(RgbaColor::Pink));
    assert!(shell.handle_controls_pointer(Some((1,0)),true));assert_eq!(shell.active.as_deref(),Some("sh1"));
    shell.handle_controls_pointer(Some((0,6)),true);assert_eq!(shell.active.as_deref(),Some("sh1"));
    shell.handle_controls_pointer(None,false);assert_eq!(shell.status_hover,None);
}
#[test] fn strip_contains_only_named_slots_with_matching_click_targets() {
    let ids=vec!["sh1".into(),"log".into()];
    assert_eq!(matrix_slots_text(&ids),"§sh1 §log");assert_eq!(matrix_slots_text(&[]),"");
    let row=status::runs(&ids,Some("log"),&[],None,&[]);
    assert_eq!(row.left.iter().map(|run|run.text.as_str()).collect::<String>(),"§sh1 §log");
    assert_eq!(row.left[5].color,Some(RgbaColor::Pink));
    for column in 0..4 {assert_eq!(status::hit(&ids,&[],80,column,&[]),Some(status::Target::Slot("sh1".into())));}
    assert_eq!(status::hit(&ids,&[],80,4,&[]),None);
    assert_eq!(status::hit(&ids,&[],80,5,&[]),Some(status::Target::Slot("log".into())));
    assert_eq!(status::hit(&[],&[],80,0,&[]),None);
    assert_eq!(status::hit(&ids,&[],0,0,&[]),None);
}
#[test] fn working_markers_and_aliases_preserve_hit_geometry() {
    let ids=vec!["sh1".into()];let working=ids.clone();let aliases=vec!["hello".into()];
    assert_eq!(status::hit(&ids,&aliases,30,4,&working),None);
    assert_eq!(status::hit(&ids,&aliases,30,25,&working),Some(status::Target::Alias("hello".into())));
}
'''
    with tempfile.TemporaryDirectory(prefix='shell3-slot-controls-') as directory:
        path = Path(directory)
        (path/'test.rs').write_text(source)
        subprocess.run(['rustc','--edition=2024','--test',str(path/'test.rs'),'-o',str(path/'tests')],check=True)
        subprocess.run([str(path/'tests')],check=True)

if __name__ == '__main__':
    main()
