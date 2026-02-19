#![allow(dead_code)]

use core::convert::TryFrom;

use crate::usb::usb_descripto::UsbGenericDescriptorType;
use heapless::String;

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum HidDescriptorType {
	Hid = 0x21,
	HidReport = 0x22,
	HidPhysical = 0x23,
}

impl TryFrom<u8> for HidDescriptorType {
	type Error = ();

	fn try_from(v: u8) -> Result<Self, Self::Error> {
		Ok(match v {
			0x21 => HidDescriptorType::Hid,
			0x22 => HidDescriptorType::HidReport,
			0x23 => HidDescriptorType::HidPhysical,
			_ => return Err(()),
		})
	}
}

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum DescriptorType {
	Device = 0x01,
	Configuration = 0x02,
	String = 0x03,
	Interface = 0x04,
	Endpoint = 0x05,
	DeviceQualifier = 0x06,
	OtherSpeedConfiguration = 0x07,
	InterfacePower = 0x08,
	Otg = 0x09,
	Debug = 0x0A,
	InterfaceAssociation = 0x0B,
	Bos = 0x0F,
	DeviceCapability = 0x10,
	SsEndpointCompanion = 0x30,

	Hid = 0x21,
	HidReport = 0x22,
	HidPhysical = 0x23,
}

impl TryFrom<u8> for DescriptorType {
	type Error = ();

	fn try_from(v: u8) -> Result<Self, Self::Error> {
		Ok(match v {
			0x01 => DescriptorType::Device,
			0x02 => DescriptorType::Configuration,
			0x03 => DescriptorType::String,
			0x04 => DescriptorType::Interface,
			0x05 => DescriptorType::Endpoint,
			0x06 => DescriptorType::DeviceQualifier,
			0x07 => DescriptorType::OtherSpeedConfiguration,
			0x08 => DescriptorType::InterfacePower,
			0x09 => DescriptorType::Otg,
			0x0A => DescriptorType::Debug,
			0x0B => DescriptorType::InterfaceAssociation,
			0x0F => DescriptorType::Bos,
			0x10 => DescriptorType::DeviceCapability,
			0x30 => DescriptorType::SsEndpointCompanion,
			0x21 => DescriptorType::Hid,
			0x22 => DescriptorType::HidReport,
			0x23 => DescriptorType::HidPhysical,
			_ => return Err(()),
		})
	}
}

#[inline]
fn le_u16(b0: u8, b1: u8) -> u16 {
	u16::from_le_bytes([b0, b1])
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct DeviceDescriptor {
	pub usb_bcd: u16,
	pub device_class: u8,
	pub device_subclass: u8,
	pub device_protocol: u8,
	pub max_packet_size0: u8,
	pub vid: u16,
	pub pid: u16,
	pub device_bcd: u16,
	pub manufacturer_str: u8,
	pub product_str: u8,
	pub serial_str: u8,
	pub num_configurations: u8,
}

impl DeviceDescriptor {
	pub fn parse(b: &[u8]) -> Option<Self> {
		if b.len() < 18 || b[0] != 18 || b[1] != (DescriptorType::Device as u8) {
			return None;
		}
		Some(Self {
			usb_bcd: le_u16(b[2], b[3]),
			device_class: b[4],
			device_subclass: b[5],
			device_protocol: b[6],
			max_packet_size0: b[7],
			vid: le_u16(b[8], b[9]),
			pid: le_u16(b[10], b[11]),
			device_bcd: le_u16(b[12], b[13]),
			manufacturer_str: b[14],
			product_str: b[15],
			serial_str: b[16],
			num_configurations: b[17],
		})
	}
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ConfigurationDescriptor {
	pub total_length: u16,
	pub num_interfaces: u8,
	pub configuration_value: u8,
	pub configuration_str: u8,
	pub attributes: u8,
	pub max_power_2ma: u8,
}

impl ConfigurationDescriptor {
	pub fn parse(b: &[u8]) -> Option<Self> {
		if b.len() < 9 || b[0] != 9 || b[1] != (DescriptorType::Configuration as u8) {
			return None;
		}
		Some(Self {
			total_length: le_u16(b[2], b[3]),
			num_interfaces: b[4],
			configuration_value: b[5],
			configuration_str: b[6],
			attributes: b[7],
			max_power_2ma: b[8],
		})
	}
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct InterfaceDescriptor {
	pub interface_number: u8,
	pub alternate_setting: u8,
	pub num_endpoints: u8,
	pub interface_class: u8,
	pub interface_subclass: u8,
	pub interface_protocol: u8,
	pub interface_str: u8,
}

impl InterfaceDescriptor {
	pub fn parse(b: &[u8]) -> Option<Self> {
		if b.len() < 9 || b[0] != 9 || b[1] != (DescriptorType::Interface as u8) {
			return None;
		}
		Some(Self {
			interface_number: b[2],
			alternate_setting: b[3],
			num_endpoints: b[4],
			interface_class: b[5],
			interface_subclass: b[6],
			interface_protocol: b[7],
			interface_str: b[8],
		})
	}
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct EndpointDescriptor {
	pub endpoint_address: u8,
	pub attributes: u8,
	pub max_packet_size: u16,
	pub interval: u8,
}

impl EndpointDescriptor {
	pub fn parse(b: &[u8]) -> Option<Self> {
		if b.len() < 7 || b[0] != 7 || b[1] != (DescriptorType::Endpoint as u8) {
			return None;
		}
		Some(Self {
			endpoint_address: b[2],
			attributes: b[3],
			max_packet_size: le_u16(b[4], b[5]),
			interval: b[6],
		})
	}
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct InterfaceAssociationDescriptor {
	pub first_interface: u8,
	pub interface_count: u8,
	pub function_class: u8,
	pub function_subclass: u8,
	pub function_protocol: u8,
	pub function_str: u8,
}

impl InterfaceAssociationDescriptor {
	pub fn parse(b: &[u8]) -> Option<Self> {
		if b.len() < 8 || b[0] != 8 || b[1] != (DescriptorType::InterfaceAssociation as u8) {
			return None;
		}
		Some(Self {
			first_interface: b[2],
			interface_count: b[3],
			function_class: b[4],
			function_subclass: b[5],
			function_protocol: b[6],
			function_str: b[7],
		})
	}
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct DeviceQualifierDescriptor {
	pub usb_bcd: u16,
	pub device_class: u8,
	pub device_subclass: u8,
	pub device_protocol: u8,
	pub max_packet_size0: u8,
	pub num_configurations: u8,
}

impl DeviceQualifierDescriptor {
	pub fn parse(b: &[u8]) -> Option<Self> {
		if b.len() < 10 || b[0] != 10 || b[1] != (DescriptorType::DeviceQualifier as u8) {
			return None;
		}
		Some(Self {
			usb_bcd: le_u16(b[2], b[3]),
			device_class: b[4],
			device_subclass: b[5],
			device_protocol: b[6],
			max_packet_size0: b[7],
			num_configurations: b[8],
		})
	}
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct OtherSpeedConfigurationDescriptor {
	pub total_length: u16,
	pub num_interfaces: u8,
	pub configuration_value: u8,
	pub configuration_str: u8,
	pub attributes: u8,
	pub max_power_2ma: u8,
}

impl OtherSpeedConfigurationDescriptor {
	pub fn parse(b: &[u8]) -> Option<Self> {
		if b.len() < 9
			|| b[0] != 9
			|| b[1] != (DescriptorType::OtherSpeedConfiguration as u8)
		{
			return None;
		}
		Some(Self {
			total_length: le_u16(b[2], b[3]),
			num_interfaces: b[4],
			configuration_value: b[5],
			configuration_str: b[6],
			attributes: b[7],
			max_power_2ma: b[8],
		})
	}
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct BosDescriptor {
	pub total_length: u16,
	pub num_device_caps: u8,
}

impl BosDescriptor {
	pub fn parse(b: &[u8]) -> Option<Self> {
		if b.len() < 5 || b[1] != (DescriptorType::Bos as u8) {
			return None;
		}
		Some(Self {
			total_length: le_u16(b[2], b[3]),
			num_device_caps: b[4],
		})
	}
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct DeviceCapabilityDescriptor<'a> {
	pub cap_type: u8,
	pub raw: &'a [u8],
}

impl<'a> DeviceCapabilityDescriptor<'a> {
	pub fn parse(b: &'a [u8]) -> Option<Self> {
		if b.len() < 3 || b[1] != (DescriptorType::DeviceCapability as u8) {
			return None;
		}
		Some(Self {
			cap_type: b[2],
			raw: b,
		})
	}
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct StringDescriptor<'a> {
	pub langid: Option<u16>,
	pub raw_utf16le: &'a [u8],
}

impl<'a> StringDescriptor<'a> {
	pub fn parse(b: &'a [u8]) -> Option<Self> {
		if b.len() < 2 || b[1] != (DescriptorType::String as u8) {
			return None;
		}
		let langid = if b.len() >= 4 && (b.len() & 1) == 0 {
			Some(le_u16(b[2], b[3]))
		} else {
			None
		};
		let raw_utf16le = if b.len() >= 2 { &b[2..] } else { &b[0..0] };
		Some(Self { langid, raw_utf16le })
	}
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct HidDescriptor {
	pub hid_bcd: u16,
	pub country_code: u8,
	pub num_descriptors: u8,
	pub report_desc_len: Option<u16>,
}

impl HidDescriptor {
	pub fn parse(b: &[u8]) -> Option<Self> {
		if b.len() < 9 || b[1] != (DescriptorType::Hid as u8) {
			return None;
		}

		let num = b[5];
		let mut report_len: Option<u16> = None;
		if num >= 1 && b.len() >= 9 {
			let dt = b[6];
			let dl = le_u16(b[7], b[8]);
			if dt == (DescriptorType::HidReport as u8) {
				report_len = Some(dl);
			}
		}

		Some(Self {
			hid_bcd: le_u16(b[2], b[3]),
			country_code: b[4],
			num_descriptors: num,
			report_desc_len: report_len,
		})
	}
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct RawDescriptor<'a> {
	pub len: u8,
	pub ty: u8,
	pub bytes: &'a [u8],
}

pub struct DescriptorIter<'a> {
	bytes: &'a [u8],
	idx: usize,
}

impl<'a> DescriptorIter<'a> {
	pub fn new(bytes: &'a [u8]) -> Self {
		Self { bytes, idx: 0 }
	}
}

impl<'a> Iterator for DescriptorIter<'a> {
	type Item = RawDescriptor<'a>;

	fn next(&mut self) -> Option<Self::Item> {
		if self.idx + 2 > self.bytes.len() {
			return None;
		}
		let len = self.bytes[self.idx] as usize;
		let ty = self.bytes[self.idx + 1];
		if len == 0 || self.idx + len > self.bytes.len() {
			return None;
		}
		let bytes = &self.bytes[self.idx..self.idx + len];
		self.idx += len;
		Some(RawDescriptor {
			len: len as u8,
			ty,
			bytes,
		})
	}
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ParsedDescriptor<'a> {
	Device(DeviceDescriptor),
	Configuration(ConfigurationDescriptor),
	Interface(InterfaceDescriptor),
	Endpoint(EndpointDescriptor),
	InterfaceAssociation(InterfaceAssociationDescriptor),
	DeviceQualifier(DeviceQualifierDescriptor),
	OtherSpeedConfiguration(OtherSpeedConfigurationDescriptor),
	Bos(BosDescriptor),
	DeviceCapability(DeviceCapabilityDescriptor<'a>),
	String(StringDescriptor<'a>),
	Hid(HidDescriptor),
	Unknown(&'a [u8]),
}

pub fn parse_any_descriptor<'a>(d: RawDescriptor<'a>) -> ParsedDescriptor<'a> {
	match DescriptorType::try_from(d.ty) {
		Ok(DescriptorType::Device) => DeviceDescriptor::parse(d.bytes)
			.map(ParsedDescriptor::Device)
			.unwrap_or(ParsedDescriptor::Unknown(d.bytes)),
		Ok(DescriptorType::Configuration) => ConfigurationDescriptor::parse(d.bytes)
			.map(ParsedDescriptor::Configuration)
			.unwrap_or(ParsedDescriptor::Unknown(d.bytes)),
		Ok(DescriptorType::Interface) => InterfaceDescriptor::parse(d.bytes)
			.map(ParsedDescriptor::Interface)
			.unwrap_or(ParsedDescriptor::Unknown(d.bytes)),
		Ok(DescriptorType::Endpoint) => EndpointDescriptor::parse(d.bytes)
			.map(ParsedDescriptor::Endpoint)
			.unwrap_or(ParsedDescriptor::Unknown(d.bytes)),
		Ok(DescriptorType::InterfaceAssociation) => InterfaceAssociationDescriptor::parse(d.bytes)
			.map(ParsedDescriptor::InterfaceAssociation)
			.unwrap_or(ParsedDescriptor::Unknown(d.bytes)),
		Ok(DescriptorType::DeviceQualifier) => DeviceQualifierDescriptor::parse(d.bytes)
			.map(ParsedDescriptor::DeviceQualifier)
			.unwrap_or(ParsedDescriptor::Unknown(d.bytes)),
		Ok(DescriptorType::OtherSpeedConfiguration) => OtherSpeedConfigurationDescriptor::parse(d.bytes)
			.map(ParsedDescriptor::OtherSpeedConfiguration)
			.unwrap_or(ParsedDescriptor::Unknown(d.bytes)),
		Ok(DescriptorType::Bos) => BosDescriptor::parse(d.bytes)
			.map(ParsedDescriptor::Bos)
			.unwrap_or(ParsedDescriptor::Unknown(d.bytes)),
		Ok(DescriptorType::DeviceCapability) => DeviceCapabilityDescriptor::parse(d.bytes)
			.map(ParsedDescriptor::DeviceCapability)
			.unwrap_or(ParsedDescriptor::Unknown(d.bytes)),
		Ok(DescriptorType::String) => StringDescriptor::parse(d.bytes)
			.map(ParsedDescriptor::String)
			.unwrap_or(ParsedDescriptor::Unknown(d.bytes)),
		Ok(DescriptorType::Hid) => HidDescriptor::parse(d.bytes)
			.map(ParsedDescriptor::Hid)
			.unwrap_or(ParsedDescriptor::Unknown(d.bytes)),
		_ => ParsedDescriptor::Unknown(d.bytes),
	}
}

pub fn find_hid_report_desc_len(cfg: &[u8], iface: u8, alt: u8) -> Option<u16> {
	let mut cur_iface: Option<(u8, u8)> = None;
	for raw in DescriptorIter::new(cfg) {
		match parse_any_descriptor(raw) {
			ParsedDescriptor::Interface(id) => {
				cur_iface = Some((id.interface_number, id.alternate_setting));
			}
			ParsedDescriptor::Hid(h) => {
				if cur_iface == Some((iface, alt)) {
					if let Some(len) = h.report_desc_len {
						return Some(len);
					}
				}
			}
			_ => {}
		}
	}
	None
}

#[derive(Copy, Clone, Debug, Default)]
pub struct HidOutputReportFormat {
	pub report_id: u8,
	pub total_len_bytes: u16,
}

#[derive(Copy, Clone, Debug, Default)]
struct HidGlobalState {
	report_size_bits: u32,
	report_count: u32,
	report_id: u8,
}

pub fn parse_output_report_format(desc: &[u8]) -> Option<HidOutputReportFormat> {
	let mut idx: usize = 0;
	let mut state = HidGlobalState {
		report_size_bits: 0,
		report_count: 0,
		report_id: 0,
	};
	let mut stack: [HidGlobalState; 4] = [HidGlobalState::default(); 4];
	let mut sp: usize = 0;

	let mut best_id: u8 = 0;
	let mut best_payload_bytes: u16 = 0;

	while idx < desc.len() {
		let b = desc[idx];
		idx += 1;

		if b == 0xFE {
			if idx + 2 > desc.len() {
				break;
			}
			let data_size = desc[idx] as usize;
			idx += 2;
			idx = idx.saturating_add(data_size);
			continue;
		}

		let size_code = (b & 0x03) as usize;
		let data_size = match size_code {
			0 => 0,
			1 => 1,
			2 => 2,
			3 => 4,
			_ => 0,
		};
		let item_type = (b >> 2) & 0x03;
		let tag = (b >> 4) & 0x0F;

		if idx + data_size > desc.len() {
			break;
		}

		let mut value_u32: u32 = 0;
		for i in 0..data_size {
			value_u32 |= (desc[idx + i] as u32) << (8 * i);
		}
		idx += data_size;

		match (item_type, tag) {
			(1, 7) => {
				state.report_size_bits = value_u32;
			}
			(1, 9) => {
				state.report_count = value_u32;
			}
			(1, 8) => {
				state.report_id = (value_u32 & 0xFF) as u8;
			}
			(1, 10) => {
				if sp < stack.len() {
					stack[sp] = state;
					sp += 1;
				}
			}
			(1, 11) => {
				if sp > 0 {
					sp -= 1;
					state = stack[sp];
				}
			}
			(0, 9) => {
				let bits = state.report_size_bits.saturating_mul(state.report_count);
				if bits == 0 {
					continue;
				}
				let payload_bytes = ((bits + 7) / 8) as u16;
				if payload_bytes >= best_payload_bytes {
					best_payload_bytes = payload_bytes;
					best_id = state.report_id;
				}
			}
			_ => {}
		}
	}

	if best_payload_bytes == 0 {
		return None;
	}

	let total_len_bytes = best_payload_bytes.saturating_add((best_id != 0) as u16);
	Some(HidOutputReportFormat {
		report_id: best_id,
		total_len_bytes,
	})
}

pub fn hid_kind_from_iface(subclass: u8, protocol: u8) -> u8 {
	if subclass != 0x01 {
		return 0;
	}

	match protocol {
		0x01 => 1,
		0x02 => 2,
		_ => 0,
	}
}

pub fn hid_kind_from_report_desc(subclass: u8, protocol: u8, report_desc: &[u8]) -> u8 {
	let boot = hid_kind_from_iface(subclass, protocol);
	if boot != 0 {
		return boot;
	}

	if report_desc.windows(2).any(|w| w == [0x05, 0x0D]) {
		return 3;
	}

	if report_desc.windows(2).any(|w| w == [0x05, 0x07])
		|| (report_desc.windows(2).any(|w| w == [0x05, 0x01])
			&& report_desc.windows(2).any(|w| w == [0x09, 0x06]))
	{
		return 1;
	}

	let has_rel_input = report_desc
		.windows(2)
		.any(|w| w[0] == 0x81 && (w[1] & 0x04) != 0);
	let has_x = report_desc.windows(2).any(|w| w == [0x09, 0x30]);
	let has_y = report_desc.windows(2).any(|w| w == [0x09, 0x31]);
	if has_rel_input
		&& has_x
		&& has_y
		&& report_desc.windows(2).any(|w| w == [0x05, 0x01])
		&& report_desc.windows(2).any(|w| w == [0x09, 0x02])
	{
		return 2;
	}

	let has_abs_input = report_desc
		.windows(2)
		.any(|w| w[0] == 0x81 && (w[1] & 0x04) == 0);
	if has_x && has_y && has_abs_input {
		return 3;
	}

	0
}

pub fn log_all_descriptor_types(cfg: &[u8], port: u8, slot_id: u32) {
	crate::log!(
		"usb: desc-types port={} slot={} cfg_len={}\n",
		port,
		slot_id,
		cfg.len()
	);

	let mut idx: usize = 0;
	let mut cur_if: Option<(u8, u8, u8, u8, u8)> = None;
	while idx + 2 <= cfg.len() {
		let len = cfg[idx] as usize;
		let ty = cfg[idx + 1];
		if len == 0 || idx + len > cfg.len() {
			crate::log!(
				"usb: desc-types truncated idx={} len={} total={}\n",
				idx,
				len,
				cfg.len()
			);
			break;
		}

		let name = match DescriptorType::try_from(ty) {
			Ok(_) => {
				if let Ok(g) = UsbGenericDescriptorType::try_from(ty) {
					match g {
						UsbGenericDescriptorType::Device => "Device",
						UsbGenericDescriptorType::Configuration => "Configuration",
						UsbGenericDescriptorType::String => "String",
						UsbGenericDescriptorType::Interface => "Interface",
						UsbGenericDescriptorType::Endpoint => "Endpoint",
						UsbGenericDescriptorType::DeviceQualifier => "DeviceQualifier",
						UsbGenericDescriptorType::OtherSpeedConfiguration => "OtherSpeedConfiguration",
						UsbGenericDescriptorType::InterfacePower => "InterfacePower",
						UsbGenericDescriptorType::Otg => "Otg",
						UsbGenericDescriptorType::Debug => "Debug",
						UsbGenericDescriptorType::InterfaceAssociation => "InterfaceAssociation",
						UsbGenericDescriptorType::Bos => "Bos",
						UsbGenericDescriptorType::DeviceCapability => "DeviceCapability",
						UsbGenericDescriptorType::SsEndpointCompanion => "SsEndpointCompanion",
					}
				} else if let Ok(h) = HidDescriptorType::try_from(ty) {
					match h {
						HidDescriptorType::Hid => "Hid",
						HidDescriptorType::HidReport => "HidReport",
						HidDescriptorType::HidPhysical => "HidPhysical",
					}
				} else {
					"Unknown"
				}
			}
			Err(()) => "Unknown",
		};

		crate::log!("usb:  desc idx={} len={} ty=0x{:02X} {}\n", idx, len, ty, name);

		let bytes = &cfg[idx..idx + len];
		match DescriptorType::try_from(ty) {
			Ok(DescriptorType::Interface) if bytes.len() >= 9 => {
				let if_num = bytes[2];
				let alt = bytes[3];
				let cls = bytes[5];
				let sub = bytes[6];
				let prot = bytes[7];
				cur_if = Some((if_num, alt, cls, sub, prot));
				crate::log!(
					"usb:   if{} alt={} cls={:02X}/{:02X}/{:02X}\n",
					if_num,
					alt,
					cls,
					sub,
					prot
				);
			}
			Ok(DescriptorType::Endpoint) if bytes.len() >= 7 => {
				let ep = bytes[2];
				let attrs = bytes[3];
				let mps = le_u16(bytes[4], bytes[5]) & 0x07FF;
				let interval = bytes[6];
				crate::log!(
					"usb:   ep addr=0x{:02X} attrs=0x{:02X} mps={} interval={}\n",
					ep,
					attrs,
					mps,
					interval
				);
			}
			Ok(DescriptorType::SsEndpointCompanion) if bytes.len() >= 6 => {
				let max_burst = bytes[2];
				let attrs = bytes[3];
				let bytes_per_interval = le_u16(bytes[4], bytes[5]);
				crate::log!(
					"usb:   ss_ep_comp max_burst={} attrs=0x{:02X} bytes_per_interval={}\n",
					max_burst,
					attrs,
					bytes_per_interval
				);
			}
			Ok(DescriptorType::Hid) if bytes.len() >= 9 => {
				let hid_bcd = le_u16(bytes[2], bytes[3]);
				let country = bytes[4];
				let num_desc = bytes[5];
				let if_hint = cur_if.map(|(n, a, c, s, p)| (n, a, c, s, p));
				if let Some((ifn, alt, cls, sub, prot)) = if_hint {
					crate::log!(
						"usb:   hid if{} alt={} cls={:02X}/{:02X}/{:02X} bcd=0x{:04X} country={} numDesc={}\n",
						ifn,
						alt,
						cls,
						sub,
						prot,
						hid_bcd,
						country,
						num_desc
					);
				} else {
					crate::log!(
						"usb:   hid bcd=0x{:04X} country={} numDesc={}\n",
						hid_bcd,
						country,
						num_desc
					);
				}

				let mut j: usize = 0;
				while j < (num_desc as usize) {
					let base = 6 + j * 3;
					if base + 2 >= bytes.len() {
						break;
					}
					let dt = bytes[base];
					let dl = le_u16(bytes[base + 1], bytes[base + 2]);
					crate::log!("usb:    hid_desc[{}] type=0x{:02X} len={}\n", j, dt, dl);
					j += 1;
				}
			}
			_ => {}
		}

		idx += len;
	}
}

fn hex_bytes(bytes: &[u8]) -> String<32> {
	const HEX: &[u8; 16] = b"0123456789ABCDEF";
	let mut s: String<32> = String::new();
	let mut first = true;
	for &b in bytes.iter() {
		if !first {
			let _ = s.push(' ');
		}
		first = false;
		let _ = s.push(HEX[(b >> 4) as usize] as char);
		let _ = s.push(HEX[(b & 0xF) as usize] as char);
	}
	s
}

pub fn log_hid_report_like_descriptor_table(
	desc: &[u8],
	port: u8,
	slot_id: u32,
	iface: u8,
	which: &'static str,
) {
	crate::log!(
		"usb: hid {} table port={} slot={} iface={} len={}\n",
		which,
		port,
		slot_id,
		iface,
		desc.len()
	);
	crate::log!("usb:  off  raw              ty      tag   size  value\n");

	let mut idx: usize = 0;
	while idx < desc.len() {
		let off = idx;
		let b = desc[idx];
		idx += 1;

		if b == 0xFE {
			if idx + 2 > desc.len() {
				crate::log!("usb:  0x{:03X} FE <trunc>\n", off);
				break;
			}
			let data_size = desc[idx] as usize;
			let long_tag = desc[idx + 1];
			idx += 2;
			let end = core::cmp::min(idx + data_size, desc.len());
			let raw = &desc[off..end];
			crate::log!(
				"usb:  0x{:03X} {:<16} long    0x{:02X} {:>4}  0x{:08X}\n",
				off,
				hex_bytes(raw).as_str(),
				long_tag,
				data_size,
				0u32
			);
			idx = end;
			continue;
		}

		let size_code = (b & 0x03) as usize;
		let data_size = match size_code {
			0 => 0,
			1 => 1,
			2 => 2,
			3 => 4,
			_ => 0,
		};
		let item_type = (b >> 2) & 0x03;
		let tag = (b >> 4) & 0x0F;

		if idx + data_size > desc.len() {
			crate::log!(
				"usb:  0x{:03X} {:<16} <trunc>\n",
				off,
				hex_bytes(&desc[off..]).as_str()
			);
			break;
		}

		let raw = &desc[off..idx + data_size];
		let mut value_u32: u32 = 0;
		for i in 0..data_size {
			value_u32 |= (desc[idx + i] as u32) << (8 * i);
		}
		idx += data_size;

		let ty_str = match item_type {
			0 => "main",
			1 => "global",
			2 => "local",
			_ => "resv",
		};

		let tag_str: &'static str = match (item_type, tag) {
			(0, 8) => "Input",
			(0, 9) => "Output",
			(0, 10) => "Collection",
			(0, 11) => "Feature",
			(0, 12) => "EndColl",
			(1, 0) => "UsagePg",
			(1, 1) => "LogMin",
			(1, 2) => "LogMax",
			(1, 3) => "PhysMin",
			(1, 4) => "PhysMax",
			(1, 5) => "UnitExp",
			(1, 6) => "Unit",
			(1, 7) => "RepSize",
			(1, 8) => "RepID",
			(1, 9) => "RepCnt",
			(1, 10) => "Push",
			(1, 11) => "Pop",
			(2, 0) => "Usage",
			(2, 1) => "UseMin",
			(2, 2) => "UseMax",
			_ => "Tag",
		};

		let value_i32 = if data_size == 0 {
			0i32
		} else {
			let shift = 32u32.saturating_sub((data_size as u32) * 8);
			((value_u32 << shift) as i32) >> shift
		};

		crate::log!(
			"usb:  0x{:03X} {:<16} {:<7} {:<7} {:>4} 0x{:08X} ({})\n",
			off,
			hex_bytes(raw).as_str(),
			ty_str,
			tag_str,
			data_size,
			value_u32,
			value_i32
		);
	}
}
