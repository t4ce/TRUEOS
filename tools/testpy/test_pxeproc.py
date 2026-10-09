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
    readiness = "src/r/readiness.rs"
    readiness_constants = "\n".join(constant(readiness, name) for name in (
        "TRUEOSFS_ROOT_MOUNTED", "BACKGROUND_AP_WORKER_READY", "VTHREAD_HW_TAG_READY",
        "RAYON_READY", "NET_ANY_CONFIGURED", "NET_SOCKET_READY", "TLS_SOCKET_SERVICE_READY",
        "INTEL_HDA_READY",
    ))
    spirit = "src/spirit/response_window.rs"
    source = f'''#![allow(dead_code,unreachable_patterns)]
extern crate alloc;
#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum KeyCode {{Up,Down,Enter,Left,Right,Esc,Char(char)}}
struct KeyEvent {{code:KeyCode}}
mod limine {{
    pub static CMDLINE:std::sync::Mutex<Option<&'static str>>=std::sync::Mutex::new(None);
    pub fn executable_cmdline()->Option<&'static str>{{*CMDLINE.lock().unwrap()}}
}}
mod live_update {{
    pub static WARM:std::sync::atomic::AtomicBool=std::sync::atomic::AtomicBool::new(false);
    pub fn warm_boot_active()->bool {{WARM.load(std::sync::atomic::Ordering::Relaxed)}}
}}
#[path="{ROOT}/src/disc/install/pxeproc.rs"] mod mode;
mod disc {{pub mod install {{pub(crate) use crate::mode as pxeproc;}}}}
mod shell2 {{pub mod cmds {{pub mod update {{
    {constant("src/shell2/cmds/update.rs", "LAN_ISO_URL")}
}}}}}}
mod r {{
    pub mod readiness {{
        {readiness_constants}
        pub static MASK:std::sync::atomic::AtomicU32=std::sync::atomic::AtomicU32::new(0);
        pub fn is_set(required:u32)->bool {{MASK.load(std::sync::atomic::Ordering::Relaxed)&required==required}}
    }}
    pub mod services {{pub mod pxeproc_service {{
        use alloc::string::String;
        {item("src/r/services/pxeproc_service.rs", "startup_text")}
    }}}}
}}
{constant("src/r/services/spawn_service.rs", "BP_AUTOSTART_READY")}
{item("src/r/services/spawn_service.rs", "bp_autostart_gate")}
{item("src/r/services/spawn_service.rs", "html_shack_gate")}
{item("src/hv/blueprint/prebind.rs", "prebind_import_readiness")}
{item("src/hv/blueprint/prebind.rs", "is_rayon_import")}
mod spirit_text {{
    use alloc::{{string::String,vec::Vec}};
    {constant(spirit, "SPIRIT_GRID_COLUMNS")}
    {constant(spirit, "SPIRIT_GRID_ROWS")}
    {constant(spirit, "STARTUP_WARMUP_TEXT")}
    {item(spirit, "sanitize_response")}
    {item(spirit, "sanitize_response_inner")}
    {item(spirit, "startup_warmup_text")}
    pub fn greeting(pxeproc:bool,ip:Option<[u8;4]>)->String {{startup_warmup_text(pxeproc,ip)}}
}}
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
#[test] fn bootloader_mode_is_captured_before_services_without_disk_access() {{
    use std::sync::atomic::Ordering;
    *limine::CMDLINE.lock().unwrap()=Some("keyboard=de pxeproc=1");
    live_update::WARM.store(false,Ordering::Relaxed);
    mode::init_boot_mode();
    *limine::CMDLINE.lock().unwrap()=None; // Services consume the captured boot policy.
    assert!(mode::boot_enabled());
    assert!(mode::cold_boot_enabled());
    live_update::WARM.store(true,Ordering::Relaxed);
    assert!(!mode::cold_boot_enabled());
    mode::init_boot_mode();
    assert!(!mode::boot_enabled());
}}
#[test] fn pxeproc_autostart_and_downloader_can_start_before_trueosfs() {{
    use std::sync::atomic::Ordering;
    use r::readiness::*;
    assert_eq!(BP_AUTOSTART_READY,BACKGROUND_AP_WORKER_READY|VTHREAD_HW_TAG_READY);
    MASK.store(BP_AUTOSTART_READY,Ordering::Relaxed);
    *limine::CMDLINE.lock().unwrap()=Some("pxeproc=1");
    mode::init_boot_mode();
    live_update::WARM.store(false,Ordering::Relaxed);
    assert!(bp_autostart_gate());assert!(html_shack_gate());
    live_update::WARM.store(true,Ordering::Relaxed);
    assert!(!bp_autostart_gate());assert!(!html_shack_gate());
    *limine::CMDLINE.lock().unwrap()=None;mode::init_boot_mode();
    live_update::WARM.store(false,Ordering::Relaxed);
    assert!(!bp_autostart_gate());assert!(!html_shack_gate());
    MASK.store(BP_AUTOSTART_READY|TRUEOSFS_ROOT_MOUNTED,Ordering::Relaxed);
    assert!(bp_autostart_gate());assert!(html_shack_gate());
}}
#[test] fn ram_vfile_imports_honor_the_blueprints_filesystem_independent_contract() {{
    use r::readiness::*;
    for import in ["trueos_cabi_async_fs_read_start","trueos_cabi_async_fs_status","trueos_cabi_async_fs_result_read"] {{
        assert_eq!(prebind_import_readiness(import,true),0);
        assert_eq!(prebind_import_readiness(import,false),TRUEOSFS_ROOT_MOUNTED);
    }}
    assert_eq!(prebind_import_readiness("trueos_cabi_archive_extract",true),TRUEOSFS_ROOT_MOUNTED|BACKGROUND_AP_WORKER_READY);
    assert_eq!(prebind_import_readiness("trueos_cabi_net_fetch_start",true),NET_ANY_CONFIGURED|NET_SOCKET_READY|TLS_SOCKET_SERVICE_READY);
}}
#[test] fn spirit_netboot_status_fits_her_grid_and_keeps_the_ip_and_target() {{
    for ip in [Some([192,168,178,94]),Some([255,255,255,255]),None] {{
        let greeting=spirit_text::greeting(true,ip);
        assert!(greeting.starts_with("Live Update -\\nContinue to TrueOS"));
        assert!(!greeting.contains("Hello"));
        assert!(!greeting.ends_with("..."));
        assert!(greeting.lines().count()<=13);
        assert!(greeting.lines().all(|line|line.chars().count()<=19));
        assert!(greeting.replace('\\n',"").contains(shell2::cmds::update::LAN_ISO_URL));
        if let Some([a,b,c,d])=ip {{assert!(greeting.contains(&format!("{{a}}.{{b}}.{{c}}.{{d}}")));}}
        else {{assert!(greeting.contains("unavailable"));}}
    }}
    assert_eq!(spirit_text::greeting(false,None),"Hello from TrueOS §");
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
