#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum UsbClassDescriptorUsage {
    Device,
    Interface,
    Both,
}

impl UsbClassDescriptorUsage {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Device => "device",
            Self::Interface => "interface",
            Self::Both => "both",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum UsbBaseClass {
    PerInterface,
    Audio,
    CommunicationsCdcControl,
    Hid,
    Physical,
    Image,
    Printer,
    MassStorage,
    Hub,
    CdcData,
    SmartCard,
    ContentSecurity,
    Video,
    PersonalHealthcare,
    AudioVideo,
    Billboard,
    TypeCBridge,
    BulkDisplay,
    Mctp,
    I3c,
    Diagnostic,
    WirelessController,
    Miscellaneous,
    ApplicationSpecific,
    VendorSpecific,
    Reserved(u8),
}

impl UsbBaseClass {
    pub(crate) const fn from_u8(class: u8) -> Self {
        match class {
            0x00 => Self::PerInterface,
            0x01 => Self::Audio,
            0x02 => Self::CommunicationsCdcControl,
            0x03 => Self::Hid,
            0x05 => Self::Physical,
            0x06 => Self::Image,
            0x07 => Self::Printer,
            0x08 => Self::MassStorage,
            0x09 => Self::Hub,
            0x0A => Self::CdcData,
            0x0B => Self::SmartCard,
            0x0D => Self::ContentSecurity,
            0x0E => Self::Video,
            0x0F => Self::PersonalHealthcare,
            0x10 => Self::AudioVideo,
            0x11 => Self::Billboard,
            0x12 => Self::TypeCBridge,
            0x13 => Self::BulkDisplay,
            0x14 => Self::Mctp,
            0x3C => Self::I3c,
            0xDC => Self::Diagnostic,
            0xE0 => Self::WirelessController,
            0xEF => Self::Miscellaneous,
            0xFE => Self::ApplicationSpecific,
            0xFF => Self::VendorSpecific,
            other => Self::Reserved(other),
        }
    }

    pub(crate) const fn code(self) -> u8 {
        match self {
            Self::PerInterface => 0x00,
            Self::Audio => 0x01,
            Self::CommunicationsCdcControl => 0x02,
            Self::Hid => 0x03,
            Self::Physical => 0x05,
            Self::Image => 0x06,
            Self::Printer => 0x07,
            Self::MassStorage => 0x08,
            Self::Hub => 0x09,
            Self::CdcData => 0x0A,
            Self::SmartCard => 0x0B,
            Self::ContentSecurity => 0x0D,
            Self::Video => 0x0E,
            Self::PersonalHealthcare => 0x0F,
            Self::AudioVideo => 0x10,
            Self::Billboard => 0x11,
            Self::TypeCBridge => 0x12,
            Self::BulkDisplay => 0x13,
            Self::Mctp => 0x14,
            Self::I3c => 0x3C,
            Self::Diagnostic => 0xDC,
            Self::WirelessController => 0xE0,
            Self::Miscellaneous => 0xEF,
            Self::ApplicationSpecific => 0xFE,
            Self::VendorSpecific => 0xFF,
            Self::Reserved(code) => code,
        }
    }

    pub(crate) const fn descriptor_usage(self) -> UsbClassDescriptorUsage {
        match self {
            Self::PerInterface => UsbClassDescriptorUsage::Device,
            Self::Audio => UsbClassDescriptorUsage::Interface,
            Self::CommunicationsCdcControl => UsbClassDescriptorUsage::Both,
            Self::Hid => UsbClassDescriptorUsage::Interface,
            Self::Physical => UsbClassDescriptorUsage::Interface,
            Self::Image => UsbClassDescriptorUsage::Interface,
            Self::Printer => UsbClassDescriptorUsage::Interface,
            Self::MassStorage => UsbClassDescriptorUsage::Interface,
            Self::Hub => UsbClassDescriptorUsage::Device,
            Self::CdcData => UsbClassDescriptorUsage::Interface,
            Self::SmartCard => UsbClassDescriptorUsage::Interface,
            Self::ContentSecurity => UsbClassDescriptorUsage::Interface,
            Self::Video => UsbClassDescriptorUsage::Interface,
            Self::PersonalHealthcare => UsbClassDescriptorUsage::Interface,
            Self::AudioVideo => UsbClassDescriptorUsage::Interface,
            Self::Billboard => UsbClassDescriptorUsage::Device,
            Self::TypeCBridge => UsbClassDescriptorUsage::Interface,
            Self::BulkDisplay => UsbClassDescriptorUsage::Interface,
            Self::Mctp => UsbClassDescriptorUsage::Interface,
            Self::I3c => UsbClassDescriptorUsage::Interface,
            Self::Diagnostic => UsbClassDescriptorUsage::Both,
            Self::WirelessController => UsbClassDescriptorUsage::Interface,
            Self::Miscellaneous => UsbClassDescriptorUsage::Both,
            Self::ApplicationSpecific => UsbClassDescriptorUsage::Interface,
            Self::VendorSpecific => UsbClassDescriptorUsage::Both,
            Self::Reserved(_) => UsbClassDescriptorUsage::Both,
        }
    }

    pub(crate) const fn short_name(self) -> &'static str {
        match self {
            Self::PerInterface => "per-interface",
            Self::Audio => "audio",
            Self::CommunicationsCdcControl => "comm",
            Self::Hid => "hid",
            Self::Physical => "physical",
            Self::Image => "image",
            Self::Printer => "printer",
            Self::MassStorage => "mass-storage",
            Self::Hub => "hub",
            Self::CdcData => "cdc-data",
            Self::SmartCard => "smart-card",
            Self::ContentSecurity => "content-security",
            Self::Video => "video",
            Self::PersonalHealthcare => "healthcare",
            Self::AudioVideo => "audio-video",
            Self::Billboard => "billboard",
            Self::TypeCBridge => "type-c-bridge",
            Self::BulkDisplay => "bulk-display",
            Self::Mctp => "mctp",
            Self::I3c => "i3c",
            Self::Diagnostic => "diagnostic",
            Self::WirelessController => "wireless",
            Self::Miscellaneous => "misc",
            Self::ApplicationSpecific => "app-specific",
            Self::VendorSpecific => "vendor",
            Self::Reserved(_) => "reserved",
        }
    }

    pub(crate) const fn description(self) -> &'static str {
        match self {
            Self::PerInterface => "Use class information in the Interface Descriptors",
            Self::Audio => "Audio",
            Self::CommunicationsCdcControl => "Communications and CDC Control",
            Self::Hid => "HID (Human Interface Device)",
            Self::Physical => "Physical",
            Self::Image => "Image",
            Self::Printer => "Printer",
            Self::MassStorage => "Mass Storage",
            Self::Hub => "Hub",
            Self::CdcData => "CDC-Data",
            Self::SmartCard => "Smart Card",
            Self::ContentSecurity => "Content Security",
            Self::Video => "Video",
            Self::PersonalHealthcare => "Personal Healthcare",
            Self::AudioVideo => "Audio/Video Devices",
            Self::Billboard => "Billboard Device Class",
            Self::TypeCBridge => "USB Type-C Bridge Class",
            Self::BulkDisplay => "USB Bulk Display Protocol Device Class",
            Self::Mctp => "MCTP over USB Protocol Endpoint Device Class",
            Self::I3c => "I3C Device Class",
            Self::Diagnostic => "Diagnostic Device",
            Self::WirelessController => "Wireless Controller",
            Self::Miscellaneous => "Miscellaneous",
            Self::ApplicationSpecific => "Application Specific",
            Self::VendorSpecific => "Vendor Specific",
            Self::Reserved(_) => "Reserved / unknown USB base class",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum UsbClassTriple {
    PerInterface,
    AudioGeneric,
    CommunicationsGeneric,
    HidGeneric,
    PhysicalGeneric,
    StillImaging,
    PrinterGeneric,
    MassStorageGeneric,
    FullSpeedHub,
    HighSpeedHubSingleTt,
    HighSpeedHubMultiTt,
    CdcDataGeneric,
    SmartCardGeneric,
    ContentSecurity,
    VideoGeneric,
    PersonalHealthcareGeneric,
    AudioVideoControl,
    AudioVideoVideoStreaming,
    AudioVideoAudioStreaming,
    Billboard,
    TypeCBridge,
    BulkDisplay,
    MctpEndpointV1,
    MctpEndpointV2,
    MctpHostInterfaceV1,
    MctpHostInterfaceV2,
    I3c,
    DiagnosticUsb2Compliance,
    DiagnosticDebugTargetVendorDefined,
    DiagnosticGnuRemoteDebug,
    DiagnosticTraceDbcUndefined,
    DiagnosticTraceDbcVendorDefined,
    DiagnosticDfxDbcUndefined,
    DiagnosticDfxDbcVendorDefined,
    DiagnosticTraceDvcGpVendorDefined,
    DiagnosticTraceDvcGpGnu,
    DiagnosticDfxDvcUndefined,
    DiagnosticDfxDvcVendorDefined,
    DiagnosticTraceDvcUndefined,
    DiagnosticTraceDvcVendorDefined,
    DiagnosticReserved08,
    WirelessBluetooth,
    WirelessUwb,
    WirelessRemoteNdis,
    WirelessBluetoothAmp,
    WirelessHostWireAdapter,
    WirelessDeviceWireAdapter,
    WirelessDeviceWireAdapterIsochronous,
    MiscActiveSync,
    MiscPalmSync,
    MiscInterfaceAssociationDescriptor,
    MiscWireAdapterMultifunctionPeripheral,
    MiscCableBasedAssociationFramework,
    MiscRndisEthernet,
    MiscRndisWifi,
    MiscRndisWimax,
    MiscRndisWwan,
    MiscRndisRawIpv4,
    MiscRndisRawIpv6,
    MiscRndisGprs,
    MiscUsb3VisionControl,
    MiscUsb3VisionEvent,
    MiscUsb3VisionStreaming,
    MiscStep,
    MiscStepRaw,
    MiscCommandInterfaceIad,
    MiscCommandInterfaceDescriptor,
    MiscMediaInterfaceDescriptor,
    MiscOcpSecureFirmwareRecovery,
    MiscOcpObmfIcp,
    AppDeviceFirmwareUpgrade,
    AppIrdaBridge,
    AppUsbTestMeasurement,
    AppUsbTestMeasurementUsb488,
    VendorSpecific,
    Unclassified {
        base: UsbBaseClass,
        subclass: u8,
        protocol: u8,
    },
}

impl UsbClassTriple {
    pub(crate) const fn from_codes(class: u8, subclass: u8, protocol: u8) -> Self {
        let base = UsbBaseClass::from_u8(class);
        match (class, subclass, protocol) {
            (0x00, 0x00, 0x00) => Self::PerInterface,
            (0x01, _, _) => Self::AudioGeneric,
            (0x02, _, _) => Self::CommunicationsGeneric,
            (0x03, _, _) => Self::HidGeneric,
            (0x05, _, _) => Self::PhysicalGeneric,
            (0x06, 0x01, 0x01) => Self::StillImaging,
            (0x07, _, _) => Self::PrinterGeneric,
            (0x08, _, _) => Self::MassStorageGeneric,
            (0x09, 0x00, 0x00) => Self::FullSpeedHub,
            (0x09, 0x00, 0x01) => Self::HighSpeedHubSingleTt,
            (0x09, 0x00, 0x02) => Self::HighSpeedHubMultiTt,
            (0x0A, _, _) => Self::CdcDataGeneric,
            (0x0B, _, _) => Self::SmartCardGeneric,
            (0x0D, 0x00, 0x00) => Self::ContentSecurity,
            (0x0E, _, _) => Self::VideoGeneric,
            (0x0F, _, _) => Self::PersonalHealthcareGeneric,
            (0x10, 0x01, 0x00) => Self::AudioVideoControl,
            (0x10, 0x02, 0x00) => Self::AudioVideoVideoStreaming,
            (0x10, 0x03, 0x00) => Self::AudioVideoAudioStreaming,
            (0x11, 0x00, 0x00) => Self::Billboard,
            (0x12, 0x00, 0x00) => Self::TypeCBridge,
            (0x13, 0x00, 0x00) => Self::BulkDisplay,
            (0x14, 0x00, 0x01) => Self::MctpEndpointV1,
            (0x14, 0x00, 0x02) => Self::MctpEndpointV2,
            (0x14, 0x01, 0x01) => Self::MctpHostInterfaceV1,
            (0x14, 0x01, 0x02) => Self::MctpHostInterfaceV2,
            (0x3C, 0x00, 0x00) => Self::I3c,
            (0xDC, 0x01, 0x01) => Self::DiagnosticUsb2Compliance,
            (0xDC, 0x02, 0x00) => Self::DiagnosticDebugTargetVendorDefined,
            (0xDC, 0x02, 0x01) => Self::DiagnosticGnuRemoteDebug,
            (0xDC, 0x03, 0x00) => Self::DiagnosticTraceDbcUndefined,
            (0xDC, 0x03, 0x01) => Self::DiagnosticTraceDbcVendorDefined,
            (0xDC, 0x04, 0x00) => Self::DiagnosticDfxDbcUndefined,
            (0xDC, 0x04, 0x01) => Self::DiagnosticDfxDbcVendorDefined,
            (0xDC, 0x05, 0x00) => Self::DiagnosticTraceDvcGpVendorDefined,
            (0xDC, 0x05, 0x01) => Self::DiagnosticTraceDvcGpGnu,
            (0xDC, 0x06, 0x00) => Self::DiagnosticDfxDvcUndefined,
            (0xDC, 0x06, 0x01) => Self::DiagnosticDfxDvcVendorDefined,
            (0xDC, 0x07, 0x00) => Self::DiagnosticTraceDvcUndefined,
            (0xDC, 0x07, 0x01) => Self::DiagnosticTraceDvcVendorDefined,
            (0xDC, 0x08, 0x00) => Self::DiagnosticReserved08,
            (0xE0, 0x01, 0x01) => Self::WirelessBluetooth,
            (0xE0, 0x01, 0x02) => Self::WirelessUwb,
            (0xE0, 0x01, 0x03) => Self::WirelessRemoteNdis,
            (0xE0, 0x01, 0x04) => Self::WirelessBluetoothAmp,
            (0xE0, 0x02, 0x01) => Self::WirelessHostWireAdapter,
            (0xE0, 0x02, 0x02) => Self::WirelessDeviceWireAdapter,
            (0xE0, 0x02, 0x03) => Self::WirelessDeviceWireAdapterIsochronous,
            (0xEF, 0x01, 0x01) => Self::MiscActiveSync,
            (0xEF, 0x01, 0x02) => Self::MiscPalmSync,
            (0xEF, 0x02, 0x01) => Self::MiscInterfaceAssociationDescriptor,
            (0xEF, 0x02, 0x02) => Self::MiscWireAdapterMultifunctionPeripheral,
            (0xEF, 0x03, 0x01) => Self::MiscCableBasedAssociationFramework,
            (0xEF, 0x04, 0x01) => Self::MiscRndisEthernet,
            (0xEF, 0x04, 0x02) => Self::MiscRndisWifi,
            (0xEF, 0x04, 0x03) => Self::MiscRndisWimax,
            (0xEF, 0x04, 0x04) => Self::MiscRndisWwan,
            (0xEF, 0x04, 0x05) => Self::MiscRndisRawIpv4,
            (0xEF, 0x04, 0x06) => Self::MiscRndisRawIpv6,
            (0xEF, 0x04, 0x07) => Self::MiscRndisGprs,
            (0xEF, 0x05, 0x00) => Self::MiscUsb3VisionControl,
            (0xEF, 0x05, 0x01) => Self::MiscUsb3VisionEvent,
            (0xEF, 0x05, 0x02) => Self::MiscUsb3VisionStreaming,
            (0xEF, 0x06, 0x01) => Self::MiscStep,
            (0xEF, 0x06, 0x02) => Self::MiscStepRaw,
            (0xEF, 0x07, 0x01) => Self::MiscCommandInterfaceIad,
            (0xEF, 0x07, 0x02) => Self::MiscMediaInterfaceDescriptor,
            (0xEF, 0x08, 0x01) => Self::MiscOcpSecureFirmwareRecovery,
            (0xEF, 0x09, 0x01) => Self::MiscOcpObmfIcp,
            (0xFE, 0x01, 0x01) => Self::AppDeviceFirmwareUpgrade,
            (0xFE, 0x02, 0x00) => Self::AppIrdaBridge,
            (0xFE, 0x03, 0x00) => Self::AppUsbTestMeasurement,
            (0xFE, 0x03, 0x01) => Self::AppUsbTestMeasurementUsb488,
            (0xFF, _, _) => Self::VendorSpecific,
            _ => Self::Unclassified {
                base,
                subclass,
                protocol,
            },
        }
    }

    pub(crate) const fn base_class(self) -> UsbBaseClass {
        match self {
            Self::PerInterface => UsbBaseClass::PerInterface,
            Self::AudioGeneric => UsbBaseClass::Audio,
            Self::CommunicationsGeneric => UsbBaseClass::CommunicationsCdcControl,
            Self::HidGeneric => UsbBaseClass::Hid,
            Self::PhysicalGeneric => UsbBaseClass::Physical,
            Self::StillImaging => UsbBaseClass::Image,
            Self::PrinterGeneric => UsbBaseClass::Printer,
            Self::MassStorageGeneric => UsbBaseClass::MassStorage,
            Self::FullSpeedHub | Self::HighSpeedHubSingleTt | Self::HighSpeedHubMultiTt => {
                UsbBaseClass::Hub
            }
            Self::CdcDataGeneric => UsbBaseClass::CdcData,
            Self::SmartCardGeneric => UsbBaseClass::SmartCard,
            Self::ContentSecurity => UsbBaseClass::ContentSecurity,
            Self::VideoGeneric => UsbBaseClass::Video,
            Self::PersonalHealthcareGeneric => UsbBaseClass::PersonalHealthcare,
            Self::AudioVideoControl
            | Self::AudioVideoVideoStreaming
            | Self::AudioVideoAudioStreaming => UsbBaseClass::AudioVideo,
            Self::Billboard => UsbBaseClass::Billboard,
            Self::TypeCBridge => UsbBaseClass::TypeCBridge,
            Self::BulkDisplay => UsbBaseClass::BulkDisplay,
            Self::MctpEndpointV1
            | Self::MctpEndpointV2
            | Self::MctpHostInterfaceV1
            | Self::MctpHostInterfaceV2 => UsbBaseClass::Mctp,
            Self::I3c => UsbBaseClass::I3c,
            Self::DiagnosticUsb2Compliance
            | Self::DiagnosticDebugTargetVendorDefined
            | Self::DiagnosticGnuRemoteDebug
            | Self::DiagnosticTraceDbcUndefined
            | Self::DiagnosticTraceDbcVendorDefined
            | Self::DiagnosticDfxDbcUndefined
            | Self::DiagnosticDfxDbcVendorDefined
            | Self::DiagnosticTraceDvcGpVendorDefined
            | Self::DiagnosticTraceDvcGpGnu
            | Self::DiagnosticDfxDvcUndefined
            | Self::DiagnosticDfxDvcVendorDefined
            | Self::DiagnosticTraceDvcUndefined
            | Self::DiagnosticTraceDvcVendorDefined
            | Self::DiagnosticReserved08 => UsbBaseClass::Diagnostic,
            Self::WirelessBluetooth
            | Self::WirelessUwb
            | Self::WirelessRemoteNdis
            | Self::WirelessBluetoothAmp
            | Self::WirelessHostWireAdapter
            | Self::WirelessDeviceWireAdapter
            | Self::WirelessDeviceWireAdapterIsochronous => UsbBaseClass::WirelessController,
            Self::MiscActiveSync
            | Self::MiscPalmSync
            | Self::MiscInterfaceAssociationDescriptor
            | Self::MiscWireAdapterMultifunctionPeripheral
            | Self::MiscCableBasedAssociationFramework
            | Self::MiscRndisEthernet
            | Self::MiscRndisWifi
            | Self::MiscRndisWimax
            | Self::MiscRndisWwan
            | Self::MiscRndisRawIpv4
            | Self::MiscRndisRawIpv6
            | Self::MiscRndisGprs
            | Self::MiscUsb3VisionControl
            | Self::MiscUsb3VisionEvent
            | Self::MiscUsb3VisionStreaming
            | Self::MiscStep
            | Self::MiscStepRaw
            | Self::MiscCommandInterfaceIad
            | Self::MiscCommandInterfaceDescriptor
            | Self::MiscMediaInterfaceDescriptor
            | Self::MiscOcpSecureFirmwareRecovery
            | Self::MiscOcpObmfIcp => UsbBaseClass::Miscellaneous,
            Self::AppDeviceFirmwareUpgrade
            | Self::AppIrdaBridge
            | Self::AppUsbTestMeasurement
            | Self::AppUsbTestMeasurementUsb488 => UsbBaseClass::ApplicationSpecific,
            Self::VendorSpecific => UsbBaseClass::VendorSpecific,
            Self::Unclassified { base, .. } => base,
        }
    }

    pub(crate) const fn short_name(self) -> &'static str {
        match self {
            Self::PerInterface => "per-interface",
            Self::AudioGeneric => "audio",
            Self::CommunicationsGeneric => "comm",
            Self::HidGeneric => "hid",
            Self::PhysicalGeneric => "physical",
            Self::StillImaging => "still-imaging",
            Self::PrinterGeneric => "printer",
            Self::MassStorageGeneric => "mass-storage",
            Self::FullSpeedHub => "hub-fs",
            Self::HighSpeedHubSingleTt => "hub-hs-single-tt",
            Self::HighSpeedHubMultiTt => "hub-hs-multi-tt",
            Self::CdcDataGeneric => "cdc-data",
            Self::SmartCardGeneric => "smart-card",
            Self::ContentSecurity => "content-security",
            Self::VideoGeneric => "video",
            Self::PersonalHealthcareGeneric => "healthcare",
            Self::AudioVideoControl => "av-control",
            Self::AudioVideoVideoStreaming => "av-video-stream",
            Self::AudioVideoAudioStreaming => "av-audio-stream",
            Self::Billboard => "billboard",
            Self::TypeCBridge => "type-c-bridge",
            Self::BulkDisplay => "bulk-display",
            Self::MctpEndpointV1 => "mctp-endpoint-v1",
            Self::MctpEndpointV2 => "mctp-endpoint-v2",
            Self::MctpHostInterfaceV1 => "mctp-host-v1",
            Self::MctpHostInterfaceV2 => "mctp-host-v2",
            Self::I3c => "i3c",
            Self::DiagnosticUsb2Compliance => "diag-usb2-compliance",
            Self::DiagnosticDebugTargetVendorDefined => "diag-debug-target-vendor",
            Self::DiagnosticGnuRemoteDebug => "diag-gnu-remote-debug",
            Self::DiagnosticTraceDbcUndefined => "diag-trace-dbc-undef",
            Self::DiagnosticTraceDbcVendorDefined => "diag-trace-dbc-vendor",
            Self::DiagnosticDfxDbcUndefined => "diag-dfx-dbc-undef",
            Self::DiagnosticDfxDbcVendorDefined => "diag-dfx-dbc-vendor",
            Self::DiagnosticTraceDvcGpVendorDefined => "diag-trace-dvc-gp-vendor",
            Self::DiagnosticTraceDvcGpGnu => "diag-trace-dvc-gp-gnu",
            Self::DiagnosticDfxDvcUndefined => "diag-dfx-dvc-undef",
            Self::DiagnosticDfxDvcVendorDefined => "diag-dfx-dvc-vendor",
            Self::DiagnosticTraceDvcUndefined => "diag-trace-dvc-undef",
            Self::DiagnosticTraceDvcVendorDefined => "diag-trace-dvc-vendor",
            Self::DiagnosticReserved08 => "diag-reserved-08",
            Self::WirelessBluetooth => "bt",
            Self::WirelessUwb => "uwb",
            Self::WirelessRemoteNdis => "remote-ndis",
            Self::WirelessBluetoothAmp => "bt-amp",
            Self::WirelessHostWireAdapter => "hwa-host",
            Self::WirelessDeviceWireAdapter => "hwa-device",
            Self::WirelessDeviceWireAdapterIsochronous => "hwa-device-iso",
            Self::MiscActiveSync => "activesync",
            Self::MiscPalmSync => "palmsync",
            Self::MiscInterfaceAssociationDescriptor => "iad",
            Self::MiscWireAdapterMultifunctionPeripheral => "wire-adapter-mfp",
            Self::MiscCableBasedAssociationFramework => "cbaf",
            Self::MiscRndisEthernet => "rndis-eth",
            Self::MiscRndisWifi => "rndis-wifi",
            Self::MiscRndisWimax => "rndis-wimax",
            Self::MiscRndisWwan => "rndis-wwan",
            Self::MiscRndisRawIpv4 => "rndis-ipv4",
            Self::MiscRndisRawIpv6 => "rndis-ipv6",
            Self::MiscRndisGprs => "rndis-gprs",
            Self::MiscUsb3VisionControl => "usb3-vision-control",
            Self::MiscUsb3VisionEvent => "usb3-vision-event",
            Self::MiscUsb3VisionStreaming => "usb3-vision-stream",
            Self::MiscStep => "step",
            Self::MiscStepRaw => "step-raw",
            Self::MiscCommandInterfaceIad => "command-iad",
            Self::MiscCommandInterfaceDescriptor => "command-iface",
            Self::MiscMediaInterfaceDescriptor => "media-iface",
            Self::MiscOcpSecureFirmwareRecovery => "ocp-secure-fw-recovery",
            Self::MiscOcpObmfIcp => "ocp-obmf-icp",
            Self::AppDeviceFirmwareUpgrade => "dfu",
            Self::AppIrdaBridge => "irda-bridge",
            Self::AppUsbTestMeasurement => "usbtmc",
            Self::AppUsbTestMeasurementUsb488 => "usbtmc-usb488",
            Self::VendorSpecific => "vendor",
            Self::Unclassified { .. } => "unclassified",
        }
    }

    pub(crate) const fn description(self) -> &'static str {
        match self {
            Self::PerInterface => "Use class code info from Interface Descriptors",
            Self::AudioGeneric => "Audio device",
            Self::CommunicationsGeneric => "Communication device class",
            Self::HidGeneric => "HID device class",
            Self::PhysicalGeneric => "Physical device class",
            Self::StillImaging => "Still Imaging device",
            Self::PrinterGeneric => "Printer device",
            Self::MassStorageGeneric => "Mass Storage device",
            Self::FullSpeedHub => "Full speed Hub",
            Self::HighSpeedHubSingleTt => "Hi-speed hub with single TT",
            Self::HighSpeedHubMultiTt => "Hi-speed hub with multiple TTs",
            Self::CdcDataGeneric => "CDC data device",
            Self::SmartCardGeneric => "Smart Card device",
            Self::ContentSecurity => "Content Security device",
            Self::VideoGeneric => "Video device",
            Self::PersonalHealthcareGeneric => "Personal Healthcare device",
            Self::AudioVideoControl => "Audio/Video Device - AVControl Interface",
            Self::AudioVideoVideoStreaming => {
                "Audio/Video Device - AVData Video Streaming Interface"
            }
            Self::AudioVideoAudioStreaming => {
                "Audio/Video Device - AVData Audio Streaming Interface"
            }
            Self::Billboard => "Billboard Device",
            Self::TypeCBridge => "USB Type-C Bridge Device",
            Self::BulkDisplay => "USB BDP Device",
            Self::MctpEndpointV1 => {
                "MCTP 1.x Management-controller and Managed-Device endpoints"
            }
            Self::MctpEndpointV2 => {
                "MCTP 2.x Management-controller and Managed-Device endpoints"
            }
            Self::MctpHostInterfaceV1 => "MCTP 1.x Host Interface endpoint",
            Self::MctpHostInterfaceV2 => "MCTP 2.x Host Interface endpoint",
            Self::I3c => "I3C Device",
            Self::DiagnosticUsb2Compliance => "USB2 Compliance Device",
            Self::DiagnosticDebugTargetVendorDefined => "Debug Target vendor defined",
            Self::DiagnosticGnuRemoteDebug => "GNU Remote Debug Command Set",
            Self::DiagnosticTraceDbcUndefined => "Undefined",
            Self::DiagnosticTraceDbcVendorDefined => "Vendor defined Trace protocol on DbC",
            Self::DiagnosticDfxDbcUndefined => "Undefined",
            Self::DiagnosticDfxDbcVendorDefined => "Vendor defined Dfx protocol on DbC",
            Self::DiagnosticTraceDvcGpVendorDefined => {
                "Vendor defined Trace protocol over GP endpoint on DvC"
            }
            Self::DiagnosticTraceDvcGpGnu => "GNU Protocol over GP endpoint on DvC",
            Self::DiagnosticDfxDvcUndefined => "Undefined",
            Self::DiagnosticDfxDvcVendorDefined => "Vendor defined Dfx protocol on DvC",
            Self::DiagnosticTraceDvcUndefined => "Undefined",
            Self::DiagnosticTraceDvcVendorDefined => "Vendor defined Trace protocol on DvC",
            Self::DiagnosticReserved08 => "Undefined",
            Self::WirelessBluetooth => "Bluetooth Programming Interface",
            Self::WirelessUwb => "UWB Radio Control Interface",
            Self::WirelessRemoteNdis => "Remote NDIS",
            Self::WirelessBluetoothAmp => "Bluetooth AMP Controller",
            Self::WirelessHostWireAdapter => "Host Wire Adapter Control/Data interface",
            Self::WirelessDeviceWireAdapter => "Device Wire Adapter Control/Data interface",
            Self::WirelessDeviceWireAdapterIsochronous => {
                "Device Wire Adapter Isochronous interface"
            }
            Self::MiscActiveSync => "Active Sync device",
            Self::MiscPalmSync => "Palm Sync",
            Self::MiscInterfaceAssociationDescriptor => "Interface Association Descriptor",
            Self::MiscWireAdapterMultifunctionPeripheral => {
                "Wire Adapter Multifunction Peripheral programming interface"
            }
            Self::MiscCableBasedAssociationFramework => "Cable Based Association Framework",
            Self::MiscRndisEthernet => "RNDIS over Ethernet",
            Self::MiscRndisWifi => "RNDIS over WiFi",
            Self::MiscRndisWimax => "RNDIS over WiMAX",
            Self::MiscRndisWwan => "RNDIS over WWAN",
            Self::MiscRndisRawIpv4 => "RNDIS for Raw IPv4",
            Self::MiscRndisRawIpv6 => "RNDIS for Raw IPv6",
            Self::MiscRndisGprs => "RNDIS for GPRS",
            Self::MiscUsb3VisionControl => "USB3 Vision Control Interface",
            Self::MiscUsb3VisionEvent => "USB3 Vision Event Interface",
            Self::MiscUsb3VisionStreaming => "USB3 Vision Streaming Interface",
            Self::MiscStep => "STEP",
            Self::MiscStepRaw => "STEP RAW",
            Self::MiscCommandInterfaceIad => "Command Interface in IAD",
            Self::MiscCommandInterfaceDescriptor => "Command Interface in Interface Descriptor",
            Self::MiscMediaInterfaceDescriptor => "Media Interface in Interface Descriptor",
            Self::MiscOcpSecureFirmwareRecovery => "OCP Secure Firmware Recovery",
            Self::MiscOcpObmfIcp => "OCP OBMF-ICP",
            Self::AppDeviceFirmwareUpgrade => "Device Firmware Upgrade",
            Self::AppIrdaBridge => "IRDA Bridge device",
            Self::AppUsbTestMeasurement => "USB Test and Measurement Device",
            Self::AppUsbTestMeasurementUsb488 => {
                "USB Test and Measurement Device conforming to USB488"
            }
            Self::VendorSpecific => "Vendor specific",
            Self::Unclassified { .. } => "No explicit class triple meaning registered",
        }
    }
}
