use core::str;

use heapless::String;
use v::vnet as api;

use crate::gate::{DeviceClass, DeviceIp, DeviceSnapshot};

pub const ESP_STATUS_PATH: &str = "/status";
pub const ESP_UPLOAD_PATH: &str = "/upload";
pub const ESP_RUN_PATH: &str = "/run";
pub const ESP_RESTART_PATH: &str = "/restart";
pub const ESP_ROOT_PATH: &str = "/";
pub const ESP_FILES_PATH: &str = "/files";
pub const DEVICE_STATUS_TEXT_CAP: usize = 32;
pub const DEVICE_STATUS_ERROR_CAP: usize = 96;
pub const DEVICE_STATUS_URL_CAP: usize = 96;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceInterface {
    pub handle: api::NetHandle,
    pub class: DeviceClass,
    pub ip: Option<DeviceIp>,
    pub service_port: u16,
}

impl DeviceInterface {
    pub fn from_snapshot(snapshot: &DeviceSnapshot) -> Self {
        Self {
            handle: snapshot.handle,
            class: snapshot.class,
            ip: snapshot.ip,
            service_port: snapshot.service_port,
        }
    }

    pub fn status_url(&self) -> Option<String<DEVICE_STATUS_URL_CAP>> {
        self.path_url(ESP_STATUS_PATH)
    }

    pub fn root_url(&self) -> Option<String<DEVICE_STATUS_URL_CAP>> {
        self.path_url(ESP_ROOT_PATH)
    }

    pub fn files_url(&self) -> Option<String<DEVICE_STATUS_URL_CAP>> {
        self.path_url(ESP_FILES_PATH)
    }

    pub fn upload_url(&self) -> Option<String<DEVICE_STATUS_URL_CAP>> {
        self.path_url(ESP_UPLOAD_PATH)
    }

    pub fn run_url(&self) -> Option<String<DEVICE_STATUS_URL_CAP>> {
        self.path_url(ESP_RUN_PATH)
    }

    pub fn restart_url(&self) -> Option<String<DEVICE_STATUS_URL_CAP>> {
        self.path_url(ESP_RESTART_PATH)
    }

    fn path_url(&self, path: &str) -> Option<String<DEVICE_STATUS_URL_CAP>> {
        if self.class != DeviceClass::EspUploader {
            return None;
        }

        match self.ip {
            Some(DeviceIp::V4(addr)) => {
                let mut out = String::new();
                let _ = core::fmt::write(
                    &mut out,
                    format_args!(
                        "http://{}.{}.{}.{}:{}/{}",
                        addr[0],
                        addr[1],
                        addr[2],
                        addr[3],
                        self.service_port,
                        &path[1..]
                    ),
                );
                Some(out)
            }
            Some(DeviceIp::V6(_)) | None => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceStatusSnapshot {
    pub threading_available: bool,
    pub app_exists: bool,
    pub running: bool,
    pub last_status: String<DEVICE_STATUS_TEXT_CAP>,
    pub last_error: String<DEVICE_STATUS_ERROR_CAP>,
    pub last_started_ms: Option<u64>,
    pub last_finished_ms: Option<u64>,
    pub last_heartbeat_ms: Option<u64>,
    pub heartbeat_count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StatusChangeEvent {
    pub handle: api::NetHandle,
    pub previous: Option<DeviceStatusSnapshot>,
    pub current: DeviceStatusSnapshot,
}

pub fn parse_status_snapshot(body: &[u8]) -> Option<DeviceStatusSnapshot> {
    let text = str::from_utf8(body).ok()?;

    let mut snapshot = DeviceStatusSnapshot {
        threading_available: false,
        app_exists: false,
        running: false,
        last_status: String::new(),
        last_error: String::new(),
        last_started_ms: None,
        last_finished_ms: None,
        last_heartbeat_ms: None,
        heartbeat_count: 0,
    };

    let mut saw_any = false;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let (key, value) = line.split_once('=')?;
        let key = key.trim();
        let value = value.trim();
        saw_any = true;

        match key {
            "threading_available" => snapshot.threading_available = parse_bool(value)?,
            "app_exists" => snapshot.app_exists = parse_bool(value)?,
            "running" => snapshot.running = parse_bool(value)?,
            "last_status" => assign_string(&mut snapshot.last_status, value),
            "last_error" => assign_string(&mut snapshot.last_error, value),
            "last_started_ms" => snapshot.last_started_ms = parse_optional_u64(value)?,
            "last_finished_ms" => snapshot.last_finished_ms = parse_optional_u64(value)?,
            "last_heartbeat_ms" => snapshot.last_heartbeat_ms = parse_optional_u64(value)?,
            "heartbeat_count" => snapshot.heartbeat_count = value.parse::<u64>().ok()?,
            _ => {}
        }
    }

    if !saw_any {
        return None;
    }

    Some(snapshot)
}

fn parse_bool(value: &str) -> Option<bool> {
    if value.eq_ignore_ascii_case("true") {
        Some(true)
    } else if value.eq_ignore_ascii_case("false") {
        Some(false)
    } else {
        None
    }
}

fn parse_optional_u64(value: &str) -> Option<Option<u64>> {
    if value.eq_ignore_ascii_case("none") {
        Some(None)
    } else {
        value.parse::<u64>().ok().map(Some)
    }
}

fn assign_string<const N: usize>(out: &mut String<N>, value: &str) {
    out.clear();
    for ch in value.chars() {
        if out.push(ch).is_err() {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_status_snapshot() {
        let snapshot = parse_status_snapshot(
            b"threading_available=True\napp_exists=True\nrunning=True\nlast_status=running\nlast_error=none\nlast_started_ms=174778\nlast_finished_ms=None\nlast_heartbeat_ms=23095\nheartbeat_count=1\n",
        )
        .expect("status snapshot");

        assert!(snapshot.threading_available);
        assert!(snapshot.app_exists);
        assert!(snapshot.running);
        assert_eq!(snapshot.last_status.as_str(), "running");
        assert_eq!(snapshot.last_error.as_str(), "none");
        assert_eq!(snapshot.last_started_ms, Some(174_778));
        assert_eq!(snapshot.last_finished_ms, None);
        assert_eq!(snapshot.last_heartbeat_ms, Some(23_095));
        assert_eq!(snapshot.heartbeat_count, 1);
    }

    #[test]
    fn builds_status_url_from_snapshot() {
        let snapshot = DeviceSnapshot {
            handle: api::NetHandle(1),
            class: DeviceClass::EspUploader,
            tag: String::new(),
            ip: Some(DeviceIp::V4([192, 168, 178, 102])),
            service_port: 8080,
            node_id: 0,
            caps: 0,
            connected_at_ms: 0,
            last_activity_ms: 0,
            status: None,
        };

        let iface = DeviceInterface::from_snapshot(&snapshot);
        let url = iface.status_url().expect("status url");
        assert_eq!(url.as_str(), "http://192.168.178.102:8080/status");
        let upload_url = iface.upload_url().expect("upload url");
        assert_eq!(upload_url.as_str(), "http://192.168.178.102:8080/upload");
    }
}
