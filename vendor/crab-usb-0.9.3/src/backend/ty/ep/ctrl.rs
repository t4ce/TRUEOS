use alloc::vec::Vec;

use usb_if::{
    descriptor::{ConfigurationDescriptor, DescriptorType, DeviceDescriptor},
    endpoint::TransferRequest,
    err::{TransferError, USBError},
    host::ControlSetup,
    transfer::{Recipient, Request, RequestType},
};

use super::Endpoint;

impl Endpoint {
    pub async fn control_in(
        &mut self,
        param: usb_if::host::ControlSetup,
        buff: &mut [u8],
    ) -> Result<usize, TransferError> {
        let trace_descriptor_read = matches!(param.request, Request::GetDescriptor)
            && (param.value >> 8) == DescriptorType::CONFIGURATION.0 as u16;
        if trace_descriptor_read {
            info!(
                "crabusb/ctrl: control_in begin req={:?} type={:?} recip={:?} value=0x{:04x} index=0x{:04x} len={}",
                param.request,
                param.request_type,
                param.recipient,
                param.value,
                param.index,
                buff.len()
            );
        }
        let t = self.wait(TransferRequest::control_in(param, buff)).await?;
        if trace_descriptor_read {
            info!(
                "crabusb/ctrl: control_in end actual={} status={:?}",
                t.actual_length,
                t.status
            );
        }
        Ok(t.actual_length)
    }

    pub async fn control_out(
        &mut self,
        param: usb_if::host::ControlSetup,
        buff: &[u8],
    ) -> Result<usize, TransferError> {
        let t = self.wait(TransferRequest::control_out(param, buff)).await?;
        Ok(t.actual_length)
    }

    pub async fn set_configuration(
        &mut self,
        configuration_value: u8,
    ) -> Result<(), TransferError> {
        self.control_out(
            ControlSetup {
                request_type: RequestType::Standard,
                recipient: Recipient::Device,
                request: Request::SetConfiguration,
                value: configuration_value as u16,
                index: 0,
            },
            &[],
        )
        .await?;
        Ok(())
    }

    pub async fn get_descriptor(
        &mut self,
        desc_type: DescriptorType,
        desc_index: u8,
        language_id: u16,
        buff: &mut [u8],
    ) -> Result<usize, TransferError> {
        self.control_in(
            ControlSetup {
                request_type: RequestType::Standard,
                recipient: Recipient::Device,
                request: Request::GetDescriptor,
                value: ((desc_type.0 as u16) << 8) | desc_index as u16,
                index: language_id,
            },
            buff,
        )
        .await
    }

    pub async fn get_device_descriptor(&mut self) -> Result<DeviceDescriptor, USBError> {
        let mut buff = alloc::vec![0u8; DeviceDescriptor::LEN];
        self.get_descriptor(DescriptorType::DEVICE, 0, 0, &mut buff)
            .await?;
        trace!("data: {buff:?}");
        let desc = DeviceDescriptor::parse(&buff).ok_or(anyhow!("device descriptor parse err"))?;

        Ok(desc)
    }

    pub async fn get_configuration(&mut self) -> Result<u8, TransferError> {
        let mut buff = alloc::vec![0u8; 1];
        self.control_in(
            ControlSetup {
                request_type: RequestType::Standard,
                recipient: Recipient::Device,
                request: Request::GetConfiguration,
                value: 0,
                index: 0,
            },
            &mut buff,
        )
        .await?;
        Ok(buff[0])
    }

    pub async fn get_configuration_descriptor(
        &mut self,
        index: u8,
    ) -> Result<ConfigurationDescriptor, USBError> {
        let full_data = self.get_configuration_descriptor_bytes(index).await?;

        ConfigurationDescriptor::parse(&full_data)
            .ok_or_else(|| anyhow!("config descriptor parse err").into())
    }

    pub async fn get_configuration_descriptor_bytes(
        &mut self,
        index: u8,
    ) -> Result<Vec<u8>, USBError> {
        let mut header = alloc::vec![0u8; ConfigurationDescriptor::LEN];
        let header_len = self
            .get_descriptor(DescriptorType::CONFIGURATION, index, 0, &mut header)
            .await?;
        if header_len < 4 {
            return Err(anyhow!("short config descriptor header").into());
        }

        let total_length = u16::from_le_bytes(header[2..4].try_into().unwrap()) as usize;
        if total_length < ConfigurationDescriptor::LEN {
            return Err(anyhow!("invalid config descriptor length {total_length}").into());
        }

        let mut full_data = alloc::vec![0u8; total_length];
        debug!("Reading configuration descriptor for index {index}, total length: {total_length}");
        self.get_descriptor(DescriptorType::CONFIGURATION, index, 0, &mut full_data)
            .await?;

        Ok(full_data)
    }
}
