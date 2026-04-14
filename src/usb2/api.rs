use crab_usb::{
    Device, EndpointBulkIn, EndpointBulkOut, EndpointInterruptIn, EndpointInterruptOut,
    EndpointIsoIn, EndpointKind, err::USBError,
};

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
}

pub(crate) async fn claim_interface(
    device: &mut Device,
    interface_number: u8,
    alternate_setting: u8,
) -> Result<ClaimedInterface<'_>, USBError> {
    device
        .claim_interface(interface_number, alternate_setting)
        .await?;
    Ok(ClaimedInterface { device })
}

impl ClaimedInterface<'_> {
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

    pub(crate) async fn endpoint_isochronous_in(
        &mut self,
        address: u8,
    ) -> Result<EndpointIsoIn, InterfaceEndpointError> {
        match self.device.get_endpoint(address).await? {
            EndpointKind::IsochronousIn(endpoint) => Ok(endpoint),
            _ => Err(InterfaceEndpointError::WrongKind {
                address,
                expected: "iso-in",
            }),
        }
    }

    pub(crate) async fn endpoint_interrupt_in(
        &mut self,
        address: u8,
    ) -> Result<EndpointInterruptIn, InterfaceEndpointError> {
        match self.device.get_endpoint(address).await? {
            EndpointKind::InterruptIn(endpoint) => Ok(endpoint),
            _ => Err(InterfaceEndpointError::WrongKind {
                address,
                expected: "interrupt-in",
            }),
        }
    }

    pub(crate) async fn endpoint_interrupt_out(
        &mut self,
        address: u8,
    ) -> Result<EndpointInterruptOut, InterfaceEndpointError> {
        match self.device.get_endpoint(address).await? {
            EndpointKind::InterruptOut(endpoint) => Ok(endpoint),
            _ => Err(InterfaceEndpointError::WrongKind {
                address,
                expected: "interrupt-out",
            }),
        }
    }
}
