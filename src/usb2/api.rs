use crab_usb::{Device, EndpointBulkIn, EndpointBulkOut, EndpointKind, err::USBError};

#[derive(Debug)]
pub(crate) enum InterfaceEndpointError {
    Usb(USBError),
    WrongKind { address: u8, expected: &'static str },
}

impl From<USBError> for InterfaceEndpointError {
    fn from(value: USBError) -> Self {
        Self::Usb(value)
    }
}

pub(crate) struct ClaimedInterface<'a> {
    device: &'a mut Device,
    interface_number: u8,
    alternate_setting: u8,
}

pub(crate) async fn claim_interface(
    device: &mut Device,
    interface_number: u8,
    alternate_setting: u8,
) -> Result<ClaimedInterface<'_>, USBError> {
    device
        .claim_interface(interface_number, alternate_setting)
        .await?;
    Ok(ClaimedInterface {
        device,
        interface_number,
        alternate_setting,
    })
}

impl ClaimedInterface<'_> {
    pub(crate) fn interface_number(&self) -> u8 {
        self.interface_number
    }

    pub(crate) fn alternate_setting(&self) -> u8 {
        self.alternate_setting
    }

    pub(crate) fn device(&mut self) -> &mut Device {
        self.device
    }

    pub(crate) async fn endpoint_bulk_in(
        &mut self,
        address: u8,
    ) -> Result<EndpointBulkIn, InterfaceEndpointError> {
        match self.device.get_endpoint(address).await? {
            EndpointKind::BulkIn(endpoint) => Ok(endpoint),
            _ => Err(InterfaceEndpointError::WrongKind {
                address,
                expected: "bulk-in",
            }),
        }
    }

    pub(crate) async fn endpoint_bulk_out(
        &mut self,
        address: u8,
    ) -> Result<EndpointBulkOut, InterfaceEndpointError> {
        match self.device.get_endpoint(address).await? {
            EndpointKind::BulkOut(endpoint) => Ok(endpoint),
            _ => Err(InterfaceEndpointError::WrongKind {
                address,
                expected: "bulk-out",
            }),
        }
    }
}
