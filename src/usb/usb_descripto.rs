#![allow(dead_code)]

use core::convert::TryFrom;

#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum UsbGenericDescriptorType {
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
}

impl TryFrom<u8> for UsbGenericDescriptorType {
    type Error = ();

    fn try_from(v: u8) -> Result<Self, Self::Error> {
        Ok(match v {
            0x01 => UsbGenericDescriptorType::Device,
            0x02 => UsbGenericDescriptorType::Configuration,
            0x03 => UsbGenericDescriptorType::String,
            0x04 => UsbGenericDescriptorType::Interface,
            0x05 => UsbGenericDescriptorType::Endpoint,
            0x06 => UsbGenericDescriptorType::DeviceQualifier,
            0x07 => UsbGenericDescriptorType::OtherSpeedConfiguration,
            0x08 => UsbGenericDescriptorType::InterfacePower,
            0x09 => UsbGenericDescriptorType::Otg,
            0x0A => UsbGenericDescriptorType::Debug,
            0x0B => UsbGenericDescriptorType::InterfaceAssociation,
            0x0F => UsbGenericDescriptorType::Bos,
            0x10 => UsbGenericDescriptorType::DeviceCapability,
            0x30 => UsbGenericDescriptorType::SsEndpointCompanion,
            _ => return Err(()),
        })
    }
}
