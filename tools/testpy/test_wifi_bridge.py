#!/usr/bin/env python3
"""Run the production Wi-Fi Ethernet bridge against a controllable radio."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
source = r'''
extern crate alloc;
extern crate self as spin;
use std::sync::MutexGuard;
pub struct Mutex<T>(std::sync::Mutex<T>);
impl<T> Mutex<T> {
    pub fn new(value: T) -> Self { Self(std::sync::Mutex::new(value)) }
    pub fn lock(&self) -> MutexGuard<'_, T> { self.0.lock().unwrap() }
}
mod pci { #[derive(Clone, Copy)] pub struct PciDevice; }
mod net {
    #[derive(Clone, Copy, PartialEq)] pub enum DriverStatus { Loading, Running }
    pub struct DriverInfo { pub name: &'static str }
    pub mod ring {
        pub fn alloc_packet_buf(n: usize) -> Vec<u8> { vec![0; n] }
        pub fn recycle_packet_buf(_: Vec<u8>) {}
    }
    #[path = "DEVICE"] pub mod device;
    pub mod wifi {
        use std::sync::Arc;
        use crate::{Mutex, net::{DriverInfo, DriverStatus}};
        #[derive(Clone, Copy, PartialEq)] pub enum WifiState { Disconnected, Connected }
        pub trait WifiDriver: Send {
            fn status(&self) -> DriverStatus;
            fn ethernet_data_path(&self) -> bool;
            fn mac_address(&self) -> Option<[u8; 6]>;
            fn info(&self) -> &DriverInfo;
            fn wifi_state(&self) -> WifiState;
            fn link_up(&self) -> bool;
            fn link_speed(&self) -> u32;
            fn receive(&mut self) -> Option<Vec<u8>>;
            fn send(&mut self, frame: &[u8]) -> Result<(), &'static str>;
        }
        pub type SharedDriver = Arc<Mutex<Box<dyn WifiDriver>>>;
    }
    #[path = "ADAPTER"] pub mod wifi_adapter;
}
use net::{device::NetDevice, wifi::{WifiDriver, WifiState}, wifi_adapter::WifiNic, DriverStatus};
use std::{sync::Arc, collections::VecDeque};
struct State {
    running: bool, ethernet: bool, mac: Option<[u8; 6]>, connected: bool,
    rx: VecDeque<Vec<u8>>, tx: Vec<Vec<u8>>,
}
struct Radio(Arc<Mutex<State>>);
impl WifiDriver for Radio {
    fn status(&self)->DriverStatus { if self.0.lock().running {DriverStatus::Running} else {DriverStatus::Loading} }
    fn ethernet_data_path(&self)->bool {self.0.lock().ethernet}
    fn mac_address(&self)->Option<[u8;6]> {self.0.lock().mac}
    fn info(&self)->&net::DriverInfo { static INFO:net::DriverInfo=net::DriverInfo{name:"test AX201"}; &INFO }
    fn wifi_state(&self)->WifiState { if self.0.lock().connected {WifiState::Connected} else {WifiState::Disconnected} }
    fn link_up(&self)->bool {self.0.lock().connected}
    fn link_speed(&self)->u32 {54}
    fn receive(&mut self)->Option<Vec<u8>> {self.0.lock().rx.pop_front()}
    fn send(&mut self, frame:&[u8])->Result<(), &'static str> {self.0.lock().tx.push(frame.to_vec());Ok(())}
}
fn radio()->(Arc<Mutex<State>>,net::wifi::SharedDriver) {
    let state=Arc::new(Mutex::new(State {running:true, ethernet:true, mac:Some([2,1,2,3,4,5]), connected:false,rx:VecDeque::new(),tx:Vec::new()}));
    let driver:net::wifi::SharedDriver=Arc::new(Mutex::new(Box::new(Radio(state.clone()))));
    (state, driver)
}
#[test] fn admit_only_firmware_mac_and_ethernet_path() {
    let(state,driver)=radio();
    for mac in [None,Some([0;6]),Some([0xff;6]),Some([1,2,3,4,5,6])] {
        state.lock().mac=mac;
        assert!(WifiNic::new(driver.clone(),pci::PciDevice).is_none());
    }
    state.lock().mac=Some([2,1,2,3,4,5]);
    state.lock().running=false;
    assert!(WifiNic::new(driver.clone(),pci::PciDevice).is_none());
    state.lock().running=true;
    state.lock().ethernet=false;
    assert!(WifiNic::new(driver.clone(),pci::PciDevice).is_none());
    state.lock().ethernet=true;
    let nic=WifiNic::new(driver,pci::PciDevice).unwrap();
    assert_eq!(nic.mac(),[2,1,2,3,4,5]);
    assert!(!nic.link_state().up); // A loaded radio is not an associated station.
}
#[test] fn ethernet_crosses_stack_boundary_unchanged_and_disconnect_closes_it() {
    let(state,driver)=radio();
    let mut nic=WifiNic::new(driver,pci::PciDevice).unwrap();
    let frame:Vec<u8>=(0..60).collect();
    assert!(nic.transmit(&frame).is_err());
    state.lock().connected=true;
    assert!(nic.link_state().up);
    nic.transmit(&frame).unwrap();
    assert_eq!(state.lock().tx,vec![frame.clone()]);
    state.lock().rx.push_back(vec![0;13]); // malformed
    state.lock().rx.push_back(frame.clone());
    assert!(nic.poll_rx());
    assert_eq!(nic.rx_queue_len(),1);
    assert_eq!(nic.pop_rx(),Some(frame.clone()));
    state.lock().rx.push_back(frame);
    nic.poll_rx();
    state.lock().connected=false;
    assert!(!nic.poll_rx());
    assert_eq!(nic.rx_queue_len(),0);
    assert!(!nic.transmit_ready());
}
#[test] fn rx_polling_is_bounded() {
    let(state,driver)=radio();state.lock().connected=true;
    let mut nic=WifiNic::new(driver,pci::PciDevice).unwrap();
    for _ in 0..1000 {state.lock().rx.push_back(vec![0;60]);}
    nic.poll_rx();assert_eq!(nic.rx_queue_len(),64);
    for _ in 0..20 {nic.poll_rx();}
    assert_eq!(nic.rx_queue_len(),256);
    assert!(state.lock().rx.is_empty());
}
'''
source = source.replace('DEVICE', str(ROOT / 'src/net/device.rs')).replace('ADAPTER', str(ROOT / 'src/net/wifi_adapter.rs'))
with tempfile.TemporaryDirectory(prefix='trueos-wifi-bridge-') as directory:
    directory = Path(directory)
    (directory / 'test.rs').write_text(source)
    subprocess.run(['rustc', '--edition=2024', '--test', str(directory / 'test.rs'), '-o', str(directory / 'test')], check=True)
    subprocess.run([str(directory / 'test')], check=True)
