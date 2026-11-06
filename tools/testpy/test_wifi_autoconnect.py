#!/usr/bin/env python3
"""Exercise the production boot-connect policy with a synthetic clock and radio.

Only the policy function is extracted; the private credential input is never
read or embedded. Driver, PCI identity and credentials are synthetic fixtures.
"""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
wifi = (ROOT / 'src/net/wifi.rs').read_text()
start = wifi.index('fn connect_boot_profile(')
end = wifi.index('\nstatic WIFI_ACTIVE:', start)
policy = wifi[start:end]
source = r'''
#![allow(dead_code)]
extern crate alloc;
extern crate self as trueos_time;
extern crate self as spin;
use std::sync::{Arc,MutexGuard};
pub struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    pub const fn new(value:T)->Self {Self(std::sync::Mutex::new(value))}
    pub fn lock(&self)->MutexGuard<'_,T>{self.0.lock().unwrap()}
}
static NOW:Mutex<u64>=Mutex::new(0);
pub struct Instant(u64);
impl Instant {
    pub fn now()->Self{Self(*NOW.lock())}
    pub fn as_millis(&self)->u64{self.0}
}
#[macro_export] macro_rules! log_warn {
    (target:$target:literal; $($args:tt)*)=>{{let _=format_args!($($args)*);}}
}
const SSID:&str="synthetic-network";
const PASSWORD:&str="synthetic-password";
mod boot {
    pub fn credentials()->Option<(&'static str,&'static str)>{*super::CREDENTIALS.lock()}
}
static CREDENTIALS:Mutex<Option<(&'static str,&'static str)>>=Mutex::new(None);
#[derive(Clone,Copy)] struct PciDevice{vendor_id:u16,device_id:u16}
#[derive(Clone,Copy,Debug,PartialEq,Eq)] enum DriverStatus{Unloaded,Loading,Running}
#[derive(Clone,Copy,Debug,PartialEq,Eq)] enum WifiState{NoHardware,Disabled,Disconnected,Scanning,Connecting,Authenticating,Connected,Failed}
#[path="DIAGNOSTICS"] mod diag;
#[derive(Clone)] struct WifiNetwork{ssid:String}
trait WifiDriver:Send {
    fn status(&self)->DriverStatus;
    fn wifi_state(&self)->WifiState;
    fn scan_results(&self)->Vec<WifiNetwork>;
    fn scan(&mut self)->Result<(),&'static str>;
    fn connect(&mut self,ssid:&str,password:&str)->Result<(),&'static str>;
    fn recover(&mut self)->Result<(),&'static str>;
}
type SharedDriver=Arc<Mutex<Box<dyn WifiDriver>>>;
static WIFI_DRIVER:Mutex<Option<SharedDriver>>=Mutex::new(None);
static WIFI_PCI:Mutex<Option<PciDevice>>=Mutex::new(None);
static CONNECTION_STATE:Mutex<WifiState>=Mutex::new(WifiState::NoHardware);
static LAST_OPERATION_ERROR:Mutex<Option<&'static str>>=Mutex::new(None);
fn shared_driver()->Option<SharedDriver>{WIFI_DRIVER.lock().clone()}
struct RadioState {
    status:DriverStatus,state:WifiState,networks:Vec<WifiNetwork>,
    scans:usize,connects:usize,recoveries:usize,recovery_error:Option<&'static str>,credentials_match:bool,
    scan_error:Option<&'static str>,connect_error:Option<&'static str>,connect_state:WifiState,
}
struct Radio(Arc<Mutex<RadioState>>);
impl WifiDriver for Radio {
    fn recover(&mut self)->Result<(),&'static str>{
        let mut state=self.0.lock();state.recoveries+=1;
        if let Some(reason)=state.recovery_error{return Err(reason);}
        state.status=DriverStatus::Loading;state.state=WifiState::Scanning;state.networks.clear();Ok(())
    }
    fn status(&self)->DriverStatus{self.0.lock().status}
    fn wifi_state(&self)->WifiState{self.0.lock().state}
    fn scan_results(&self)->Vec<WifiNetwork>{self.0.lock().networks.clone()}
    fn scan(&mut self)->Result<(),&'static str>{
        let mut state=self.0.lock();state.scans+=1;
        if let Some(reason)=state.scan_error{return Err(reason);}
        state.state=WifiState::Scanning;Ok(())
    }
    fn connect(&mut self,ssid:&str,password:&str)->Result<(),&'static str>{
        let mut state=self.0.lock();state.connects+=1;
        state.credentials_match=ssid==SSID&&password==PASSWORD;
        if let Some(reason)=state.connect_error{return Err(reason);}
        state.state=state.connect_state;Ok(())
    }
}
POLICY
#[cfg(test)] mod tests {
    use super::*;
    static SERIAL:std::sync::Mutex<()>=std::sync::Mutex::new(());
    fn fixture()->Arc<Mutex<RadioState>> {
        *NOW.lock()=0;*CREDENTIALS.lock()=Some((SSID,PASSWORD));
        *WIFI_PCI.lock()=Some(PciDevice{vendor_id:0x8086,device_id:0xa0f0});
        *CONNECTION_STATE.lock()=WifiState::Disconnected;*LAST_OPERATION_ERROR.lock()=Some("previous failure");
        let state=Arc::new(Mutex::new(RadioState{
            status:DriverStatus::Running,state:WifiState::Disconnected,
            networks:vec![WifiNetwork{ssid:SSID.into()}],scans:0,connects:0,recoveries:0,recovery_error:None,credentials_match:false,
            scan_error:None,connect_error:None,connect_state:WifiState::Connecting,
        }));
        *WIFI_DRIVER.lock()=Some(Arc::new(Mutex::new(Box::new(Radio(state.clone())))));
        state
    }
    #[test]fn disabled_profile_has_no_radio_side_effects(){
        let _serial=SERIAL.lock().unwrap();let state=fixture();*CREDENTIALS.lock()=None;
        let(mut attempted,mut deadline)=(false,0);
        connect_boot_profile(&mut attempted,&mut deadline);
        assert!(attempted);assert_eq!(state.lock().scans,0);assert_eq!(state.lock().connects,0);
        assert_eq!(deadline,0);assert_eq!(*CONNECTION_STATE.lock(),WifiState::Disconnected);
    }
    #[test]fn loading_scanning_or_existing_connection_does_not_connect_or_rescan(){
        let _serial=SERIAL.lock().unwrap();let state=fixture();
        let(mut attempted,mut deadline)=(false,0);
        state.lock().status=DriverStatus::Loading;
        connect_boot_profile(&mut attempted,&mut deadline);state.lock().status=DriverStatus::Running;
        for wifi_state in [WifiState::NoHardware,WifiState::Disabled,WifiState::Scanning,WifiState::Connecting,WifiState::Authenticating,WifiState::Connected]{
            state.lock().state=wifi_state;connect_boot_profile(&mut attempted,&mut deadline);
        }
        assert!(!attempted);assert_eq!(deadline,0);assert_eq!(state.lock().scans,0);assert_eq!(state.lock().connects,0);
    }
    #[test]fn missing_driver_or_wrong_pci_identity_never_consumes_connection(){
        let _serial=SERIAL.lock().unwrap();let state=fixture();let(mut attempted,mut deadline)=(false,0);
        for identity in [None,Some(PciDevice{vendor_id:0x8086,device_id:0x7a70}),Some(PciDevice{vendor_id:0x1234,device_id:0xa0f0})]{
            *WIFI_PCI.lock()=identity;connect_boot_profile(&mut attempted,&mut deadline);
        }
        *WIFI_PCI.lock()=Some(PciDevice{vendor_id:0x8086,device_id:0xa0f0});*WIFI_DRIVER.lock()=None;
        connect_boot_profile(&mut attempted,&mut deadline);
        assert!(!attempted);assert_eq!(deadline,0);assert_eq!(state.lock().scans,0);assert_eq!(state.lock().connects,0);
    }
    #[test]fn ready_target_connects_and_throttles_retries(){
        let _serial=SERIAL.lock().unwrap();let state=fixture();let(mut attempted,mut deadline)=(false,0);
        connect_boot_profile(&mut attempted,&mut deadline);
        assert!(attempted);assert_eq!(state.lock().connects,1);assert!(state.lock().credentials_match);
        assert_eq!(*CONNECTION_STATE.lock(),WifiState::Connecting);assert!(LAST_OPERATION_ERROR.lock().is_none());
        state.lock().state=WifiState::Disconnected;
        connect_boot_profile(&mut attempted,&mut deadline);
        assert_eq!(state.lock().connects,1);assert_eq!(state.lock().scans,0);
        *NOW.lock()=4999;connect_boot_profile(&mut attempted,&mut deadline);
        assert_eq!(state.lock().connects,1);
        *NOW.lock()=5000;connect_boot_profile(&mut attempted,&mut deadline);
        assert_eq!(state.lock().connects,1);assert_eq!(state.lock().recoveries,1);assert_eq!(deadline,10000);
        assert_eq!(*CONNECTION_STATE.lock(),WifiState::Scanning);
        state.lock().status=DriverStatus::Running;state.lock().state=WifiState::Disconnected;
        state.lock().networks.push(WifiNetwork{ssid:SSID.into()});
        *NOW.lock()=5001;connect_boot_profile(&mut attempted,&mut deadline);
        assert_eq!(state.lock().connects,2);assert!(state.lock().credentials_match);
    }
    #[test]fn connection_error_retries_after_five_seconds(){
        let _serial=SERIAL.lock().unwrap();let state=fixture();state.lock().connect_error=Some("station rejected");
        let(mut attempted,mut deadline)=(false,0);connect_boot_profile(&mut attempted,&mut deadline);
        assert!(attempted);assert_eq!(*LAST_OPERATION_ERROR.lock(),Some("station rejected"));
        state.lock().connect_error=None;connect_boot_profile(&mut attempted,&mut deadline);
        assert_eq!(state.lock().connects,1);assert_eq!(state.lock().scans,0);
        state.lock().state=WifiState::Failed;
        *NOW.lock()=5000;connect_boot_profile(&mut attempted,&mut deadline);
        assert_eq!(state.lock().connects,1);assert_eq!(state.lock().recoveries,1);
        assert!(!attempted);assert!(LAST_OPERATION_ERROR.lock().is_none());
    }
    #[test]fn missing_target_rescans_only_after_the_deadline_and_preserves_connect_attempt(){
        let _serial=SERIAL.lock().unwrap();let state=fixture();state.lock().networks=vec![WifiNetwork{ssid:"other-network".into()}];
        let(mut attempted,mut deadline)=(false,0);connect_boot_profile(&mut attempted,&mut deadline);
        assert!(!attempted);assert_eq!(state.lock().scans,1);assert_eq!(deadline,5000);
        assert_eq!(*CONNECTION_STATE.lock(),WifiState::Scanning);
        *NOW.lock()=1000;connect_boot_profile(&mut attempted,&mut deadline); // Still scanning.
        state.lock().state=WifiState::Disconnected;*NOW.lock()=4999;
        connect_boot_profile(&mut attempted,&mut deadline);assert_eq!(state.lock().scans,1);
        *NOW.lock()=5000;connect_boot_profile(&mut attempted,&mut deadline);
        assert!(!attempted);assert_eq!(state.lock().scans,2);assert_eq!(deadline,10000);assert_eq!(state.lock().connects,0);
    }
    #[test]fn target_found_by_retry_connects_without_waiting_for_next_scan_deadline(){
        let _serial=SERIAL.lock().unwrap();let state=fixture();state.lock().networks.clear();
        let(mut attempted,mut deadline)=(false,0);connect_boot_profile(&mut attempted,&mut deadline);
        state.lock().state=WifiState::Disconnected;state.lock().networks.push(WifiNetwork{ssid:SSID.into()});
        *NOW.lock()=1000;connect_boot_profile(&mut attempted,&mut deadline);
        assert!(attempted);assert_eq!(state.lock().scans,1);assert_eq!(state.lock().connects,1);
        assert!(state.lock().credentials_match);assert_eq!(*CONNECTION_STATE.lock(),WifiState::Connecting);
    }
    #[test]fn scan_failure_is_recorded_and_retried_at_low_cadence(){
        let _serial=SERIAL.lock().unwrap();let state=fixture();state.lock().networks.clear();state.lock().scan_error=Some("scan rejected");
        let(mut attempted,mut deadline)=(false,0);connect_boot_profile(&mut attempted,&mut deadline);
        assert!(attempted);assert_eq!(*LAST_OPERATION_ERROR.lock(),Some("scan rejected"));
        state.lock().scan_error=None;*NOW.lock()=10000;connect_boot_profile(&mut attempted,&mut deadline);
        assert_eq!(state.lock().scans,1);assert_eq!(state.lock().recoveries,1);assert_eq!(state.lock().connects,0);
        assert!(LAST_OPERATION_ERROR.lock().is_none());
    }
    #[test]fn failed_reset_is_throttled_and_does_not_submit_scan_or_connect(){
        let _serial=SERIAL.lock().unwrap();let state=fixture();
        state.lock().state=WifiState::Failed;state.lock().status=DriverStatus::Loading;
        state.lock().recovery_error=Some("master stop timeout");
        let(mut attempted,mut deadline)=(false,0);
        connect_boot_profile(&mut attempted,&mut deadline);
        assert_eq!(state.lock().recoveries,1);assert!(attempted);
        assert_eq!(*LAST_OPERATION_ERROR.lock(),Some("master stop timeout"));
        for time in [1,10,4999]{*NOW.lock()=time;connect_boot_profile(&mut attempted,&mut deadline);}
        assert_eq!(state.lock().recoveries,1);
        *NOW.lock()=5000;connect_boot_profile(&mut attempted,&mut deadline);
        assert_eq!(state.lock().recoveries,2);assert_eq!(state.lock().scans,0);assert_eq!(state.lock().connects,0);
    }
    #[test]fn bootstrap_failure_can_recover_before_driver_running(){
        let _serial=SERIAL.lock().unwrap();let state=fixture();
        state.lock().status=DriverStatus::Loading;state.lock().state=WifiState::Failed;
        let(mut attempted,mut deadline)=(false,0);connect_boot_profile(&mut attempted,&mut deadline);
        assert_eq!(state.lock().recoveries,1);assert!(!attempted);
        assert_eq!(*CONNECTION_STATE.lock(),WifiState::Scanning);
    }
    #[test]fn retries_never_interrupt_active_radio_operations(){
        let _serial=SERIAL.lock().unwrap();let state=fixture();let(mut attempted,mut deadline)=(false,0);
        connect_boot_profile(&mut attempted,&mut deadline);
        *NOW.lock()=10000;
        for wifi_state in [WifiState::Scanning,WifiState::Connecting,WifiState::Authenticating,WifiState::Connected]{
            state.lock().state=wifi_state;connect_boot_profile(&mut attempted,&mut deadline);
        }
        assert_eq!(state.lock().connects,1);assert_eq!(state.lock().scans,0);
    }
}
'''.replace('POLICY', policy).replace('DIAGNOSTICS', str(ROOT / 'src/net/wifi_diag.rs'))
with tempfile.TemporaryDirectory(prefix='trueos-wifi-autoconnect-') as temporary:
    directory = Path(temporary)
    (directory / 'test.rs').write_text(source)
    subprocess.run(['rustc', '--edition=2024', '--test', str(directory / 'test.rs'), '-o', str(directory / 'test')], check=True)
    subprocess.run([str(directory / 'test')], check=True)
