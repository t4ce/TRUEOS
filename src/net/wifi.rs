use super::{DriverStatus, NetworkDriver};
use crate::pci::PciDevice;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;

#[path = "wifi_boot.rs"]
mod boot;
#[path = "wifi_diag.rs"]
mod diag;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WifiSecurity {
    Open,
    WEP,
    WPA,
    WPA2,
    WPA3,
    Unknown,
}

impl WifiSecurity {
    pub fn as_str(&self) -> &'static str {
        match self {
            WifiSecurity::Open => "Open",
            WifiSecurity::WEP => "WEP",
            WifiSecurity::WPA => "WPA",
            WifiSecurity::WPA2 => "WPA2",
            WifiSecurity::WPA3 => "WPA3",
            WifiSecurity::Unknown => "???",
        }
    }
}

#[derive(Debug, Clone)]
pub struct WifiNetwork {
    pub ssid: String,
    pub bssid: [u8; 6],
    pub channel: u8,
    pub signal_dbm: i8,
    pub security: WifiSecurity,
    pub frequency_mhz: u16,
}

impl WifiNetwork {
    pub fn signal_quality(&self) -> u8 {
        // Convert dBm to percentage: -30 dBm = 100%, -90 dBm = 0%
        if self.signal_dbm >= -30 {
            return 100;
        }
        if self.signal_dbm <= -90 {
            return 0;
        }
        ((self.signal_dbm as i16 + 90) * 100 / 60) as u8
    }

    pub fn signal_bars(&self) -> u8 {
        match self.signal_quality() {
            80..=100 => 4,
            60..=79 => 3,
            40..=59 => 2,
            20..=39 => 1,
            _ => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WifiState {
    NoHardware,
    Disabled,
    Disconnected,
    Scanning,
    Connecting,
    Authenticating,
    Connected,
    Failed,
}

pub trait WifiDriver: NetworkDriver + Send {
    /// Reboot firmware on the existing device/NIC after a proven DMA stop.
    /// Unsupported drivers keep their current ownership and return an error.
    fn recover(&mut self) -> Result<(), &'static str> {
        Err("Wi-Fi driver recovery unavailable")
    }
    fn last_error(&self) -> Option<&'static str> { None }
    /// Real NVM/firmware station address, available only after hardware reads it.
    fn mac_address(&self) -> Option<[u8; 6]> {
        None
    }
    /// Opt in only when send/receive carry Ethernet II frames. Firmware commands,
    /// management notifications and raw 802.11 MPDUs must stay driver-side.
    fn ethernet_data_path(&self) -> bool {
        false
    }
    fn wifi_state(&self) -> WifiState;
    fn scan(&mut self) -> Result<(), &'static str>;
    fn scan_results(&self) -> Vec<WifiNetwork>;
    fn connect(&mut self, ssid: &str, password: &str) -> Result<(), &'static str>;
    fn disconnect(&mut self) -> Result<(), &'static str>;
    fn connected_ssid(&self) -> Option<String>;
    fn signal_strength(&self) -> Option<i8>;
}

pub(crate) type SharedDriver = Arc<Mutex<Box<dyn WifiDriver>>>;
static WIFI_DRIVER: Mutex<Option<SharedDriver>> = Mutex::new(None);
static WIFI_PCI: Mutex<Option<PciDevice>> = Mutex::new(None);

pub(crate) fn shared_driver() -> Option<SharedDriver> {
    WIFI_DRIVER.lock().clone()
}

#[derive(Default)]
struct NetworkRegistration {
    index: Option<usize>,
    poll_started: bool,
    service_started: bool,
}
static NETWORK_REGISTRATION: Mutex<NetworkRegistration> = Mutex::new(NetworkRegistration {
    index: None,
    poll_started: false,
    service_started: false,
});

/// Called by the radio service after polling firmware. A valid hardware MAC
/// and explicit Ethernet framing support admit a NIC; association controls its
/// link separately. A late firmware start uses the existing per-NIC IP tasks.
#[cfg(not(test))]
pub(crate) fn register_network_interface(
    spawner: trueos_executor::Spawner,
) -> Result<Option<usize>, &'static str> {
    let mut registration = NETWORK_REGISTRATION.lock();
    let index = if let Some(index) = registration.index {
        index
    } else {
        let Some(driver) = shared_driver() else {
            return Ok(None);
        };
        let Some(pci) = *WIFI_PCI.lock() else {
            return Ok(None);
        };
        let Some(nic) = super::wifi_adapter::WifiNic::new(driver, pci) else {
            return Ok(None);
        };
        let index = super::register_wifi_nic(nic)?;
        registration.index = Some(index);
        crate::log_info!(target: "net";
            "net/wifi: stage=ethernet-registered net={} pci={:04x}:{:04x} bdf={:02x}:{:02x}.{} link=association-controlled\n",
            index, pci.vendor_id, pci.device_id, pci.bus, pci.slot, pci.function);
        index
    };
    if !registration.poll_started {
        let task =
            super::adapter::net_poll_task(index).map_err(|_| "WiFi net-poll task capacity")?;
        spawner.spawn(task);
        registration.poll_started = true;
    }
    if !registration.service_started {
        let task = super::adapter::net_service_task(index)
            .map_err(|_| "WiFi net-service task capacity")?;
        spawner.spawn(task);
        registration.service_started = true;
    }
    drop(registration);
    super::prefer_link_up_device(index);
    Ok(Some(index))
}

pub(crate) fn owns_network_interface(index: usize) -> bool {
    NETWORK_REGISTRATION.lock().index == Some(index)
}

#[cfg(not(test))]
#[trueos_executor::task(pool_size = 1)]
pub(crate) async fn wifi_service_task(spawner: trueos_executor::Spawner) {
    let mut last_registration_error = None;
    let mut boot_connect_attempted = false;
    let mut next_boot_scan_ms = 0;
    if boot::credentials().is_some() {
        crate::log_warn!(target: "net"; "wifi-boot: state=waiting-for-scan credentials=embedded\n");
    }
    let mut ticks = 0u64;
    let mut last_heartbeat_ms = trueos_time::Instant::now().as_millis();
    loop {
        ticks = ticks.saturating_add(1);
        poll();
        connect_boot_profile(&mut boot_connect_attempted, &mut next_boot_scan_ms);
        let wifi_state = state();
        let color = match wifi_state {
            WifiState::Connected => 0xff00_ff00,
            WifiState::Scanning | WifiState::Connecting | WifiState::Authenticating => 0xff00_00ff,
            WifiState::NoHardware | WifiState::Disabled | WifiState::Failed => 0xffff_0000,
            WifiState::Disconnected if last_operation_error().is_some() => 0xffff_0000,
            WifiState::Disconnected => 0xff00_00ff,
        };
        crate::r::services::microfont_log_service::wifi_color_changed(color);
        match register_network_interface(spawner) {
            Ok(_) => last_registration_error = None,
            Err(error) => {
                if last_registration_error != Some(error) {
                    crate::log_warn!(target: "net";
                        "net/wifi: stage=ethernet-admission-deferred reason={}\n", error);
                    last_registration_error = Some(error);
                }
            }
        }
        let wifi_index = NETWORK_REGISTRATION.lock().index;
        let address = wifi_index.and_then(super::adapter::ipv4_at);
        crate::r::services::microfont_log_service::wifi_ipv4_changed(address);
        let now_ms = trueos_time::Instant::now().as_millis();
        if state() != WifiState::Connected
            && now_ms.saturating_sub(last_heartbeat_ms) >= 5_000
        {
            crate::log_warn!(target: "net";
                "wifi-service: alive tick={} uptime_ms={} state={:?} port=closed\n",
                ticks, now_ms, state());
            last_heartbeat_ms = now_ms;
        }
        trueos_time::Timer::after(trueos_time::Duration::from_millis(10)).await;
    }
}
/// The local build profile is optional and requires no runtime filesystem or
/// keyboard. Retry idle scans/connections at five-second intervals. The driver
/// performs a checked device reset before retrying a failed/previous station;
/// never interrupt a scan, association, or authentication already in progress.
fn connect_boot_profile(attempted: &mut bool, next_scan_ms: &mut u64) {
    let now_ms = trueos_time::Instant::now().as_millis();
    if *attempted && now_ms < *next_scan_ms {
        return;
    }
    let Some((ssid, password)) = boot::credentials() else {
        *attempted = true;
        return;
    };
    let Some(pci) = *WIFI_PCI.lock() else {
        return;
    };
    if (pci.vendor_id, pci.device_id) != (0x8086, 0xa0f0) {
        return;
    }
    let Some(shared) = shared_driver() else {
        return;
    };
    let mut guard = shared.lock();
    let driver = guard.as_mut();
    let state = driver.wifi_state();
    if !matches!(state, WifiState::Disconnected | WifiState::Failed) {
        return;
    }
    if state == WifiState::Failed || *attempted {
        if now_ms < *next_scan_ms {
            return;
        }
        *next_scan_ms = now_ms.saturating_add(5_000);
        #[cfg(not(test))]
        crate::r::services::microfont_log_service::wifi_retry_started();
        diag::action_started(diag::Action::Recover, now_ms);
        let result = driver.recover();
        diag::action_finished(diag::Action::Recover, result, trueos_time::Instant::now().as_millis());
        match result {
            Ok(()) => {
                *attempted = false;
                *CONNECTION_STATE.lock() = driver.wifi_state();
                *LAST_OPERATION_ERROR.lock() = None;
                crate::log_warn!(target: "net"; "wifi-boot: state=recovering reset=confirmed scan=pending\n");
            }
            Err(reason) => {
                *attempted = true;
                *LAST_OPERATION_ERROR.lock() = Some(reason);
                crate::log_warn!(target: "net"; "wifi-boot: state=recovery-failed reason={}\n", reason);
            }
        }
        return;
    }
    if driver.status() != DriverStatus::Running {
        return;
    }
    if !driver.scan_results().iter().any(|network| network.ssid == ssid) {
        if now_ms >= *next_scan_ms {
            *next_scan_ms = now_ms.saturating_add(5_000);
            #[cfg(not(test))]
            crate::r::services::microfont_log_service::wifi_retry_started();
            diag::action_started(diag::Action::Scan, now_ms);
            let result = driver.scan();
            diag::action_finished(diag::Action::Scan, result, trueos_time::Instant::now().as_millis());
            match result {
                Ok(()) => {
                    *CONNECTION_STATE.lock() = driver.wifi_state();
                    *LAST_OPERATION_ERROR.lock() = None;
                    crate::log_warn!(target: "net"; "wifi-boot: state=waiting-for-network scan=retry\n");
                }
                Err(reason) => {
                    *attempted = true;
                    *LAST_OPERATION_ERROR.lock() = Some(reason);
                    crate::log_warn!(target: "net"; "wifi-boot: state=scan-failed reason={}\n", reason);
                }
            }
        }
        return;
    }
    *attempted = true;
    *next_scan_ms = now_ms.saturating_add(5_000);
    #[cfg(not(test))]
    crate::r::services::microfont_log_service::wifi_retry_started();
    crate::log_warn!(target: "net"; "wifi-boot: state=connecting\n");
    diag::action_started(diag::Action::Connect, now_ms);
    let result = driver.connect(ssid, password);
    diag::action_finished(diag::Action::Connect, result, trueos_time::Instant::now().as_millis());
    match result {
        Ok(()) => {
            *CONNECTION_STATE.lock() = driver.wifi_state();
            *LAST_OPERATION_ERROR.lock() = None;
        }
        Err(reason) => {
            *LAST_OPERATION_ERROR.lock() = Some(reason);
            crate::log_warn!(target: "net"; "wifi-boot: state=failed reason={}\n", reason);
        }
    }
}

static WIFI_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Deferred WiFi PCI location (bus, device, function) — set during boot, probed on first use
static DEFERRED_PCI: Mutex<Option<(u8, u8, u8)>> = Mutex::new(None);

/// Last scan results (cached for UI)
static SCAN_RESULTS: Mutex<Vec<WifiNetwork>> = Mutex::new(Vec::new());

/// Current connection state
static CONNECTION_STATE: Mutex<WifiState> = Mutex::new(WifiState::NoHardware);

/// Currently connected SSID
static CONNECTED_SSID: Mutex<Option<String>> = Mutex::new(None);

/// Pending connection request
static CONNECT_REQUEST: Mutex<Option<(String, String)>> = Mutex::new(None);
static LAST_OPERATION_ERROR: Mutex<Option<&'static str>> = Mutex::new(None);

pub fn last_operation_error() -> Option<&'static str> {
    shared_driver().and_then(|driver| driver.lock().last_error()).or(*LAST_OPERATION_ERROR.lock())
}

/// Retained software evidence remains readable while the radio owns a long
/// reset/bootstrap operation. No driver lock, MMIO, log sink or disk is used.
pub(crate) fn diagnostic_snapshot_text() -> String {
    diag::snapshot_text(trueos_time::Instant::now().as_millis())
}

pub(crate) fn record_diagnostic_stage(stage: &'static str) {
    diag::stage(stage, trueos_time::Instant::now().as_millis());
}

pub(crate) fn record_diagnostic_failure(stage: &'static str, reason: &'static str) {
    diag::failure(stage, reason, trueos_time::Instant::now().as_millis());
}

pub(crate) fn record_diagnostic_ap_disconnect(subtype: u8, reason: u16) {
    diag::ap_disconnect(subtype, reason, trueos_time::Instant::now().as_millis());
}

pub(crate) fn record_diagnostic_registers(
    stage: &'static str,
    csr_int: u32,
    csr_reset: u32,
    csr_gp: u32,
    hw_if: u32,
    rx_read: u16,
    rx_write: u16,
    tx_write: u16,
) {
    diag::registers(diag::Registers {
        at_ms: trueos_time::Instant::now().as_millis(), stage,
        csr_int, csr_reset, csr_gp, hw_if, rx_read, rx_write, tx_write,
    });
}

/// Publish a failure and its readback with one timestamp and one lock. A clock
/// tick between two separate records must not detach the original evidence.
pub(crate) fn record_diagnostic_failure_registers(
    stage: &'static str,
    reason: &'static str,
    csr_int: u32,
    csr_reset: u32,
    csr_gp: u32,
    hw_if: u32,
    rx_read: u16,
    rx_write: u16,
    tx_write: u16,
) {
    diag::failure_registers(diag::Registers {
        at_ms: trueos_time::Instant::now().as_millis(), stage,
        csr_int, csr_reset, csr_gp, hw_if, rx_read, rx_write, tx_write,
    }, reason);
}

pub fn set_deferred_pci(bus: u8, device: u8, function: u8) {
    *DEFERRED_PCI.lock() = Some((bus, device, function));
    crate::log!(
        "wifi: Deferred PCI probe stored: {}.{}.{}",
        bus,
        device,
        function
    );
}

pub fn has_wifi() -> bool {
    WIFI_ACTIVE.load(Ordering::Relaxed) || DEFERRED_PCI.lock().is_some()
}

pub fn has_active_driver() -> bool {
    WIFI_ACTIVE.load(Ordering::Relaxed)
}

pub fn lazy_probe() -> Result<(), &'static str> {
    // Already probed?
    if WIFI_ACTIVE.load(Ordering::Relaxed) {
        return Ok(());
    }

    let pci_loc = DEFERRED_PCI.lock().take();
    let (bus, dev, func) = pci_loc.ok_or("No WiFi hardware detected during boot")?;

    crate::log!("wifi: lazy probe pci={}.{}.{}\n", bus, dev, func);
    crate::log!("wifi: Lazy probe: {}.{}.{}", bus, dev, func);

    // Find the PCI device by bus/device/function
    let pci_dev = crate::pci::with_devices(|devices| {
        devices
            .iter()
            .copied()
            .find(|d| d.bus == bus && d.slot == dev && d.function == func)
    })
    .ok_or("WiFi PCI device not found")?;

    // Now actually probe (map_bar0 + bus master)
    if probe_pci(&pci_dev) {
        crate::log!("wifi: PCI probe ready\n");
        Ok(())
    } else {
        // Put the deferred info back so user can retry
        *DEFERRED_PCI.lock() = Some((bus, dev, func));
        Err("WiFi hardware probe failed")
    }
}

pub fn state() -> WifiState {
    shared_driver().map_or(WifiState::NoHardware, |driver| driver.lock().wifi_state())
}

pub fn is_connected() -> bool {
    state() == WifiState::Connected
}

pub fn connected_ssid() -> Option<String> {
    shared_driver().and_then(|driver| driver.lock().connected_ssid())
}

pub fn signal_strength() -> Option<i8> {
    shared_driver().and_then(|driver| driver.lock().signal_strength())
}

pub fn ensure_started() -> Result<(), &'static str> {
    // Lazy probe if not yet done (deferred from boot)
    if !WIFI_ACTIVE.load(Ordering::Relaxed) {
        lazy_probe()?;
    }

    let shared = shared_driver().ok_or("No WiFi driver")?;
    let mut guard = shared.lock();
    let driver = guard.as_mut();
    if driver.status() == DriverStatus::Running {
        return Ok(());
    }
    crate::log!("wifi: Auto-starting driver (hw_init + firmware)...");
    crate::log!("wifi: starting hardware\n");
    match driver.start() {
        Ok(()) => {
            crate::log!("wifi: Driver started successfully");
            crate::log!("wifi: hardware initialized\n");
            Ok(())
        }
        Err(e) => {
            *LAST_OPERATION_ERROR.lock() = Some(e);
            crate::log!("wifi: Driver start failed: {}", e);
            crate::log!("wifi: start failed: {}", e);
            Err(e)
        }
    }
}

pub fn start_scan() -> Result<(), &'static str> {
    // Lazy probe + auto-start
    ensure_started()?;

    {
        let shared = shared_driver().ok_or("No WiFi driver")?;
        let mut guard = shared.lock();
        let driver = guard.as_mut();
        *CONNECTION_STATE.lock() = WifiState::Scanning;
        let result = driver.scan();
        *LAST_OPERATION_ERROR.lock() = result.as_ref().err().copied();
        result
    }
}

pub fn get_scan_results() -> Vec<WifiNetwork> {
    SCAN_RESULTS.lock().clone()
}

pub fn poll() {
    if let Some(shared) = shared_driver() {
        let mut guard = shared.lock();
        let driver = guard.as_mut();
        driver.poll();

        // Update cached state
        let new_state = driver.wifi_state();
        diag::observe(new_state, driver.status(), driver.last_error(), trueos_time::Instant::now().as_millis());
        let old_state = *CONNECTION_STATE.lock();

        if new_state != old_state {
            *CONNECTION_STATE.lock() = new_state;
            crate::log!("wifi: State: {:?} -> {:?}", old_state, new_state);
        }

        // Update scan results when scan completes
        if old_state == WifiState::Scanning && new_state != WifiState::Scanning {
            let results = driver.scan_results();
            crate::log!("wifi: Scan complete: {} networks found", results.len());
            *SCAN_RESULTS.lock() = results;
        }

        // Update connected SSID
        *CONNECTED_SSID.lock() = driver.connected_ssid();

        // Process pending connect request
        let request = CONNECT_REQUEST.lock().take();
        if let Some((ssid, password)) = request {
            crate::log!("wifi: Connecting to '{}'...", ssid);
            match driver.connect(&ssid, &password) {
                Ok(()) => {
                    *CONNECTION_STATE.lock() = driver.wifi_state();
                    *LAST_OPERATION_ERROR.lock() = None;
                }
                Err(e) => {
                    *LAST_OPERATION_ERROR.lock() = Some(e);
                    crate::log!("wifi: Connect failed: {}", e);
                    *CONNECTION_STATE.lock() = WifiState::Failed;
                }
            }
        }
    }
}

pub fn request_connect(ssid: &str, password: &str) {
    if let Err(error) = try_request_connect(ssid, password) {
        crate::log_warn!(target: "net"; "wifi: connect request failed reason={}\n", error);
    }
}

/// Queue credentials in memory for the radio task; never persist or log keys.
pub fn try_request_connect(ssid: &str, password: &str) -> Result<(), &'static str> {
    if ssid.is_empty() || ssid.len() > 32 {
        return Err("Wi-Fi SSID must contain 1-32 bytes");
    }
    let result = ensure_started();
    *LAST_OPERATION_ERROR.lock() = result.as_ref().err().copied();
    result?;
    *CONNECT_REQUEST.lock() = Some((String::from(ssid), String::from(password)));
    Ok(())
}

pub fn disconnect() -> Result<(), &'static str> {
    let shared = shared_driver().ok_or("No WiFi driver")?;
    let mut guard = shared.lock();
    let driver = guard.as_mut();
    driver.disconnect()?;
    *CONNECTION_STATE.lock() = WifiState::Disconnected;
    *CONNECTED_SSID.lock() = None;
    Ok(())
}

pub fn set_driver(driver: Box<dyn WifiDriver>) {
    crate::log!("wifi: WiFi driver active: {}", driver.info().name);
    *WIFI_DRIVER.lock() = Some(Arc::new(Mutex::new(driver)));
    WIFI_ACTIVE.store(true, Ordering::SeqCst);
    *CONNECTION_STATE.lock() = WifiState::Disconnected;
}

pub fn probe_pci(pci_dev: &PciDevice) -> bool {
    // AX201 has a separate QuZ transport; never feed it AX211/4965 firmware.
    if pci_dev.vendor_id == 0x8086 && pci_dev.device_id == 0xA0F0 {
        if let Some(driver) = super::iwl_ax201::probe(pci_dev) {
            *WIFI_PCI.lock() = Some(*pci_dev);
            set_driver(driver);
            return true;
        }
        return false;
    }

    // AX211 is an integrated Gen2 device. Keep it out of the legacy 4965
    // probe and let the dedicated path perform the single PCI claim.
    if pci_dev.vendor_id == 0x8086 && pci_dev.device_id == 0x7A70 {
        if let Some(driver) = super::iwl_ax211::probe(pci_dev) {
            *WIFI_PCI.lock() = Some(*pci_dev);
            set_driver(driver);
            return true;
        }
        return false;
    }

    // Debug: log every device we check
    crate::log!(
        "[WIFI-PROBE] Checking {:04X}:{:04X} class={:02X} sub={:02X} at {}.{}.{}",
        pci_dev.vendor_id,
        pci_dev.device_id,
        pci_dev.class,
        pci_dev.subclass,
        pci_dev.bus,
        pci_dev.slot,
        pci_dev.function
    );

    // Intel WiFi devices: class 0x02 (Network) subclass 0x80 (Other)
    // or class 0x0D (Wireless)
    // or Intel vendor with known WiFi device IDs
    let is_wireless = pci_dev.class == 0x0D
        || (pci_dev.class == crate::pci::class::NETWORK && pci_dev.subclass == 0x80)
        || (pci_dev.vendor_id == 0x8086
            && super::iwl4965::IWL4965_DEVICE_IDS.contains(&pci_dev.device_id));

    if !is_wireless {
        crate::log!(
            "[WIFI-PROBE] -> Not wireless (class={:02X} sub={:02X} devid={:04X})",
            pci_dev.class,
            pci_dev.subclass,
            pci_dev.device_id
        );
        return false;
    }

    crate::log!(
        "wifi: Found wireless device: {:04X}:{:04X} at {}.{}.{}",
        pci_dev.vendor_id,
        pci_dev.device_id,
        pci_dev.bus,
        pci_dev.slot,
        pci_dev.function
    );

    const PCI_CLAIM_OWNER: &str = "net/wifi";
    if let Err(error) = crate::pci::claim_device(pci_dev, PCI_CLAIM_OWNER) {
        crate::log!(
            "wifi: PCI claim failed for {:04X}:{:04X} at {}.{}.{}: {:?}",
            pci_dev.vendor_id,
            pci_dev.device_id,
            pci_dev.bus,
            pci_dev.slot,
            pci_dev.function,
            error
        );
        return false;
    }

    // Try Intel WiFi Link 4965AGN
    if pci_dev.vendor_id == 0x8086 {
        if let Some(driver) = super::iwl4965::probe(pci_dev) {
            *WIFI_PCI.lock() = Some(*pci_dev);
            set_driver(driver);
            return true;
        }
    }

    let _ = crate::pci::release_device_claim(
        pci_dev.bus,
        pci_dev.slot,
        pci_dev.function,
        PCI_CLAIM_OWNER,
    );
    false
}
