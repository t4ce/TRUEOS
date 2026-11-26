//! Native Gen12 state for the admitted bfly isoline patch stages.

use super::{
    DispatchMode, ShaderKernelMetadata, TrianglePipeline, TrianglePixelShader,
    TrianglePixelShaderMetadata, TriangleVertexShader, TriangleVertexShaderMetadata,
};

pub(crate) const PANEL_SIDE_INSTANCES: u32 = 16;
const FLOCK_SIDES_PER_BUTTERFLY: u32 = 2;
const FLOCK_BUTTERFLIES_MIN: u32 = 8;
const FLOCK_BUTTERFLIES_MAX: u32 = 80;
const VS_BYTES: usize = 168;
const WHITE_PS_BYTES: usize = 48;
const RGB_PS_BYTES: usize = 72;
const SMOKE_PS_BYTES: usize = 264;
const FILLED_PS_BYTES: usize = 72;
const BREED_FILL_PS_BYTES: usize = 112;
const TRANSLUCENT_FILL_PS_BYTES: usize = 120;
const TWILIGHT_RIBBON_PS_BYTES: usize = 672;
const SUNRISE_RIBBON_PS_BYTES: usize = 3624;
const PROJECTED_WIDTHS_PACKAGE_ID: u32 = 30;
const INSET_OUTLINE_PACKAGE_ID: u32 = 33;
const BREED_FILL_PACKAGE_ID: u32 = 34;
const TRANSLUCENT_FILL_PACKAGE_ID: u32 = 36;
pub(crate) const TWILIGHT_RIBBON_PACKAGE_ID: u32 = 37;
pub(crate) const SUNRISE_RIBBON_PACKAGE_ID: u32 = 38;
const SUNRISE_OUTLINE_PACKAGE_ID: u32 = 39;
const NEUTRAL_MIXED_OUTLINE_PACKAGE_ID: u32 = 41;
const NEUTRAL_INSET_OUTLINE_PACKAGE_ID: u32 = 42;
const NEUTRAL_WAKE_OUTLINE_PACKAGE_ID: u32 = 43;

pub(crate) const SGVS: u32 = 0xe001_4001;
pub(crate) const SGVS_2: [u32; 2] = [0xb001_0001, 2];
pub(crate) const VF_COMPONENT_PACKING: u32 = 0xa7;
pub(crate) const VS_URB_OUTPUT_LENGTH: u8 = 1;
pub(crate) const VS_URB_READ_LENGTH: u8 = 1;

// Mesa ANV 26.0.8, ADL-S 4680 rev 0C, indexed PATCHLIST_4 with 16 instances.
// KSP slots are relocated relative to the renderer's instruction base.
const HS: [u32; 9] = [
    0x781b0007, 0x00080000, 0xa0014f03, 0, 0, 0, 0, 0x01340000, 0,
];
pub(crate) const HULL_STATE_DWORDS: usize = HS.len();
const TE: [u32; 5] = [0x781c0003, 0x00800121, 0x427c0000, 0x42800000, 0];
const FILLED_TE: [u32; 5] = [0x781c0003, 0x00800301, 0x427c0000, 0x42800000, 0];
// Size in 64-byte units, start in 8 KiB units, entry count.
const URB: [(u32, u32, u32); 4] =
    [(1, 4, 2304), (3, 22, 1024), (1, 46, 2304), (1, 4, 0)];
const RIBBON_URB: [(u32, u32, u32); 4] =
    [(1, 4, 2048), (4, 20, 896), (1, 48, 2048), (1, 4, 0)];
const WAKE_BLEND_URB: [(u32, u32, u32); 4] =
    [(1, 4, 1664), (4, 17, 704), (2, 39, 1600), (1, 4, 0)];
const CLIP: [u32; 3] = [0x00040400, 0xc4000001, 0x0003ffe0];
const FILLED_CLIP: [u32; 3] = [0x00040400, 0xd4000001, 0x0003ffe0];
const RASTER: u32 = 0x04210003;
const FILLED_RASTER: u32 = 0x04a11003;
const WM: u32 = 0x80000040;
const RIBBON_WM: u32 = 0x80000840;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum RibbonTexture {
    TwilightR8,
    SunriseRgba8,
}

impl RibbonTexture {
    pub(crate) const fn package_id(self) -> u32 {
        match self {
            Self::TwilightR8 => TWILIGHT_RIBBON_PACKAGE_ID,
            Self::SunriseRgba8 => SUNRISE_RIBBON_PACKAGE_ID,
        }
    }

    pub(crate) const fn width(self) -> u32 {
        256
    }

    pub(crate) const fn height(self) -> u32 {
        match self {
            Self::TwilightR8 => 256,
            Self::SunriseRgba8 => 768,
        }
    }

    pub(crate) const fn pitch(self) -> u32 {
        self.width() * match self {
            Self::TwilightR8 => 1,
            Self::SunriseRgba8 => 4,
        }
    }

    pub(crate) const fn byte_len(self) -> usize {
        (self.pitch() * self.height()) as usize
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::TwilightR8 => "twilight R8",
            Self::SunriseRgba8 => "wake RGBA8 atlas",
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum Geometry {
    Closed,
    Open,
    Flock,
    GrowingFlock,
    SwappingFlock,
    SmokeFlock,
    MixedFlock,
    FilledFlock,
    InsetFillFlock,
    InsetOutlineFlock,
    TwilightRibbon,
    SunriseRibbon,
    SunriseOutline,
}

impl Geometry {
    const fn patch_tags(self) -> &'static [f32] {
        match self {
            Self::Closed => &[0.0, 0.0, 1.0, 0.0, 1.0],
            Self::Open
            | Self::Flock
            | Self::GrowingFlock
            | Self::SwappingFlock
            | Self::SmokeFlock => &[0.0, 0.0, 0.0, 1.0],
            Self::MixedFlock | Self::TwilightRibbon => {
                &[0.0, 0.0, 0.0, 1.0, 2.0, 2.0, 2.0, 2.0, 2.0, 3.0]
            }
            Self::FilledFlock => &[0.0, 0.0, 1.0, 2.0, 3.0, 4.0, 4.0, 4.0, 5.0, 6.0, 6.0, 7.0],
            Self::InsetFillFlock | Self::InsetOutlineFlock => {
                &[0.0, 0.0, 0.0, 1.0, 4.0, 4.0, 4.0, 4.0, 4.0, 5.0]
            }
            Self::SunriseRibbon | Self::SunriseOutline => {
                &[0.0, 8.0, 16.0, 1.0, 4.0, 12.0, 20.0, 28.0, 36.0, 5.0]
            }
        }
    }

    pub(crate) const fn control_vertices(self) -> usize {
        self.patch_tags().len() * 4
    }

    pub(crate) const fn is_filled(self) -> bool {
        matches!(self, Self::FilledFlock | Self::InsetFillFlock) || self.ribbon_texture().is_some()
    }

    pub(crate) const fn ribbon_texture(self) -> Option<RibbonTexture> {
        match self {
            Self::TwilightRibbon => Some(RibbonTexture::TwilightR8),
            Self::SunriseRibbon => Some(RibbonTexture::SunriseRgba8),
            _ => None,
        }
    }

    pub(crate) fn instance_count(self, butterfly_count: f32) -> Option<u32> {
        if !matches!(
            self,
            Self::Flock
                | Self::GrowingFlock
                | Self::SwappingFlock
                | Self::SmokeFlock
                | Self::MixedFlock
                | Self::FilledFlock
                | Self::InsetFillFlock
                | Self::InsetOutlineFlock
                | Self::TwilightRibbon
                | Self::SunriseRibbon
                | Self::SunriseOutline
        ) {
            return Some(PANEL_SIDE_INSTANCES);
        }
        let count = butterfly_count as u32;
        ((FLOCK_BUTTERFLIES_MIN..=FLOCK_BUTTERFLIES_MAX).contains(&count)
            && butterfly_count == count as f32)
            .then(|| count * FLOCK_SIDES_PER_BUTTERFLY)
    }

    pub(crate) fn valid_vertices(self, vertices: &[u8], indices: &[u32]) -> bool {
        let count = self.control_vertices();
        vertices.len() == count * 12
            && indices.len() == count
            && indices
                .iter()
                .copied()
                .enumerate()
                .all(|(i, idx)| i == idx as usize)
            && vertices
                .chunks_exact(48)
                .zip(self.patch_tags())
                .all(|(patch, &tag)| {
                    patch.chunks_exact(12).all(|point| {
                        let x = f32::from_le_bytes(point[0..4].try_into().unwrap());
                        let y = f32::from_le_bytes(point[4..8].try_into().unwrap());
                        let edge = f32::from_le_bytes(point[8..12].try_into().unwrap());
                        x.is_finite()
                            && y.is_finite()
                            && x.abs() <= 2.0
                            && y.abs() <= 2.0
                            && edge == tag
                    })
                })
    }
}

pub(crate) const LAYERED_CONTROL_VERTICES: usize =
    Geometry::InsetFillFlock.control_vertices() + Geometry::MixedFlock.control_vertices();
pub(crate) const LAYERED_WAKE_CONTROL_VERTICES: usize =
    LAYERED_CONTROL_VERTICES + Geometry::SunriseRibbon.control_vertices();

pub(crate) fn valid_layered_vertices(vertices: &[u8], indices: &[u32]) -> bool {
    let inset_count = Geometry::InsetFillFlock.control_vertices();
    let Some((inset, original)) = vertices.split_at_checked(inset_count * 12) else {
        return false;
    };
    let Some((inset_indices, original_indices)) = indices.split_at_checked(inset_count) else {
        return false;
    };
    Geometry::InsetFillFlock.valid_vertices(inset, inset_indices)
        && Geometry::MixedFlock.valid_vertices(original, original_indices)
}

pub(crate) fn valid_layered_wake_vertices(vertices: &[u8], indices: &[u32]) -> bool {
    let Some((layered, wake)) = vertices.split_at_checked(LAYERED_CONTROL_VERTICES * 12) else {
        return false;
    };
    let Some((layered_indices, wake_indices)) = indices.split_at_checked(LAYERED_CONTROL_VERTICES) else {
        return false;
    };
    valid_layered_vertices(layered, layered_indices)
        && Geometry::SunriseRibbon.valid_vertices(wake, wake_indices)
}

#[cfg(test)]
mod layered_wake_tests {
    use super::*;
    use alloc::vec::Vec;

    fn layered_wake_fixture() -> (Vec<u8>, Vec<u32>) {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        for geometry in [Geometry::InsetFillFlock, Geometry::MixedFlock, Geometry::SunriseRibbon] {
            for tag in geometry.patch_tags() {
                for _ in 0..4 {
                    vertices.extend_from_slice(&[0; 8]);
                    vertices.extend_from_slice(&tag.to_le_bytes());
                }
            }
            indices.extend(0..geometry.control_vertices() as u32);
        }
        (vertices, indices)
    }

    #[test]
    fn layered_wake_requires_three_locally_indexed_control_sections() {
        let (vertices, mut indices) = layered_wake_fixture();
        assert_eq!(indices.len(), LAYERED_WAKE_CONTROL_VERTICES);
        assert!(valid_layered_wake_vertices(&vertices, &indices));
        assert!(!valid_layered_vertices(&vertices, &indices));
        assert!(!valid_layered_wake_vertices(&vertices, &indices[..LAYERED_CONTROL_VERTICES]));
        indices[LAYERED_CONTROL_VERTICES] = LAYERED_CONTROL_VERTICES as u32;
        assert!(!valid_layered_wake_vertices(&vertices, &indices));
    }

    #[test]
    fn layered_wake_requires_ribbon_curve_tags_in_its_last_section() {
        let (mut vertices, indices) = layered_wake_fixture();
        let second_curve_tag = (LAYERED_CONTROL_VERTICES + 4) * 12 + 8;
        vertices[second_curve_tag..second_curve_tag + 4].copy_from_slice(&0.0f32.to_le_bytes());
        assert!(!valid_layered_wake_vertices(&vertices, &indices));
    }
}

struct Capture {
    package_id: u32,
    geometry: Geometry,
    hs_bytes: usize,
    ds_bytes: usize,
    ds_urb_read: u32,
    line_width_px: u32,
}

pub(crate) struct Program {
    pipeline: TrianglePipeline,
    pub(crate) tessellation_code: [&'static [u32]; 2],
    capture: &'static Capture,
}

impl Program {
    pub(crate) fn line_width_raw(&self) -> u32 {
        self.capture.line_width_px << 7
    }

    pub(crate) fn uses_projected_widths(&self) -> bool {
        matches!(
            self.capture.package_id,
            PROJECTED_WIDTHS_PACKAGE_ID
                | INSET_OUTLINE_PACKAGE_ID
                | NEUTRAL_MIXED_OUTLINE_PACKAGE_ID
                | NEUTRAL_INSET_OUTLINE_PACKAGE_ID
                | NEUTRAL_WAKE_OUTLINE_PACKAGE_ID
        )
    }

    pub(crate) fn is_filled(&self) -> bool {
        self.capture.geometry.is_filled()
    }

    pub(crate) fn ribbon_texture(&self) -> Option<RibbonTexture> {
        self.capture.geometry.ribbon_texture()
    }

    pub(crate) fn urb(&self) -> [(u32, u32, u32); 4] {
        match self.ribbon_texture() {
            Some(RibbonTexture::SunriseRgba8) => WAKE_BLEND_URB,
            Some(RibbonTexture::TwilightR8) => RIBBON_URB,
            None => URB,
        }
    }

    pub(crate) fn wm_dw1(&self) -> u32 {
        if self.ribbon_texture().is_some() { RIBBON_WM } else { WM }
    }

    pub(crate) fn clip_state(&self) -> [u32; 3] {
        if self.is_filled() { FILLED_CLIP } else { CLIP }
    }

    pub(crate) fn raster_dw1(&self) -> u32 {
        if self.is_filled() { FILLED_RASTER } else { RASTER }
    }

    pub(crate) fn control_vertices(&self) -> usize {
        self.capture.geometry.control_vertices()
    }

    pub(crate) fn accepts_instance_count(&self, instances: u32) -> bool {
        match self.capture.geometry {
            Geometry::Flock
            | Geometry::GrowingFlock
            | Geometry::SwappingFlock
            | Geometry::SmokeFlock
            | Geometry::MixedFlock
            | Geometry::FilledFlock
            | Geometry::InsetFillFlock
            | Geometry::InsetOutlineFlock
            | Geometry::TwilightRibbon
            | Geometry::SunriseRibbon
            | Geometry::SunriseOutline => {
                instances % FLOCK_SIDES_PER_BUTTERFLY == 0
                    && (FLOCK_BUTTERFLIES_MIN..=FLOCK_BUTTERFLIES_MAX)
                        .contains(&(instances / FLOCK_SIDES_PER_BUTTERFLY))
            }
            _ => instances == PANEL_SIDE_INSTANCES,
        }
    }

    pub(crate) fn stage_packets(&self, ksp: [u32; 2]) -> Result<[u32; 25], &'static str> {
        // Disable the legacy panel planes. The viewport and scissor enforce
        // the UI4 window bounds.
        let ds = [
            0x781d0009,
            0,
            0,
            0,
            0,
            0,
            self.capture.ds_urb_read,
            0x44200609,
            0,
            0,
            0,
        ];
        let te = if self.is_filled() { FILLED_TE } else { TE };
        super::relocate_tessellation_stage_packets(HS, te, ds, ksp)
    }
}

pub(crate) fn supports_physical_device(vendor: u16, device: u16, revision: u8) -> bool {
    matches!(
        (vendor, device, revision),
        (0x8086, 0x4680, 0x0c) | (0x8086, 0xa780, 0x04)
    )
}

// New meshes prefer the latest package for their layout. Resident meshes retain their capture.
static PROGRAMS: [(Capture, spin::Once<Program>); 25] = [
    (
        Capture {
            package_id: NEUTRAL_MIXED_OUTLINE_PACKAGE_ID,
            geometry: Geometry::MixedFlock,
            hs_bytes: 27504,
            ds_bytes: 632,
            ds_urb_read: 0x0050a800,
            line_width_px: 2,
        },
        spin::Once::new(),
    ),
    (
        Capture {
            package_id: NEUTRAL_INSET_OUTLINE_PACKAGE_ID,
            geometry: Geometry::InsetOutlineFlock,
            hs_bytes: 27504,
            ds_bytes: 632,
            ds_urb_read: 0x0050a800,
            line_width_px: 2,
        },
        spin::Once::new(),
    ),
    (
        Capture {
            package_id: NEUTRAL_WAKE_OUTLINE_PACKAGE_ID,
            geometry: Geometry::SunriseOutline,
            hs_bytes: 27512,
            ds_bytes: 632,
            ds_urb_read: 0x0050a800,
            line_width_px: 2,
        },
        spin::Once::new(),
    ),
    (
        Capture {
            package_id: SUNRISE_RIBBON_PACKAGE_ID,
            geometry: Geometry::SunriseRibbon,
            hs_bytes: 29456,
            ds_bytes: 1336,
            ds_urb_read: 0x0050d800,
            line_width_px: 1,
        },
        spin::Once::new(),
    ),
    (
        Capture {
            package_id: SUNRISE_OUTLINE_PACKAGE_ID,
            geometry: Geometry::SunriseOutline,
            hs_bytes: 13928,
            ds_bytes: 784,
            ds_urb_read: 0x0050a800,
            line_width_px: 2,
        },
        spin::Once::new(),
    ),
    (
        Capture {
            package_id: TWILIGHT_RIBBON_PACKAGE_ID,
            geometry: Geometry::TwilightRibbon,
            hs_bytes: 15264,
            ds_bytes: 904,
            ds_urb_read: 0x0050c800,
            line_width_px: 1,
        },
        spin::Once::new(),
    ),
    (
        Capture {
            package_id: INSET_OUTLINE_PACKAGE_ID,
            geometry: Geometry::InsetOutlineFlock,
            hs_bytes: 13944,
            ds_bytes: 784,
            ds_urb_read: 0x0050a800,
            line_width_px: 2,
        },
        spin::Once::new(),
    ),
    (
        Capture {
            package_id: TRANSLUCENT_FILL_PACKAGE_ID,
            geometry: Geometry::InsetFillFlock,
            hs_bytes: 26912,
            ds_bytes: 624,
            ds_urb_read: 0x00509800,
            line_width_px: 1,
        },
        spin::Once::new(),
    ),
    (
        Capture {
            package_id: BREED_FILL_PACKAGE_ID,
            geometry: Geometry::InsetFillFlock,
            hs_bytes: 13088,
            ds_bytes: 624,
            ds_urb_read: 0x00509800,
            line_width_px: 1,
        },
        spin::Once::new(),
    ),
    (
        Capture {
            package_id: 32,
            geometry: Geometry::InsetFillFlock,
            hs_bytes: 12256,
            ds_bytes: 600,
            ds_urb_read: 0x00509800,
            line_width_px: 1,
        },
        spin::Once::new(),
    ),
    (
        Capture {
            package_id: 31,
            geometry: Geometry::FilledFlock,
            hs_bytes: 12256,
            ds_bytes: 856,
            ds_urb_read: 0x00509800,
            line_width_px: 1,
        },
        spin::Once::new(),
    ),
    (
        Capture {
            package_id: PROJECTED_WIDTHS_PACKAGE_ID,
            geometry: Geometry::MixedFlock,
            hs_bytes: 13944,
            ds_bytes: 784,
            ds_urb_read: 0x0050a800,
            line_width_px: 2,
        },
        spin::Once::new(),
    ),
    (
        Capture {
            package_id: 29,
            geometry: Geometry::MixedFlock,
            hs_bytes: 13888,
            ds_bytes: 784,
            ds_urb_read: 0x0050a800,
            line_width_px: 2,
        },
        spin::Once::new(),
    ),
    (
        Capture {
            package_id: 26,
            geometry: Geometry::SmokeFlock,
            hs_bytes: 13592,
            ds_bytes: 784,
            ds_urb_read: 0x0050a800,
            line_width_px: 2,
        },
        spin::Once::new(),
    ),
    (
        Capture {
            package_id: 23,
            geometry: Geometry::SwappingFlock,
            hs_bytes: 13144,
            ds_bytes: 752,
            ds_urb_read: 0x0050a800,
            line_width_px: 2,
        },
        spin::Once::new(),
    ),
    (
        Capture {
            package_id: 22,
            geometry: Geometry::SwappingFlock,
            hs_bytes: 13408,
            ds_bytes: 752,
            ds_urb_read: 0x0050a800,
            line_width_px: 2,
        },
        spin::Once::new(),
    ),
    (
        Capture {
            package_id: 21,
            geometry: Geometry::SwappingFlock,
            hs_bytes: 12992,
            ds_bytes: 752,
            ds_urb_read: 0x0050a800,
            line_width_px: 2,
        },
        spin::Once::new(),
    ),
    (
        Capture {
            package_id: 20,
            geometry: Geometry::SwappingFlock,
            hs_bytes: 12328,
            ds_bytes: 672,
            ds_urb_read: 0x0050a800,
            line_width_px: 1,
        },
        spin::Once::new(),
    ),
    (
        Capture {
            package_id: 19,
            geometry: Geometry::SwappingFlock,
            hs_bytes: 12328,
            ds_bytes: 672,
            ds_urb_read: 0x0050a800,
            line_width_px: 1,
        },
        spin::Once::new(),
    ),
    (
        Capture {
            package_id: 18,
            geometry: Geometry::GrowingFlock,
            hs_bytes: 9864,
            ds_bytes: 544,
            ds_urb_read: 0x0050a800,
            line_width_px: 1,
        },
        spin::Once::new(),
    ),
    (
        Capture {
            package_id: 17,
            geometry: Geometry::Flock,
            hs_bytes: 8984,
            ds_bytes: 544,
            ds_urb_read: 0x0050a800,
            line_width_px: 1,
        },
        spin::Once::new(),
    ),
    (
        Capture {
            package_id: 16,
            geometry: Geometry::Open,
            hs_bytes: 6944,
            ds_bytes: 544,
            ds_urb_read: 0x0050a800,
            line_width_px: 1,
        },
        spin::Once::new(),
    ),
    (
        Capture {
            package_id: 15,
            geometry: Geometry::Closed,
            hs_bytes: 6224,
            ds_bytes: 544,
            ds_urb_read: 0x0050a800,
            line_width_px: 1,
        },
        spin::Once::new(),
    ),
    (
        Capture {
            package_id: 14,
            geometry: Geometry::Closed,
            hs_bytes: 4064,
            ds_bytes: 544,
            ds_urb_read: 0x0050a800,
            line_width_px: 1,
        },
        spin::Once::new(),
    ),
    (
        Capture {
            package_id: 13,
            geometry: Geometry::Closed,
            hs_bytes: 4280,
            ds_bytes: 592,
            ds_urb_read: 0x0050b800,
            line_width_px: 1,
        },
        spin::Once::new(),
    ),
];

fn load(capture: &'static Capture, slot: &'static spin::Once<Program>) -> Option<&'static Program> {
    if let Some(program) = slot.get() {
        return Some(program);
    }
    let code = |stage| crate::gpu::app_shaders::words(capture.package_id, stage);
    let [vs, hs, ds, ps] = [code(0)?, code(1)?, code(2)?, code(3)?];
    let ribbon = capture.geometry.ribbon_texture().is_some();
    let ps_bytes = match (capture.geometry, capture.package_id) {
        (_, NEUTRAL_MIXED_OUTLINE_PACKAGE_ID | NEUTRAL_INSET_OUTLINE_PACKAGE_ID | NEUTRAL_WAKE_OUTLINE_PACKAGE_ID) => WHITE_PS_BYTES,
        (Geometry::TwilightRibbon, TWILIGHT_RIBBON_PACKAGE_ID) => TWILIGHT_RIBBON_PS_BYTES,
        (Geometry::SunriseRibbon, SUNRISE_RIBBON_PACKAGE_ID) => SUNRISE_RIBBON_PS_BYTES,
        (Geometry::InsetFillFlock, TRANSLUCENT_FILL_PACKAGE_ID) => TRANSLUCENT_FILL_PS_BYTES,
        (Geometry::InsetFillFlock, BREED_FILL_PACKAGE_ID) => BREED_FILL_PS_BYTES,
        (Geometry::SmokeFlock | Geometry::MixedFlock | Geometry::InsetOutlineFlock | Geometry::SunriseOutline, _) => {
            SMOKE_PS_BYTES
        }
        (Geometry::SwappingFlock, _) => RGB_PS_BYTES,
        (Geometry::FilledFlock | Geometry::InsetFillFlock, _) => FILLED_PS_BYTES,
        _ => WHITE_PS_BYTES,
    };
    let flat_color = matches!(capture.package_id, BREED_FILL_PACKAGE_ID | TRANSLUCENT_FILL_PACKAGE_ID)
        || (!capture.geometry.is_filled() && ps_bytes != WHITE_PS_BYTES);
    let wake_blend = capture.package_id == SUNRISE_RIBBON_PACKAGE_ID;
    let varying_inputs = if wake_blend {
        3
    } else {
        u8::from(flat_color || ribbon)
    };
    if [vs.len(), hs.len(), ds.len(), ps.len()].map(|words| words * 4)
        != [VS_BYTES, capture.hs_bytes, capture.ds_bytes, ps_bytes]
    {
        return None;
    }
    Some(slot.call_once(|| Program {
        capture,
        tessellation_code: [hs, ds],
        pipeline: TrianglePipeline {
            vs: TriangleVertexShader {
                code: vs,
                meta: TriangleVertexShaderMetadata {
                    kernel: ShaderKernelMetadata {
                        code_offset_bytes: 0,
                        code_size_bytes: VS_BYTES as u32,
                        code_alignment_bytes: 64,
                        ksp_offset_bytes: 0,
                        dispatch_mode: DispatchMode::Simd8,
                        grf_start_register: 2,
                        grf_used: 128,
                        push_constant_bytes: 0,
                        binding_table_entry_count: 0,
                        sampler_count: 0,
                    },
                    max_threads: 546,
                    urb_entry_output_length: VS_URB_OUTPUT_LENGTH,
                },
            },
            ps: TrianglePixelShader {
                code: ps,
                meta: TrianglePixelShaderMetadata {
                    kernel: ShaderKernelMetadata {
                        code_offset_bytes: VS_BYTES.next_multiple_of(64) as u32,
                        code_size_bytes: ps_bytes as u32,
                        code_alignment_bytes: 64,
                        ksp_offset_bytes: 0,
                        dispatch_mode: DispatchMode::Simd16,
                        grf_start_register: if ribbon { 6 } else { 2 },
                        grf_used: 128,
                        push_constant_bytes: 0,
                        binding_table_entry_count: if ribbon { 3 } else { 1 },
                        sampler_count: u8::from(ribbon),
                    },
                    num_varying_inputs: varying_inputs,
                    uses_vmask: true,
                    computed_stencil: false,
                    persample_dispatch: false,
                    computed_depth_mode: 0,
                    flat_inputs: if wake_blend { 1 << 2 } else { u32::from(flat_color) },
                },
            },
        },
    }))
}

pub(crate) fn for_pipeline(pipeline: &TrianglePipeline) -> Option<&'static Program> {
    PROGRAMS
        .iter()
        .filter_map(|(capture, slot)| load(capture, slot))
        .find(|program| core::ptr::eq(pipeline.vs.code, program.pipeline.vs.code))
}

pub(crate) fn pipeline(geometry: Geometry) -> Option<&'static TrianglePipeline> {
    PROGRAMS
        .iter()
        .filter(|(capture, _)| capture.geometry == geometry)
        .find_map(|(capture, slot)| load(capture, slot))
        .map(|program| &program.pipeline)
}

pub(crate) fn projected_mixed_pipeline() -> Option<&'static TrianglePipeline> {
    PROGRAMS
        .iter()
        .filter(|(capture, _)| {
            matches!(capture.package_id, NEUTRAL_MIXED_OUTLINE_PACKAGE_ID | PROJECTED_WIDTHS_PACKAGE_ID)
        })
        .find_map(|(capture, slot)| load(capture, slot))
        .map(|program| &program.pipeline)
}

pub(crate) fn tagged_mixed_pipeline() -> Option<&'static TrianglePipeline> {
    PROGRAMS
        .iter()
        .filter(|(capture, _)| {
            capture.geometry == Geometry::MixedFlock && capture.package_id != NEUTRAL_MIXED_OUTLINE_PACKAGE_ID
        })
        .find_map(|(capture, slot)| load(capture, slot))
        .map(|program| &program.pipeline)
}
