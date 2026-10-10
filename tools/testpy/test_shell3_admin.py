#!/usr/bin/env python3
"""Production admin menus, disk operations and cry persistence with host adapters."""
from pathlib import Path
import os
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]

def item(file, marker):
    return re.search(r'^'+re.escape(marker)+r'.*?^}', (ROOT/file).read_text(), re.M|re.S).group()

HARNESS = r'''
#![allow(dead_code)]
extern crate alloc;
extern crate self as trueos_executor;
extern crate self as trueos_time;
use alloc::{string::String,vec::Vec};
use std::{future::Future,pin::Pin,task::{Context,Poll,Waker},sync::atomic::{AtomicBool,AtomicU64,AtomicUsize,Ordering},cell::RefCell};
use spin::Mutex;
use zeroize::Zeroizing;
#[derive(Clone,Copy)]pub struct Spawner;impl Spawner {pub fn spawn<T>(&self,_:T){}}
pub struct Duration;impl Duration {pub fn from_millis(_:u64)->Self{Self}}
pub struct Timer(bool);impl Timer {pub fn after(_:Duration)->Self{Self(false)}}
impl Future for Timer {type Output=();fn poll(mut self:Pin<&mut Self>,_:&mut Context<'_>)->Poll<()> {if self.0 {Poll::Ready(())}else{self.0=true;Poll::Pending}}}
pub async fn with_timeout<T>(_:Duration,f:impl Future<Output=T>)->Result<T,()> {if TIMEOUT.load(Ordering::SeqCst){Err(())}else{Ok(f.await)}}
fn run<T>(f:impl Future<Output=T>)->T {let mut f=std::pin::pin!(f);loop {if let Poll::Ready(out)=f.as_mut().poll(&mut Context::from_waker(Waker::noop())){return out;}}}
static NOW:AtomicU64=AtomicU64::new(0);static VISIBLE:AtomicBool=AtomicBool::new(true);static LIVE:AtomicBool=AtomicBool::new(true);
static WORK:AtomicUsize=AtomicUsize::new(0);static SCOPE:AtomicUsize=AtomicUsize::new(2);static TIMEOUT:AtomicBool=AtomicBool::new(false);
static PERSIST_FAIL:AtomicBool=AtomicBool::new(false);static ABORTS:AtomicUsize=AtomicUsize::new(0);static LOGINS:AtomicUsize=AtomicUsize::new(0);
static TWO_FACTOR:AtomicBool=AtomicBool::new(true);static AUTH:AtomicBool=AtomicBool::new(true);
static WRITES:Mutex<Vec<Vec<u8>>>=Mutex::new(Vec::new());static IO:Mutex<Vec<&'static str>>=Mutex::new(Vec::new());
mod chronos {pub fn monotonic_nanos()->u64{crate::NOW.load(crate::Ordering::SeqCst)}}
mod workers {pub type WorkerSpawner=crate::Spawner;pub fn pick_background_spawner()->Option<WorkerSpawner>{Some(crate::Spawner)}}
mod wait {
    use super::*;thread_local!{static QUEUE:RefCell<Vec<Pin<Box<dyn Future<Output=()>>>>>=RefCell::new(Vec::new());}
    pub fn spawn_local_detached(f:impl Future<Output=()>+'static){QUEUE.with(|q|q.borrow_mut().push(Box::pin(f)));}
    pub fn drain(){loop {let f=QUEUE.with(|q|q.borrow_mut().pop());if let Some(f)=f{crate::run(f);}else{break;}}}
}
mod allcaps {pub mod storage {pub const USB_MASS_UAS_IO_TIMEOUT_MS:u64=1000;}}
mod disc {
    pub mod block {
        #[derive(Clone,Copy,Debug,PartialEq,Eq)]pub struct DeviceHandle {pub id:u32,pub generation:u32,pub writable:bool}
        pub struct DeviceInfo {pub id:DiscId,pub label:Option<String>,pub writable:bool,pub block_count:u64,pub block_size:u32}
        #[derive(Clone,Copy)]pub struct DiscId(pub u32);impl DiscId {pub fn raw(self)->u32{self.0}}
        pub fn device_handle(id:DiscId)->Option<DeviceHandle>{Some(DeviceHandle{id:id.0,generation:0,writable:true})}
        impl DeviceHandle {pub fn id(self)->DiscId{DiscId(self.id)}pub fn info(&self)->DeviceInfo {DeviceInfo{id:self.id(),label:Some("Test disk".into()),writable:self.writable,block_count:4096,block_size:512}}}
    }
    pub mod install {pub mod gpt {
        use super::super::block::DeviceHandle;
        pub struct GptPartitionSpec {pub type_guid:[u8;16],pub name:&'static str,pub size:PartitionSize,pub attributes:u64}
        pub enum PartitionSize {Remaining}
        pub async fn write_gpt_layout_with_log(_:DeviceHandle,_:&[GptPartitionSpec],_:&mut impl FnMut(&str))->Result<(),()> {crate::IO.lock().push("gpt");Ok(())}
    }}
}
static DISKS:Mutex<Vec<disc::block::DeviceHandle>>=Mutex::new(Vec::new());
mod r {
    pub mod disc {
        pub mod partition {
            pub const GPT_TYPE_LINUX_FILESYSTEM_BYTES:[u8;16]=[0;16];pub struct Registered {pub id:crate::disc::block::DiscId}
            pub async fn register_gpt_partitions(_:crate::disc::block::DeviceHandle)->Result<Vec<Registered>,()> {crate::IO.lock().push("partition");Ok(vec![Registered{id:crate::disc::block::DiscId(9)}])}
        }
        pub mod ramdisk {pub async fn create_trueos_public(_:u64,_:u32,_:String)->Result<crate::disc::block::DeviceHandle,()> {crate::IO.lock().push("ramdisk");Ok(crate::disc::block::DeviceHandle{id:7,generation:0,writable:true})}}
    }
    pub mod fs {pub mod trueosfs {
        use crate::disc::block::DeviceHandle;
        pub fn primary_root_handle()->Option<DeviceHandle>{Some(DeviceHandle{id:0,generation:0,writable:true})}
        pub async fn format_blank_partition_async(_:DeviceHandle)->Result<(),()> {crate::IO.lock().push("format");Ok(())}
        pub async fn remount_root_async(disk:DeviceHandle)->Result<Option<DeviceHandle>,()> {crate::IO.lock().push("remount");Ok(Some(disk))}
        pub async fn mount_root_async(_:DeviceHandle)->Result<(),()> {crate::IO.lock().push("mount");Ok(())}
        pub async fn file_out_async(_:DeviceHandle,_:&str)->Result<Option<Vec<u8>>,()> {Ok(Some(vec![1,2]))}
    }}
}
mod machine_key {pub async fn seal(_:crate::disc::block::DeviceHandle,_:&str,_:&[u8;32])->Result<(),String>{Ok(())}}
mod crypt {
    use super::*;
    #[derive(Debug)]pub enum CryError {AlreadyConfigured,InvalidUsername,InvalidTotpCode}
    #[derive(Clone,Copy,Debug,PartialEq,Eq)]pub enum CryTwoFactorState {Pending,Active}
    pub struct Session {pub scope_id:u8}pub struct Status {pub username:Option<String>,pub two_factor:CryTwoFactorState,pub session:Option<Session>,pub totp_clock:Option<()> ,pub persistence:&'static str}
    pub struct Enrollment {pub qr_payload:Zeroizing<String>}
    pub struct Proof {pub challenge_sequence:u64}pub struct Plan {pub envelope:Vec<u8>}pub struct Report {pub enrollment_activated:bool}
    pub fn canonical_username(name:&str)->Result<String,CryError>{if name.len()<3 {Err(CryError::InvalidUsername)}else{Ok(name.into())}}
    pub fn setup_root_key(_: &str)->Result<(),CryError>{Ok(())}
    pub fn begin_totp_enrollment()->Result<Enrollment,CryError>{Ok(Enrollment{qr_payload:Zeroizing::new("otpauth://totp/TRUEOS:alice?secret=JBSWY3DPEHPK3PXP&issuer=TRUEOS".into())})}
    pub fn status()->Status {Status{username:Some("alice".into()),two_factor:if TWO_FACTOR.load(Ordering::SeqCst){CryTwoFactorState::Pending}else{CryTwoFactorState::Active},session:Some(Session{scope_id:SCOPE.load(Ordering::SeqCst)as u8}),totp_clock:Some(()),persistence:"sealed"}}
    pub fn prepare_login(code:&str,scope:u8)->Result<Proof,CryError>{if code.len()!=6{return Err(CryError::InvalidTotpCode);}SCOPE.store(scope as usize,Ordering::SeqCst);LOGINS.fetch_add(1,Ordering::SeqCst);Ok(Proof{challenge_sequence:17})}
    pub fn prepare_ssh_key_change(code:&str,_:[u8;32],_:bool)->Result<Proof,CryError>{prepare_login(code,0)}
    pub fn prepare_persistence(_:u64)->Result<Plan,CryError>{Ok(Plan{envelope:vec![]})}
    pub fn complete_persisted_login(_:Plan)->Result<Report,CryError>{Ok(Report{enrollment_activated:true})}
    pub fn complete_persisted_remote_login(p:Plan)->Result<Report,CryError>{complete_persisted_login(p)}
    pub fn abort_pending_login(_:u64){ABORTS.fetch_add(1,Ordering::SeqCst);}
    pub fn unlock_persisted(_:&str,_:&[u8;32],_:&[u8])->Result<(),CryError>{Ok(())}
    pub fn authenticated_recovery_key(_:u8)->Option<Zeroizing<[u8;32]>>{AUTH.load(Ordering::SeqCst).then(||Zeroizing::new([1;32]))}
    pub fn has_authenticated_two_factor_session(_:u8)->bool{AUTH.load(Ordering::SeqCst)}
    pub fn logout(_:u8)->bool{AUTH.store(false,Ordering::SeqCst);true}
    pub fn ssh_authorized_keys()->Vec<[u8;32]>{vec![[1;32]]}
    pub fn ssh_key_fingerprint(_: &[u8;32])->String{"SHA256:example".into()}
    pub fn parse_ssh_public_key(key:&str)->Result<[u8;32],CryError>{if key.starts_with("ssh-ed25519 "){Ok([1;32])}else{Err(CryError::InvalidUsername)}}
}
mod shell2 {
    #[derive(Clone)]pub struct MatrixTarget;pub const OUTPUT_SYSTEM_MASK:u16=1;pub const TRANSPORT_LOCAL_SCOPE:u8=2;pub const TRANSPORT_NET_TCP_SCOPE:u8=1;
    pub fn matrix_target_for_slot_name(_:u16,_:&str)->MatrixTarget{MatrixTarget}
    pub fn claim_matrix_target_for_named_app_slot(_:&MatrixTarget,_:&str,_:&str)->Option<MatrixTarget>{Some(MatrixTarget)}
    pub fn set_matrix_target_active(_:&MatrixTarget,active:bool){if active {crate::WORK.fetch_add(1,crate::Ordering::SeqCst);}else{crate::WORK.fetch_sub(1,crate::Ordering::SeqCst);}}
    pub fn matrix_target_slot_lease(_:&MatrixTarget){}
    pub fn matrix_slot_is_live(_: &())->bool{crate::LIVE.load(crate::Ordering::SeqCst)}
    pub mod cmds {
        pub mod tlb_helper {
            use crate::disc::block::DeviceHandle;
            pub struct DiskChoice {pub handle:DeviceHandle}impl DiskChoice {pub fn raw_id(&self)->u32{self.handle.id}pub fn label_text(&self)->String{"Test disk".into()}pub fn size_text(&self)->String{"2MiB".into()}pub fn mode_text(&self)->&'static str{if self.handle.writable{"rw"}else{"ro"}}}
            pub fn collect_top_level_disk_choices()->Vec<DiskChoice>{crate::DISKS.lock().iter().map(|disk|DiskChoice{handle:*disk}).collect()}
            pub fn select_top_level_disk(id:u32)->Option<DeviceHandle>{crate::DISKS.lock().iter().find(|disk|disk.id==id).copied()}
        }
        pub mod disc {const RAMDISK_BLOCK_SIZE:u32=512;@PARSE_SIZE@ @CREATE_RAM@}
        pub mod format {use alloc::string::String;use crate::{Duration as EmbassyDuration,Timer,with_timeout};use crate::disc::block::{self,DeviceHandle};const FORMAT_OPERATION_TIMEOUT_MS:u64=6000;@FORMAT_DISK@}
        pub mod cry {
            use alloc::{string::String,vec::Vec};use core::fmt::Write;use crate::crypt;use zeroize::Zeroizing;use qrcodegen::{QrCode,QrCodeEcc,Version};
            const QR_QUIET_ZONE:i32=4;const QR_MAX_VERSION:Version=Version::new(10);const QR_BUFFER_BYTES:usize=QR_MAX_VERSION.buffer_len();
            pub fn error_text(error:crypt::CryError)->String{format!("{error:?}")}
            pub async fn write_persistence(_: &crypt::Plan)->Result<(),String>{if crate::PERSIST_FAIL.load(crate::Ordering::SeqCst){Err("Disk write failed".into())}else{Ok(())}}
            @HEX@ @NIBBLE@ @PARSE_RECOVERY@ @PENDING@ @PENDING_DROP@ @LOGIN@ @SSH_CHANGE@ @UNLOCK@ @QR@
        }
    }
}
mod shell3 {
    pub mod service {pub fn notify_work(){}}
    pub mod capture {pub fn error_result(error:&str)->String{format!("Error: {error}")}}
    pub mod tui {
        #[derive(Clone,Copy)]pub struct Frontend {pub id:u64,pub cols:usize,pub rows:usize}pub struct Surface{pub cols:u32,pub rows:u32}
        pub fn native_slot(_:&str)->bool{false}pub fn request(_:Frontend,_:&str)->Result<(),&'static str>{Ok(())}pub fn attach_native(_:Frontend,_:&crate::shell2::MatrixTarget)->Result<(),String>{Ok(())}pub fn cancel_native_attach(_:&crate::shell2::MatrixTarget){}
        pub fn native_return(_:&crate::shell2::MatrixTarget){crate::VISIBLE.store(false,crate::Ordering::SeqCst);}
        pub fn native_transport_scope(_:&crate::shell2::MatrixTarget)->Option<u8>{Some(crate::SCOPE.load(crate::Ordering::SeqCst)as u8)}
        pub fn native_visible(_:&crate::shell2::MatrixTarget)->bool{crate::VISIBLE.load(crate::Ordering::SeqCst)}
        pub fn native_write(_:&crate::shell2::MatrixTarget,b:&[u8]){crate::WRITES.lock().push(b.to_vec());}
        pub fn native_read(_:&crate::shell2::MatrixTarget)->Option<(Vec<u8>,Vec<String>)>{None}pub fn surface(_:&crate::shell2::MatrixTarget)->Option<Surface>{Some(Surface{cols:100,rows:30})}
    }
    #[path="@ROOT@/src/shell3/helper.rs"]mod helper;
    mod admin {
        @ADMIN@
        fn menu_task(_:Kind,_:crate::shell2::MatrixTarget)->Result<(),()>{Ok(())}
        #[cfg(test)]mod tests {
            use super::*;use crate::*;
            fn reset(){wait::drain();WORK.store(0,Ordering::SeqCst);WRITES.lock().clear();IO.lock().clear();VISIBLE.store(true,Ordering::SeqCst);LIVE.store(true,Ordering::SeqCst);TIMEOUT.store(false,Ordering::SeqCst);AUTH.store(true,Ordering::SeqCst);PERSIST_FAIL.store(false,Ordering::SeqCst);ABORTS.store(0,Ordering::SeqCst);LOGINS.store(0,Ordering::SeqCst);SCOPE.store(2,Ordering::SeqCst);TWO_FACTOR.store(true,Ordering::SeqCst);*DISKS.lock()=vec![DeviceHandle{id:1,generation:0,writable:true}];}
            fn surface_frame(view:&mut View,cols:usize,rows:usize)->trueos_terminal::Terminal{let mut terminal=trueos_terminal::Terminal::new(cols,rows);view.paint(&MatrixTarget,cols,rows,0);for b in WRITES.lock().drain(..){terminal.feed(&b);}terminal}
            fn frame(view:&mut View,cols:usize,rows:usize)->Vec<String>{surface_frame(view,cols,rows).render_rows()}
            #[test]fn formatting_requires_sure_and_revalidates_disk_identity(){reset();let mut view=View::new(Kind::Disc);view.selected=1;view.choose(&MatrixTarget);view.choose(&MatrixTarget);assert!(matches!(view.page,Page::Field(Field::FormatSure(_))));assert!(IO.lock().is_empty());view.input(&MatrixTarget,b"no\r",0,25);assert!(view.pending.is_none());assert!(IO.lock().is_empty());view.format_form(DISKS.lock()[0]);view.input(&MatrixTarget,b"sure\r",0,25);assert_eq!(WORK.load(Ordering::SeqCst),1);assert!(IO.lock().is_empty());DISKS.lock()[0].generation=1;wait::drain();view.complete();assert!(view.result.contains("replaced"));assert!(IO.lock().is_empty());assert_eq!(WORK.load(Ordering::SeqCst),0);}
            #[test]fn format_read_only_timeout_and_success_report_one_result(){reset();let mut view=View::new(Kind::Disc);view.format_form(DeviceHandle{id:2,generation:0,writable:false});assert!(view.result.contains("read-only"));assert!(IO.lock().is_empty());let disk=DISKS.lock()[0];TIMEOUT.store(true,Ordering::SeqCst);assert!(run(disk_format::format_disk(disk,|_|{})).unwrap_err().contains("timed out"));assert!(IO.lock().is_empty());TIMEOUT.store(false,Ordering::SeqCst);view.submit(&MatrixTarget,Request::Format(disk));view.leave(&MatrixTarget);assert_eq!(WORK.load(Ordering::SeqCst),1);wait::drain();view.complete();assert!(view.result.contains("Formatted disc1"));assert_eq!(*IO.lock(),vec!["gpt","partition","format","remount"]);assert_eq!(WORK.load(Ordering::SeqCst),0);}
            #[test]fn ramdisk_presets_custom_size_and_closed_slot_do_not_duplicate_work(){reset();let mut view=View::new(Kind::Disc);view.set_page(Page::Field(Field::RamSize));view.input(&MatrixTarget,b"512MiB\r",0,25);assert!(view.pending.is_some());view.choose(&MatrixTarget);assert_eq!(WORK.load(Ordering::SeqCst),1);wait::drain();view.complete();assert_eq!(*IO.lock(),vec!["ramdisk","mount"]);assert!(view.result.contains("disc7"));assert_eq!(crate::shell2::cmds::disc::parse_size_bytes("1GiB"),Some(1073741824));view.submit(&MatrixTarget,Request::Ramdisc(1));LIVE.store(false,Ordering::SeqCst);IO.lock().clear();wait::drain();view.complete();assert!(IO.lock().is_empty());assert!(view.result.contains("closed"));}
            #[test]fn codes_are_masked_and_crlf_does_not_submit_the_next_form(){reset();let mut view=View::new(Kind::Cry);view.set_page(Page::Field(Field::UnlockUsername));view.input(&MatrixTarget,b"alice\r",0,25);assert!(matches!(view.page,Page::Field(Field::UnlockKey(_))));view.input(&MatrixTarget,b"\n",0,25);assert!(view.form.value.is_empty());view.set_page(Page::Field(Field::LoginCode));view.input(&MatrixTarget,b"123456",0,25);let rows=frame(&mut view,100,25);assert!(rows[22].contains("••••••"));assert!(!rows.iter().any(|line|line.contains("123456")));view.input(&MatrixTarget,b"\x1b[<0;1;25M",0,25);assert!(!VISIBLE.load(Ordering::SeqCst));assert!(matches!(view.page,Page::Home));assert!(view.form.value.is_empty());}
            #[test]fn persistence_failure_releases_proof_and_preserves_transport_scope(){reset();SCOPE.store(1,Ordering::SeqCst);PERSIST_FAIL.store(true,Ordering::SeqCst);let mut view=View::new(Kind::Cry);view.set_page(Page::Field(Field::LoginCode));view.input(&MatrixTarget,b"123456\r",0,25);wait::drain();view.complete();assert_eq!(SCOPE.load(Ordering::SeqCst),1);assert_eq!(ABORTS.load(Ordering::SeqCst),1);assert!(view.result.contains("Disk write failed"));assert_eq!(WORK.load(Ordering::SeqCst),0);PERSIST_FAIL.store(false,Ordering::SeqCst);assert!(run(cry::login_for_scope(2,"123456")).is_ok());assert_eq!(SCOPE.load(Ordering::SeqCst),2);}
            #[test]fn qr_is_complete_colored_and_secret_pages_are_cleared_on_return(){reset();let qr=cry::enrollment_qr_lines("otpauth://totp/A?secret=JBSWY3DPEHPK3PXP").unwrap();let width=qr[0].chars().count();let count=qr.len();let mut view=View::new(Kind::Cry);view.set_page(Page::Enrollment(qr));let terminal=surface_frame(&mut view,100,50);let rows=terminal.render_rows();assert_eq!(rows[3].chars().take(width).collect::<String>().trim(),"");let style=terminal.cells()[300].style;assert_eq!(style.foreground,trueos_terminal::TerminalColor::Indexed(0));assert_eq!(style.background,trueos_terminal::TerminalColor::Indexed(7));assert!(view.qr_painted);view.leave(&MatrixTarget);assert!(matches!(view.page,Page::Home));assert!(view.form.value.is_empty());VISIBLE.store(true,Ordering::SeqCst);view.set_page(Page::Enrollment(cry::enrollment_qr_lines("otpauth://totp/A?secret=JBSWY3DPEHPK3PXP").unwrap()));let rows=frame(&mut view,20,10);assert!(!view.qr_painted);assert!(rows[3].starts_with("Resize"));assert!(count+6>10||width>20);}
            #[test]fn recovery_requires_authentication_and_secret_inputs_accept_literal_navigation_letters(){reset();let mut view=View::new(Kind::Cry);view.selected=5;view.choose(&MatrixTarget);assert!(matches!(view.page,Page::Recovery(_)));view.paint(&MatrixTarget,100,25,0);AUTH.store(false,Ordering::SeqCst);let rows=frame(&mut view,100,25);assert!(matches!(view.page,Page::Home));assert!(!rows.iter().any(|row|row.contains(&"01".repeat(32))));view.set_page(Page::Field(Field::SetupUsername));view.input(&MatrixTarget,b"hjqname",0,25);assert_eq!(view.form.value.as_str(),"hjqname");view.input(&MatrixTarget,b"\x1b",0,25);view.input(&MatrixTarget,b"",75_000_000,25);assert!(!VISIBLE.load(Ordering::SeqCst));}
            #[test]fn mouse_and_keyboard_share_scrolled_small_geometry_choices(){reset();for rows in [5,8,12,25] {let mut view=View::new(Kind::Disc);view.selected=2;let rendered=frame(&mut view,60,rows);assert!(rendered.iter().any(|row|row.contains("Create a RAM disk")));let row=view.menu_start(rows)+view.selected.min(rows.saturating_sub(view.menu_start(rows)+4));let click=format!("\x1b[<0;1;{}M",row+1);view.input(&MatrixTarget,click.as_bytes(),0,rows);assert!(matches!(view.page,Page::RamSizes));}}
        }
    }
}
'''

def main():
    admin=(ROOT/'src/shell3/admin.rs').read_text()
    admin=re.sub(r'^//!.*\n|^#\[trueos_executor::task[^\n]*\n','',admin,flags=re.M).replace('async fn menu_task(','async fn menu_task_run(')
    code=HARNESS.replace('@ROOT@',str(ROOT)).replace('@ADMIN@',admin)
    markers={
        'PARSE_SIZE':('src/shell2/cmds/disc.rs','pub(crate) fn parse_size_bytes('),
        'CREATE_RAM':('src/shell2/cmds/disc.rs','pub(crate) async fn create_ramdisc_bytes('),
        'FORMAT_DISK':('src/shell2/cmds/format.rs','pub(crate) async fn format_disk('),
        'HEX':('src/shell2/cmds/cry.rs','pub(crate) fn full_hex('),
        'NIBBLE':('src/shell2/cmds/cry.rs','fn hex_nibble('),
        'PARSE_RECOVERY':('src/shell2/cmds/cry.rs','pub(crate) fn parse_recovery_key('),
        'PENDING_DROP':('src/shell2/cmds/cry.rs','impl Drop for PendingProof'),
        'LOGIN':('src/shell2/cmds/cry.rs','pub(crate) async fn login_for_scope('),
        'SSH_CHANGE':('src/shell2/cmds/cry.rs','pub(crate) async fn change_ssh_key('),
        'UNLOCK':('src/shell2/cmds/cry.rs','pub(crate) async fn unlock_account('),
        'QR':('src/shell2/cmds/cry.rs','pub(crate) fn enrollment_qr_lines('),
    }
    code=code.replace('@PENDING@','struct PendingProof(u64);')
    for key,(file,marker) in markers.items():code=code.replace('@'+key+'@',item(file,marker))
    with tempfile.TemporaryDirectory(prefix='shell3-admin-') as tmp:
        tmp=Path(tmp);(tmp/'src').mkdir();(tmp/'src/lib.rs').write_text(code)
        (tmp/'Cargo.toml').write_text(f'''[package]
name="shell3-admin-host"
version="0.1.0"
edition="2024"
[dependencies]
spin="0.10"
zeroize={{version="1",features=["alloc"]}}
qrcodegen={{package="qrcodegen-no-heap",version="1.8.1",default-features=false}}
trueos-terminal={{path="{ROOT}/crates/trueos-terminal"}}
''')
        env=dict(os.environ,CARGO_TARGET_DIR=str(ROOT/'tgt/admin-host-tests'))
        subprocess.run(['cargo','test','--offline','--quiet','--manifest-path',str(tmp/'Cargo.toml'),'--','--test-threads=1'],env=env,cwd=tmp,check=True)

if __name__=='__main__':main()
