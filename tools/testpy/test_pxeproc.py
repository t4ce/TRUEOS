#!/usr/bin/env python3
"""Run the production OS menu, startup script, and disk boot mode on the host."""

from pathlib import Path
import re
import subprocess
import tempfile

from test_clip_position3_uv_texture import ROOT, constant, item


def main():
    blueprint = ROOT.parent / "TRUEOS-Blueprints/buildins/os/src/main.rs"
    ui = blueprint.read_text()
    # The complete state machine is platform independent; terminal rendering stays in the Blueprint build.
    menu = ui[ui.index("#[derive(Clone)]\nstruct Disk"):ui.index("fn main()")]
    key_handler = item(str(blueprint), "handle_key")
    keys = (blueprint.parent / "pxeproc.rs").read_text()
    keys = keys[keys.index("pub fn parse_keys"):keys.index("pub async fn launch_keys")]
    restart = (ROOT / "src/r/restart.rs").read_text()
    restart_impl = re.search(r"^impl RestartPolicy \{.*?^}", restart, re.M | re.S).group()
    source = f'''#![allow(dead_code,unreachable_patterns)]
extern crate alloc;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum KeyCode {{Up,Down,Enter,Left,Right,Esc,Char(char)}}
struct KeyEvent {{code:KeyCode}}
mod limine {{pub fn executable_cmdline()->Option<&'static str>{{None}}}}
mod live_update {{
    pub static WARM:std::sync::atomic::AtomicBool=std::sync::atomic::AtomicBool::new(false);
    pub fn warm_boot_active()->bool {{WARM.load(std::sync::atomic::Ordering::Relaxed)}}
}}
#[path="{ROOT}/src/disc/install/pxeproc.rs"] mod mode;
{constant("src/r/services/pxeproc_service.rs", "LAUNCH_SCRIPT")}
{item("src/r/restart.rs", "RestartPolicy")}
{restart_impl}
{menu}
{key_handler}
{keys}
fn app()->App {{App::new(vec![Disk {{id:42,name:"disk42".into(),size:"64G".into(),mode:"RW".into(),status:"blank".into(),label:"test".into()}}],vec![])}}
fn key(app:&mut App,code:KeyCode)->Option<String>{{handle_key(app,KeyEvent {{code}})}}
#[test] fn startup_script_reaches_lan_update_through_visible_confirmation() {{
    let mut app=app();
    let mut keys=parse_keys(LAUNCH_SCRIPT).unwrap().into_iter();
    assert_eq!(key(&mut app,keys.next().unwrap()),None);
    assert_eq!(key(&mut app,keys.next().unwrap()),None);
    assert_eq!(key(&mut app,keys.next().unwrap()),None);
    assert!(matches!(app.screen,Screen::Confirm(Action::LiveUpdateLan)));
    assert_eq!(app.selected,0); // Return is selected until the script explicitly chooses Proceed.
    assert_eq!(key(&mut app,keys.next().unwrap()),None);
    assert_eq!(key(&mut app,keys.next().unwrap()).as_deref(),Some("os:update:lan"));
    assert_eq!(keys.next(),None);
}}
#[test] fn remote_update_keeps_its_action_and_return_is_safe() {{
    let mut app=app();key(&mut app,KeyCode::Down);key(&mut app,KeyCode::Enter);
    assert_eq!(key(&mut app,KeyCode::Enter),None);
    assert!(matches!(app.screen,Screen::Home));
    key(&mut app,KeyCode::Down);key(&mut app,KeyCode::Enter);key(&mut app,KeyCode::Down);
    assert_eq!(key(&mut app,KeyCode::Enter).as_deref(),Some("os:update:live"));
}}
#[test] fn pxeproc_has_one_entry_and_retains_both_install_sources() {{
    for (source,expected) in [(0,"os:install:pxeproc-local:42"),(1,"os:install:pxeproc-online:42")] {{
        let mut app=app();key(&mut app,KeyCode::Enter);key(&mut app,KeyCode::Enter);
        assert_eq!(app.item_count(),3);
        key(&mut app,KeyCode::Down);key(&mut app,KeyCode::Down);key(&mut app,KeyCode::Enter);
        assert!(matches!(app.screen,Screen::PxeprocSource {{disk:0}}));
        if source==1 {{key(&mut app,KeyCode::Down);}}
        key(&mut app,KeyCode::Enter);
        assert_eq!(app.selected,0);
        assert!(!app.back());
        assert!(matches!(app.screen,Screen::PxeprocSource {{disk:0}}));
        if source==1 {{key(&mut app,KeyCode::Down);}}
        key(&mut app,KeyCode::Enter);key(&mut app,KeyCode::Down);
        assert_eq!(key(&mut app,KeyCode::Enter).as_deref(),Some(expected));
    }}
}}
#[test] fn regular_install_sources_keep_their_tokens() {{
    for (source,expected) in [(0,"os:install:local:42"),(1,"os:install:online:42")] {{
        let mut app=app();key(&mut app,KeyCode::Enter);key(&mut app,KeyCode::Enter);
        if source==1 {{key(&mut app,KeyCode::Down);}}
        key(&mut app,KeyCode::Enter);key(&mut app,KeyCode::Down);
        assert_eq!(key(&mut app,KeyCode::Enter).as_deref(),Some(expected));
    }}
}}
#[test] fn empty_disks_cannot_produce_an_install_action() {{
    let mut app=App::new(vec![],vec![]);key(&mut app,KeyCode::Enter);
    assert_eq!(key(&mut app,KeyCode::Enter),None);
    assert!(matches!(app.screen,Screen::Disks));
}}
#[test] fn boot_mode_roundtrips_from_esp_and_regular_reinstall_clears_it() {{
    let cmdline=|mode:mode::InstallMode| {{
        std::str::from_utf8(mode.limine_conf()).unwrap().lines().find_map(|line|line.strip_prefix("cmdline: ")).unwrap()
    }};
    assert!(mode::enabled_in_cmdline(cmdline(mode::InstallMode::Pxeproc)));
    assert!(!mode::enabled_in_cmdline(cmdline(mode::InstallMode::Regular)));
    for invalid in ["pxeproc=0","xpxeproc=1","pxeproc=11","foo=pxeproc=1"] {{assert!(!mode::enabled_in_cmdline(invalid));}}
    assert!(mode::enabled_in_cmdline("keyboard=de\\tpxeproc=1 timezone=Europe/Berlin"));
}}
#[test] fn live_kernel_handoff_uses_restore_policy_so_pxeproc_does_not_loop() {{
    use std::sync::atomic::Ordering;
    live_update::WARM.store(false,Ordering::Relaxed);
    assert_eq!(RestartPolicy::active(),RestartPolicy::ColdStart);
    live_update::WARM.store(true,Ordering::Relaxed);
    assert_eq!(RestartPolicy::active(),RestartPolicy::LiveUpdateRestore);
}}
#[test] fn malformed_launch_scripts_cannot_submit_an_action() {{
    assert!(parse_keys("key down\\ninstall 42").is_err());
    assert!(parse_keys("key proceed").is_err());
    assert_eq!(parse_keys("# comment\\n\\nkey enter").unwrap(),vec![KeyCode::Enter]);
}}
'''
    with tempfile.TemporaryDirectory(prefix="trueos-pxeproc-tests-") as temporary:
        directory = Path(temporary)
        (directory / "test.rs").write_text(source)
        subprocess.run(["rustc", "--edition=2024", "--test", str(directory / "test.rs"), "-o", str(directory / "tests")], check=True)
        subprocess.run([str(directory / "tests"), "--test-threads=1"], check=True)


if __name__ == "__main__":
    main()
