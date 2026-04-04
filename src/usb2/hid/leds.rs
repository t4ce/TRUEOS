extern crate alloc;

use alloc::vec;
use crab_usb::{Device, DeviceInfo, USBHost, usb_if};
use embassy_executor::Spawner;
use embassy_time::{Duration as EmbassyDuration, Timer};

const LED_VID_JGINYUE: u16 = 0x0416;
const LED_PID_JGINYUE: u16 = 0xA125;

const LED_TEST_RED: u8 = 0xFF;
const LED_TEST_GREEN: u8 = 0x37;
const LED_TEST_BLUE: u8 = 0xFF;
const LED_COMMIT_REPORT: [u8; 7] = [0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
const LED_STEP_PAUSE_MS: u64 = 1200;
const LED_FEATURE_REPORT_ID: u8 = 4;
const LED_FEATURE_REPORT_LEN: usize = 301;
const LED_FEATURE_BACKING_LEN: usize = 512;
const LED_ENABLE_EARLY_FEATURE_GET_REPORT: bool = false;

#[derive(Copy, Clone, Debug)]
struct LedProbeTarget {
	configuration_value: u8,
	interface_number: u8,
}

#[derive(Copy, Clone, Debug)]
struct LedExperimentStep {
	label: &'static str,
	id5: [u8; 7],
	id6: [u8; 7],
	pause_ms: u64,
}

#[inline]
fn is_supported_led_controller(vid: u16, pid: u16) -> bool {
	vid == LED_VID_JGINYUE && pid == LED_PID_JGINYUE
}

fn pick_led_probe_target(configs: &[usb_if::descriptor::ConfigurationDescriptor]) -> Option<LedProbeTarget> {
	for config in configs.iter() {
		for interface in config.interfaces.iter() {
			for alt in interface.alt_settings.iter() {
				if alt.class != 0x03 || alt.alternate_setting != 0 {
					continue;
				}

				let has_out = alt.endpoints.iter().any(|ep| {
					ep.direction == usb_if::transfer::Direction::Out
						&& matches!(
							ep.transfer_type,
							usb_if::descriptor::EndpointType::Interrupt
								| usb_if::descriptor::EndpointType::Bulk
						)
				});
				if !has_out {
					continue;
				}

				return Some(LedProbeTarget {
					configuration_value: config.configuration_value,
					interface_number: alt.interface_number,
				});
			}
		}
	}

	None
}

#[inline]
pub(crate) fn should_share_probe_device(dev_info: &DeviceInfo) -> bool {
	let desc = dev_info.descriptor();
	is_supported_led_controller(desc.vendor_id, desc.product_id)
		&& pick_led_probe_target(dev_info.configurations()).is_some()
}

async fn send_hid_set_report(
	device: &mut Device,
	interface_number: u8,
	report_id: u8,
	payload: &[u8],
) -> Result<(), crab_usb::err::TransferError> {
	device
		.control_out(
			usb_if::host::ControlSetup {
				request_type: usb_if::transfer::RequestType::Class,
				recipient: usb_if::transfer::Recipient::Interface,
				request: usb_if::transfer::Request::Other(0x09),
				value: ((2u16) << 8) | u16::from(report_id),
				index: u16::from(interface_number),
			},
			payload,
		)
		.await
		.map(|_| ())
}

async fn read_hid_get_report_feature(
	device: &mut Device,
	interface_number: u8,
	report_id: u8,
	buff: &mut [u8],
) -> Result<usize, crab_usb::err::TransferError> {
	device
		.control_in(
			usb_if::host::ControlSetup {
				request_type: usb_if::transfer::RequestType::Class,
				recipient: usb_if::transfer::Recipient::Interface,
				request: usb_if::transfer::Request::Other(0x01),
				value: ((3u16) << 8) | u16::from(report_id),
				index: u16::from(interface_number),
			},
			buff,
		)
		.await
}

async fn read_led_feature_report_early(device: &mut Device, target: LedProbeTarget) {
	let desc = device.descriptor();
	let vendor_id = desc.vendor_id;
	let product_id = desc.product_id;

	if !LED_ENABLE_EARLY_FEATURE_GET_REPORT {
		crate::log!(
			"crabusb: leds {:04X}:{:04X} early feature get-report disabled for safety; request would be if#{} report_id={} len={} bmRequestType=0xA1 bRequest=0x01 wValue=0x{:04X} wIndex={} wLength={} backing_len={}\n",
			vendor_id,
			product_id,
			target.interface_number,
			LED_FEATURE_REPORT_ID,
			LED_FEATURE_REPORT_LEN,
			((3u16) << 8) | u16::from(LED_FEATURE_REPORT_ID),
			target.interface_number,
			LED_FEATURE_REPORT_LEN,
			LED_FEATURE_BACKING_LEN
		);
		return;
	}

	let mut buff = vec![0u8; LED_FEATURE_BACKING_LEN];
	let buff_addr = buff.as_ptr();
	let buff_len = buff.len();
	let buff_capacity = buff.capacity();
	let request = &mut buff[..LED_FEATURE_REPORT_LEN];

	crate::log!(
		"crabusb: leds {:04X}:{:04X} early feature get-report if#{} report_id={} len={} bmRequestType=0xA1 bRequest=0x01 wValue=0x{:04X} wIndex={} wLength={}\n",
		vendor_id,
		product_id,
		target.interface_number,
		LED_FEATURE_REPORT_ID,
		LED_FEATURE_REPORT_LEN,
		((3u16) << 8) | u16::from(LED_FEATURE_REPORT_ID),
		target.interface_number,
		LED_FEATURE_REPORT_LEN
	);
	crate::log!(
		"crabusb: leds {:04X}:{:04X} early feature buffer addr={:p} request_len={} backing_len={} backing_capacity={}B\n",
		vendor_id,
		product_id,
		buff_addr,
		request.len(),
		buff_len,
		buff_capacity
	);

	match read_hid_get_report_feature(
		device,
		target.interface_number,
		LED_FEATURE_REPORT_ID,
		request,
	)
	.await
	{
		Ok(read_len) => {
			let read_len = read_len.min(LED_FEATURE_REPORT_LEN);
			let nonzero = buff[..read_len].iter().filter(|byte| **byte != 0).count();
			crate::log!(
				"crabusb: leds {:04X}:{:04X} early feature report id={} len={} nonzero={} bytes={:02X?}\n",
				vendor_id,
				product_id,
				LED_FEATURE_REPORT_ID,
				read_len,
				nonzero,
				&buff[..read_len]
			);

			match send_hid_set_report(
				device,
				target.interface_number,
				LED_FEATURE_REPORT_ID,
				&buff[..read_len],
			)
			.await
			{
				Ok(()) => {
					crate::log!(
						"crabusb: leds {:04X}:{:04X} early feature roundtrip set-report id={} len={} submitted\n",
						vendor_id,
						product_id,
						LED_FEATURE_REPORT_ID,
						read_len
					);
				}
				Err(err) => {
					crate::log!(
						"crabusb: leds {:04X}:{:04X} early feature roundtrip set-report id={} len={} failed: {:?}\n",
						vendor_id,
						product_id,
						LED_FEATURE_REPORT_ID,
						read_len,
						err
					);
				}
			}
		}
		Err(err) => {
			crate::log!(
				"crabusb: leds {:04X}:{:04X} early feature report id={} read failed: {:?}\n",
				vendor_id,
				product_id,
				LED_FEATURE_REPORT_ID,
				err
			);
		}
	}
}

async fn log_led_live_state(device: &mut Device, target: LedProbeTarget) -> bool {
	let desc = device.descriptor();
	let vendor_id = desc.vendor_id;
	let product_id = desc.product_id;

	crate::log!(
		"crabusb: leds {:04X}:{:04X} live-state if#{} target_cfg={} feature request will execute early before matrix: bmRequestType=0xA1 bRequest=0x01 wValue=0x{:04X} wIndex={} wLength={}\n",
		vendor_id,
		product_id,
		target.interface_number,
		target.configuration_value,
		((3u16) << 8) | u16::from(LED_FEATURE_REPORT_ID),
		target.interface_number,
		LED_FEATURE_REPORT_LEN
	);

	match device.current_configuration_descriptor().await {
		Ok(config) => {
			crate::log!(
				"crabusb: leds {:04X}:{:04X} live current cfg={} target_cfg={} if#{}\n",
				vendor_id,
				product_id,
				config.configuration_value,
				target.configuration_value,
				target.interface_number
			);
			if config.configuration_value != target.configuration_value {
				crate::log!(
					"crabusb: leds {:04X}:{:04X} cfg mismatch on live device; aborting probe rather than reconfigure hardware\n",
					vendor_id,
					product_id
				);
				return false;
			}
			true
		}
		Err(err) => {
			crate::log!(
				"crabusb: leds {:04X}:{:04X} live current cfg read failed; aborting probe rather than reconfigure hardware: {:?}\n",
				vendor_id,
				product_id,
				err
			);
			false
		}
	}
}

#[inline]
const fn led_payload(zone: u8, red: u8, green: u8, blue: u8, field_64: u8) -> [u8; 7] {
	[zone, red, green, blue, field_64, 0x01, 0x00]
}

const LED_EXPERIMENT_STEPS: [LedExperimentStep; 10] = [
	LedExperimentStep {
		label: "baseline-zero-both",
		id5: led_payload(0xFF, 0x00, 0x00, 0x00, 0x64),
		id6: led_payload(0xFF, 0x00, 0x00, 0x00, 0x64),
		pause_ms: LED_STEP_PAUSE_MS,
	},
	LedExperimentStep {
		label: "id5-red-id6-zero",
		id5: led_payload(0xFF, 0xFF, 0x00, 0x00, 0x64),
		id6: led_payload(0xFF, 0x00, 0x00, 0x00, 0x64),
		pause_ms: LED_STEP_PAUSE_MS,
	},
	LedExperimentStep {
		label: "id5-green-id6-zero",
		id5: led_payload(0xFF, 0x00, 0xFF, 0x00, 0x64),
		id6: led_payload(0xFF, 0x00, 0x00, 0x00, 0x64),
		pause_ms: LED_STEP_PAUSE_MS,
	},
	LedExperimentStep {
		label: "id5-blue-id6-zero",
		id5: led_payload(0xFF, 0x00, 0x00, 0xFF, 0x64),
		id6: led_payload(0xFF, 0x00, 0x00, 0x00, 0x64),
		pause_ms: LED_STEP_PAUSE_MS,
	},
	LedExperimentStep {
		label: "id5-red-field00-id6-zero",
		id5: led_payload(0xFF, 0xFF, 0x00, 0x00, 0x00),
		id6: led_payload(0xFF, 0x00, 0x00, 0x00, 0x64),
		pause_ms: LED_STEP_PAUSE_MS,
	},
	LedExperimentStep {
		label: "id6-red-id5-zero",
		id5: led_payload(0xFF, 0x00, 0x00, 0x00, 0x64),
		id6: led_payload(0xFF, 0xFF, 0x00, 0x00, 0x64),
		pause_ms: LED_STEP_PAUSE_MS,
	},
	LedExperimentStep {
		label: "id6-green-id5-zero",
		id5: led_payload(0xFF, 0x00, 0x00, 0x00, 0x64),
		id6: led_payload(0xFF, 0x00, 0xFF, 0x00, 0x64),
		pause_ms: LED_STEP_PAUSE_MS,
	},
	LedExperimentStep {
		label: "id6-blue-id5-zero",
		id5: led_payload(0xFF, 0x00, 0x00, 0x00, 0x64),
		id6: led_payload(0xFF, 0x00, 0x00, 0xFF, 0x64),
		pause_ms: LED_STEP_PAUSE_MS,
	},
	LedExperimentStep {
		label: "id6-red-field00-id5-zero",
		id5: led_payload(0xFF, 0x00, 0x00, 0x00, 0x64),
		id6: led_payload(0xFF, 0xFF, 0x00, 0x00, 0x00),
		pause_ms: LED_STEP_PAUSE_MS,
	},
	LedExperimentStep {
		label: "magenta-both",
		id5: led_payload(0xFF, LED_TEST_RED, LED_TEST_GREEN, LED_TEST_BLUE, 0x64),
		id6: led_payload(0xFF, LED_TEST_RED, LED_TEST_GREEN, LED_TEST_BLUE, 0x64),
		pause_ms: 0,
	},
];

async fn submit_led_step(
	device: &mut Device,
	target: LedProbeTarget,
	step_index: usize,
	step: LedExperimentStep,
) -> bool {
	let desc = device.descriptor();
	let vendor_id = desc.vendor_id;
	let product_id = desc.product_id;

	crate::log!(
		"crabusb: leds {:04X}:{:04X} step={} label={} if#{} id5={:02X?} id6={:02X?} id1={:02X?}\n",
		vendor_id,
		product_id,
		step_index,
		step.label,
		target.interface_number,
		step.id5,
		step.id6,
		LED_COMMIT_REPORT
	);

	match send_hid_set_report(device, target.interface_number, 5, &step.id5).await {
		Ok(()) => crate::log!(
			"crabusb: leds {:04X}:{:04X} step={} id=5 submitted\n",
			vendor_id,
			product_id,
			step_index
		),
		Err(err) => {
			crate::log!(
				"crabusb: leds {:04X}:{:04X} step={} id=5 failed: {:?}\n",
				vendor_id,
				product_id,
				step_index,
				err
			);
			return false;
		}
	}

	match send_hid_set_report(device, target.interface_number, 6, &step.id6).await {
		Ok(()) => crate::log!(
			"crabusb: leds {:04X}:{:04X} step={} id=6 submitted\n",
			vendor_id,
			product_id,
			step_index
		),
		Err(err) => {
			crate::log!(
				"crabusb: leds {:04X}:{:04X} step={} id=6 failed: {:?}\n",
				vendor_id,
				product_id,
				step_index,
				err
			);
			return false;
		}
	}

	match send_hid_set_report(device, target.interface_number, 1, &LED_COMMIT_REPORT).await {
		Ok(()) => crate::log!(
			"crabusb: leds {:04X}:{:04X} step={} id=1 commit submitted\n",
			vendor_id,
			product_id,
			step_index
		),
		Err(err) => {
			crate::log!(
				"crabusb: leds {:04X}:{:04X} step={} id=1 commit failed: {:?}\n",
				vendor_id,
				product_id,
				step_index,
				err
			);
			return false;
		}
	}

	crate::log!(
		"crabusb: leds {:04X}:{:04X} step={} label={} observe now pause_ms={}\n",
		vendor_id,
		product_id,
		step_index,
		step.label,
		step.pause_ms
	);
	true
}

#[embassy_executor::task(pool_size = 2)]
async fn led_probe_task(mut device: Device, target: LedProbeTarget) {
	let desc = device.descriptor();
	let vendor_id = desc.vendor_id;
	let product_id = desc.product_id;
	let slot_id = device.slot_id();

	if !log_led_live_state(&mut device, target).await {
		return;
	}

	crate::log!(
		"crabusb: leds {:04X}:{:04X} shared probe slot={} if#{} cfg={} matrix_steps={} commit={:02X?}\n",
		vendor_id,
		product_id,
		slot_id,
		target.interface_number,
		target.configuration_value,
		LED_EXPERIMENT_STEPS.len(),
		LED_COMMIT_REPORT
	);

	read_led_feature_report_early(&mut device, target).await;

	for (step_index, step) in LED_EXPERIMENT_STEPS.iter().copied().enumerate() {
		if !submit_led_step(&mut device, target, step_index, step).await {
			return;
		}
		if step.pause_ms != 0 {
			Timer::after(EmbassyDuration::from_millis(step.pause_ms)).await;
		}
	}
}

pub(crate) async fn maybe_start_led_controller_with_device(
	device: Device,
	dev_info: &DeviceInfo,
	spawner: &Spawner,
	controller_id: u32,
) -> bool {
	let _ = controller_id;

	let desc = dev_info.descriptor();
	if !is_supported_led_controller(desc.vendor_id, desc.product_id) {
		return false;
	}

	let Some(target) = pick_led_probe_target(dev_info.configurations()) else {
		crate::log!(
			"crabusb: leds {:04X}:{:04X} no HID out interface for shared probe\n",
			desc.vendor_id,
			desc.product_id
		);
		return true;
	};

	match spawner.spawn(led_probe_task(device, target)) {
		Ok(()) => {
			crate::log!(
				"crabusb: leds {:04X}:{:04X} shared probe armed if#{} cfg={} matrix_steps={} final_rgb={},{},{}\n",
				desc.vendor_id,
				desc.product_id,
				target.interface_number,
				target.configuration_value,
				LED_EXPERIMENT_STEPS.len(),
				LED_TEST_RED,
				LED_TEST_GREEN,
				LED_TEST_BLUE
			);
		}
		Err(err) => {
			crate::log!(
				"crabusb: leds {:04X}:{:04X} shared probe spawn failed: {:?}\n",
				desc.vendor_id,
				desc.product_id,
				err
			);
		}
	}

	true
}

pub(crate) async fn maybe_start_led_controller(
	host: &mut USBHost,
	dev_info: &DeviceInfo,
	spawner: &Spawner,
	controller_id: u32,
) -> bool {
	let _ = controller_id;
	let _ = host;
	let _ = spawner;

	let desc = dev_info.descriptor();
	if !is_supported_led_controller(desc.vendor_id, desc.product_id) {
		return false;
	}

	let Some(target) = pick_led_probe_target(dev_info.configurations()) else {
		crate::log!(
			"crabusb: leds {:04X}:{:04X} no HID out interface for minimal probe\n",
			desc.vendor_id,
			desc.product_id
		);
		return true;
	};

	crate::log!(
		"crabusb: leds {:04X}:{:04X} candidate if#{} cfg={} rgb={},{},{} deferred: no probe-time reopen of stable HID leaf\n",
		desc.vendor_id,
		desc.product_id,
		target.interface_number,
		target.configuration_value,
		LED_TEST_RED,
		LED_TEST_GREEN,
		LED_TEST_BLUE
	);

	true
}
