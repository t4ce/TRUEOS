//! Read-only BSP display metrics gathered from the boot framebuffer EDID.
//!
//! Limine obtains this block from the active firmware display path before the
//! kernel takes over scanout. Consuming that copy is intentionally preferred
//! to issuing a second DDC/AUX transaction against a link which is already
//! live.

use super::DisplayPipelineTarget;

const EDID_BASE_BLOCK_LEN: usize = 128;
const EDID_HEADER: [u8; 8] = [0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x00];
const EDID_DTD_OFFSET: usize = 54;
const EDID_DTD_LEN: usize = 18;
const EDID_DTD_COUNT: usize = 4;

#[derive(Copy, Clone, Debug)]
enum EdidParseError {
    ShortBaseBlock,
    BadHeader,
    BadChecksum,
}

impl EdidParseError {
    const fn name(self) -> &'static str {
        match self {
            Self::ShortBaseBlock => "short-base-block",
            Self::BadHeader => "bad-header",
            Self::BadChecksum => "bad-base-checksum",
        }
    }
}

#[derive(Copy, Clone)]
enum PhysicalSizeSource {
    DetailedTiming1,
    DetailedTiming2,
    DetailedTiming3,
    DetailedTiming4,
    BaseBlockCentimeters,
}

impl PhysicalSizeSource {
    const fn detailed_timing(index: usize) -> Self {
        match index {
            0 => Self::DetailedTiming1,
            1 => Self::DetailedTiming2,
            2 => Self::DetailedTiming3,
            _ => Self::DetailedTiming4,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::DetailedTiming1 => "detailed-timing-1",
            Self::DetailedTiming2 => "detailed-timing-2",
            Self::DetailedTiming3 => "detailed-timing-3",
            Self::DetailedTiming4 => "detailed-timing-4",
            Self::BaseBlockCentimeters => "base-block-centimeters",
        }
    }
}

#[derive(Copy, Clone)]
struct PhysicalSize {
    width_mm: u16,
    height_mm: u16,
    source: PhysicalSizeSource,
}

#[derive(Copy, Clone)]
struct EdidText {
    bytes: [u8; 13],
    len: u8,
}

impl EdidText {
    const fn empty() -> Self {
        Self {
            bytes: [0; 13],
            len: 0,
        }
    }

    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..usize::from(self.len)]).unwrap_or("-")
    }
}

#[derive(Copy, Clone)]
struct EdidMonitorInfo {
    manufacturer: [u8; 3],
    product_code: u16,
    serial: u32,
    manufacture_week: u8,
    manufacture_year: u16,
    version: u8,
    revision: u8,
    digital_input: bool,
    extension_count: u8,
    base_width_cm: u8,
    base_height_cm: u8,
    name: EdidText,
    preferred_width: u16,
    preferred_height: u16,
    physical: Option<PhysicalSize>,
}

impl EdidMonitorInfo {
    fn manufacturer_str(&self) -> &str {
        core::str::from_utf8(&self.manufacturer).unwrap_or("???")
    }
}

#[derive(Copy, Clone)]
struct AxisDensity {
    centi_dpi: u32,
    centi_pixels_per_cm: u32,
    pixels_per_cm: u32,
}

pub(super) fn log_bsp_display_metrics_probe(active_target: Option<DisplayPipelineTarget>) {
    let Some(response) = crate::limine::framebuffer_response() else {
        log_probe_unavailable(active_target, "no-limine-framebuffer-response");
        return;
    };
    let framebuffers = response.framebuffers();
    if framebuffers.is_empty() {
        log_probe_unavailable(active_target, "no-limine-framebuffers");
        return;
    }

    for (index, framebuffer) in framebuffers.iter().copied().enumerate() {
        let association = if index == 0 {
            "provisional-primary-output"
        } else {
            "unmapped-boot-framebuffer"
        };
        let route_target = (index == 0).then_some(active_target).flatten();
        let (mode_width, mode_height, mode_source) = route_target
            .map(|target| (target.width, target.height, "active-pipeline"))
            .unwrap_or((framebuffer.width as u32, framebuffer.height as u32, "boot-framebuffer"));
        let pipeline = route_target
            .map(|target| target.pipeline.name())
            .unwrap_or("unmapped");
        let ddi = route_target
            .map(|target| target.route.ddi.name())
            .unwrap_or("unresolved");
        let edid = framebuffer.edid();

        if edid.is_empty() {
            crate::log_info!(target: "intel/display";
                "intel/display: bsp-display-metrics source=limine-framebuffer-edid status=unavailable reason=firmware-no-active-edid framebuffer={} framebuffer_mode={}x{} pipeline={} ddi={} association={} physical_mm=unknown dpi=unknown pixels_per_cm=unknown\n",
                index,
                framebuffer.width,
                framebuffer.height,
                pipeline,
                ddi,
                association,
            );
            continue;
        }

        let info = match parse_edid_base_block(edid) {
            Ok(info) => info,
            Err(error) => {
                crate::log_warn!(target: "intel/display";
                    "intel/display: bsp-display-metrics source=limine-framebuffer-edid status=invalid reason={} framebuffer={} edid_bytes={} captured_blocks={} framebuffer_mode={}x{} pipeline={} ddi={} association={} physical_mm=unknown dpi=unknown pixels_per_cm=unknown\n",
                    error.name(),
                    index,
                    edid.len(),
                    edid.len() / EDID_BASE_BLOCK_LEN,
                    framebuffer.width,
                    framebuffer.height,
                    pipeline,
                    ddi,
                    association,
                );
                continue;
            }
        };

        let declared_blocks = usize::from(info.extension_count).saturating_add(1);
        let captured_blocks = edid.len() / EDID_BASE_BLOCK_LEN;
        let extension_capture = if captured_blocks >= declared_blocks {
            "complete"
        } else {
            "base-only-truncated"
        };
        let input = if info.digital_input {
            "digital"
        } else {
            "analog"
        };

        let Some(physical) = info.physical else {
            crate::log_info!(target: "intel/display";
                "intel/display: bsp-display-metrics source=limine-framebuffer-edid status=valid-no-physical-size framebuffer={} edid_bytes={} captured_blocks={} declared_blocks={} extension_capture={} checksum=ok edid={}.{} input={} manufacturer={} product=0x{:04X} serial=0x{:08X} manufactured={}/{} name=\"{}\" base_size_cm={}x{} preferred_mode={}x{} density_mode={}x{} density_mode_source={} pipeline={} ddi={} association={} physical_mm=unknown dpi=unknown pixels_per_cm=unknown\n",
                index,
                edid.len(),
                captured_blocks,
                declared_blocks,
                extension_capture,
                info.version,
                info.revision,
                input,
                info.manufacturer_str(),
                info.product_code,
                info.serial,
                info.manufacture_week,
                info.manufacture_year,
                info.name.as_str(),
                info.base_width_cm,
                info.base_height_cm,
                info.preferred_width,
                info.preferred_height,
                mode_width,
                mode_height,
                mode_source,
                pipeline,
                ddi,
                association,
            );
            continue;
        };

        let density_x = axis_density(mode_width, physical.width_mm);
        let density_y = axis_density(mode_height, physical.height_mm);
        crate::log_info!(target: "intel/display";
            "intel/display: bsp-display-metrics source=limine-framebuffer-edid status=valid framebuffer={} edid_bytes={} captured_blocks={} declared_blocks={} extension_capture={} checksum=ok edid={}.{} input={} manufacturer={} product=0x{:04X} serial=0x{:08X} manufactured={}/{} name=\"{}\" base_size_cm={}x{} physical_mm={}x{} physical_source={} preferred_mode={}x{} density_mode={}x{} density_mode_source={} dpi={}.{:02}x{}.{:02} pixels_per_cm={}.{:02}x{}.{:02} one_cm_pixels={}x{} pipeline={} ddi={} association={} connector=unresolved\n",
            index,
            edid.len(),
            captured_blocks,
            declared_blocks,
            extension_capture,
            info.version,
            info.revision,
            input,
            info.manufacturer_str(),
            info.product_code,
            info.serial,
            info.manufacture_week,
            info.manufacture_year,
            info.name.as_str(),
            info.base_width_cm,
            info.base_height_cm,
            physical.width_mm,
            physical.height_mm,
            physical.source.name(),
            info.preferred_width,
            info.preferred_height,
            mode_width,
            mode_height,
            mode_source,
            density_x.centi_dpi / 100,
            density_x.centi_dpi % 100,
            density_y.centi_dpi / 100,
            density_y.centi_dpi % 100,
            density_x.centi_pixels_per_cm / 100,
            density_x.centi_pixels_per_cm % 100,
            density_y.centi_pixels_per_cm / 100,
            density_y.centi_pixels_per_cm % 100,
            density_x.pixels_per_cm,
            density_y.pixels_per_cm,
            pipeline,
            ddi,
            association,
        );
    }
}

/// Convert a physical rectangle into pixels using the validated boot EDID and
/// the currently active display mode. This keeps physical sizing policy in
/// display plumbing while allowing consumers such as GridPaper to request an
/// A4-sized surface without parsing monitor data themselves.
pub(super) fn physical_extent_pixels(
    active_target: DisplayPipelineTarget,
    width_mm: u32,
    height_mm: u32,
) -> Option<(u32, u32)> {
    if width_mm == 0 || height_mm == 0 {
        return None;
    }
    let framebuffer = crate::limine::framebuffer_response()?
        .framebuffers()
        .first()
        .copied()?;
    let info = parse_edid_base_block(framebuffer.edid()).ok()?;
    let physical = info.physical?;
    let width = rounded_ratio(
        u64::from(active_target.width).checked_mul(u64::from(width_mm))?,
        u64::from(physical.width_mm),
    );
    let height = rounded_ratio(
        u64::from(active_target.height).checked_mul(u64::from(height_mm))?,
        u64::from(physical.height_mm),
    );
    (width != 0 && height != 0).then_some((width, height))
}

fn log_probe_unavailable(active_target: Option<DisplayPipelineTarget>, reason: &'static str) {
    let pipeline = active_target
        .map(|target| target.pipeline.name())
        .unwrap_or("unresolved");
    let ddi = active_target
        .map(|target| target.route.ddi.name())
        .unwrap_or("unresolved");
    crate::log_info!(target: "intel/display";
        "intel/display: bsp-display-metrics source=limine-framebuffer-edid status=unavailable reason={} pipeline={} ddi={} physical_mm=unknown dpi=unknown pixels_per_cm=unknown\n",
        reason,
        pipeline,
        ddi,
    );
}

fn parse_edid_base_block(edid: &[u8]) -> Result<EdidMonitorInfo, EdidParseError> {
    if edid.len() < EDID_BASE_BLOCK_LEN {
        return Err(EdidParseError::ShortBaseBlock);
    }
    let base = &edid[..EDID_BASE_BLOCK_LEN];
    if base[..EDID_HEADER.len()] != EDID_HEADER {
        return Err(EdidParseError::BadHeader);
    }
    if base
        .iter()
        .copied()
        .fold(0u8, |sum, byte| sum.wrapping_add(byte))
        != 0
    {
        return Err(EdidParseError::BadChecksum);
    }

    let manufacturer_code = u16::from_be_bytes([base[8], base[9]]);
    let manufacturer = [
        edid_manufacturer_letter((manufacturer_code >> 10) & 0x1F),
        edid_manufacturer_letter((manufacturer_code >> 5) & 0x1F),
        edid_manufacturer_letter(manufacturer_code & 0x1F),
    ];
    let mut name = EdidText::empty();
    let mut preferred_width = 0;
    let mut preferred_height = 0;
    let mut detailed_physical = None;

    for index in 0..EDID_DTD_COUNT {
        let offset = EDID_DTD_OFFSET + index * EDID_DTD_LEN;
        let descriptor = &base[offset..offset + EDID_DTD_LEN];
        if descriptor[0] != 0 || descriptor[1] != 0 {
            let width = u16::from(descriptor[2]) | (u16::from(descriptor[4] & 0xF0) << 4);
            let height = u16::from(descriptor[5]) | (u16::from(descriptor[7] & 0xF0) << 4);
            if preferred_width == 0 && preferred_height == 0 {
                preferred_width = width;
                preferred_height = height;
            }
            let width_mm = u16::from(descriptor[12]) | (u16::from(descriptor[14] & 0xF0) << 4);
            let height_mm = u16::from(descriptor[13]) | (u16::from(descriptor[14] & 0x0F) << 8);
            if detailed_physical.is_none() && width_mm != 0 && height_mm != 0 {
                detailed_physical = Some(PhysicalSize {
                    width_mm,
                    height_mm,
                    source: PhysicalSizeSource::detailed_timing(index),
                });
            }
        } else if descriptor[3] == 0xFC {
            name = parse_edid_text(&descriptor[5..18]);
        }
    }

    let base_width_cm = base[21];
    let base_height_cm = base[22];
    let physical = detailed_physical.or_else(|| {
        (base_width_cm != 0 && base_height_cm != 0).then_some(PhysicalSize {
            width_mm: u16::from(base_width_cm) * 10,
            height_mm: u16::from(base_height_cm) * 10,
            source: PhysicalSizeSource::BaseBlockCentimeters,
        })
    });

    Ok(EdidMonitorInfo {
        manufacturer,
        product_code: u16::from_le_bytes([base[10], base[11]]),
        serial: u32::from_le_bytes([base[12], base[13], base[14], base[15]]),
        manufacture_week: base[16],
        manufacture_year: 1990u16.saturating_add(u16::from(base[17])),
        version: base[18],
        revision: base[19],
        digital_input: (base[20] & 0x80) != 0,
        extension_count: base[126],
        base_width_cm,
        base_height_cm,
        name,
        preferred_width,
        preferred_height,
        physical,
    })
}

fn edid_manufacturer_letter(value: u16) -> u8 {
    if (1..=26).contains(&value) {
        b'A' + value as u8 - 1
    } else {
        b'?'
    }
}

fn parse_edid_text(raw: &[u8]) -> EdidText {
    let mut text = EdidText::empty();
    for &byte in raw.iter().take(text.bytes.len()) {
        if matches!(byte, 0 | b'\n' | b'\r') {
            break;
        }
        let sanitized = if byte.is_ascii_graphic() || byte == b' ' {
            byte
        } else {
            b'?'
        };
        text.bytes[usize::from(text.len)] = sanitized;
        text.len = text.len.saturating_add(1);
    }
    while text.len != 0 && text.bytes[usize::from(text.len - 1)] == b' ' {
        text.len -= 1;
    }
    text
}

fn axis_density(pixels: u32, millimeters: u16) -> AxisDensity {
    let millimeters = u64::from(millimeters);
    let pixels = u64::from(pixels);
    AxisDensity {
        // dpi * 100 = pixels * 25.4 * 100 / millimeters.
        centi_dpi: rounded_ratio(pixels.saturating_mul(2540), millimeters),
        // pixels/cm * 100 = pixels * 10 * 100 / millimeters.
        centi_pixels_per_cm: rounded_ratio(pixels.saturating_mul(1000), millimeters),
        pixels_per_cm: rounded_ratio(pixels.saturating_mul(10), millimeters),
    }
}

fn rounded_ratio(numerator: u64, denominator: u64) -> u32 {
    if denominator == 0 {
        return 0;
    }
    u32::try_from(numerator.saturating_add(denominator / 2) / denominator).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_edid() -> [u8; EDID_BASE_BLOCK_LEN] {
        let mut edid = [0u8; EDID_BASE_BLOCK_LEN];
        edid[..EDID_HEADER.len()].copy_from_slice(&EDID_HEADER);
        let manufacturer = (4u16 << 10) | (5u16 << 5) | 12u16; // DEL
        edid[8..10].copy_from_slice(&manufacturer.to_be_bytes());
        edid[10..12].copy_from_slice(&0xA123u16.to_le_bytes());
        edid[12..16].copy_from_slice(&0x0102_0304u32.to_le_bytes());
        edid[16] = 7;
        edid[17] = 34;
        edid[18] = 1;
        edid[19] = 4;
        edid[20] = 0x80;
        edid[21] = 60;
        edid[22] = 34;

        let dtd = &mut edid[EDID_DTD_OFFSET..EDID_DTD_OFFSET + EDID_DTD_LEN];
        dtd[0] = 1;
        dtd[2] = 0x00;
        dtd[4] = 0xA0; // 2560 active pixels
        dtd[5] = 0xA0;
        dtd[7] = 0x50; // 1440 active pixels
        dtd[12] = 0x58;
        dtd[13] = 0x54;
        dtd[14] = 0x21; // 600 x 340 mm

        let checksum = edid[..127]
            .iter()
            .copied()
            .fold(0u8, |sum, byte| sum.wrapping_add(byte));
        edid[127] = 0u8.wrapping_sub(checksum);
        edid
    }

    #[test]
    fn parses_physical_size_and_density_without_floating_point() {
        let edid = sample_edid();
        let info = parse_edid_base_block(&edid).expect("valid EDID");
        let physical = info.physical.expect("physical size");
        assert_eq!(info.manufacturer_str(), "DEL");
        assert_eq!((info.preferred_width, info.preferred_height), (2560, 1440));
        assert_eq!((physical.width_mm, physical.height_mm), (600, 340));
        let density = axis_density(2560, physical.width_mm);
        assert_eq!(density.centi_dpi, 10_837);
        assert_eq!(density.centi_pixels_per_cm, 4_267);
        assert_eq!(density.pixels_per_cm, 43);
    }

    #[test]
    fn rejects_a_corrupt_base_block() {
        let mut edid = sample_edid();
        edid[42] ^= 1;
        assert!(matches!(parse_edid_base_block(&edid), Err(EdidParseError::BadChecksum)));
    }
}
