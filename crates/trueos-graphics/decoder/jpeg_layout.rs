#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) enum JpegSampling {
    Yuv400,
    Yuv420,
    Yuv422H,
    Yuv444,
    Yuv411,
    Yuv422V,
    Other,
    Unknown,
}

impl JpegSampling {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Yuv400 => "yuv400",
            Self::Yuv420 => "yuv420",
            Self::Yuv422H => "yuv422h",
            Self::Yuv444 => "yuv444",
            Self::Yuv411 => "yuv411",
            Self::Yuv422V => "yuv422v",
            Self::Other => "other",
            Self::Unknown => "unknown",
        }
    }

    pub(crate) const fn from_mfx_input_format(input_format: u8) -> Self {
        match input_format {
            0 => Self::Yuv400,
            1 => Self::Yuv420,
            2 => Self::Yuv422H,
            3 => Self::Yuv444,
            4 => Self::Yuv411,
            5 => Self::Yuv422V,
            6 => Self::Yuv422H,
            7 => Self::Yuv422V,
            _ => Self::Unknown,
        }
    }
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(crate) struct JpegLayout {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) components: u8,
    pub(crate) sampling: JpegSampling,
}

impl JpegLayout {
    const UNKNOWN: Self = Self {
        width: 0,
        height: 0,
        components: 0,
        sampling: JpegSampling::Unknown,
    };
}

pub(crate) fn classify_jpeg_layout(encoded: &[u8]) -> JpegLayout {
    let Some((width, height, components, h, v)) = parse_sof_sampling(encoded) else {
        return JpegLayout::UNKNOWN;
    };
    let sampling = classify_sampling(components, h, v);
    JpegLayout {
        width,
        height,
        components,
        sampling,
    }
}

fn classify_sampling(components: u8, h: [u8; 3], v: [u8; 3]) -> JpegSampling {
    if components == 1 {
        return JpegSampling::Yuv400;
    }
    if components < 3 || h[1] != h[2] || v[1] != v[2] {
        return JpegSampling::Other;
    }
    match ((h[0], v[0]), (h[1], v[1])) {
        ((2, 2), (1, 1)) => JpegSampling::Yuv420,
        ((2, 1), (1, 1)) => JpegSampling::Yuv422H,
        ((1, 1), (1, 1)) => JpegSampling::Yuv444,
        ((4, 1), (1, 1)) => JpegSampling::Yuv411,
        ((1, 2), (1, 1)) => JpegSampling::Yuv422V,
        ((2, 2), (1, 2)) => JpegSampling::Yuv422H,
        ((2, 2), (2, 1)) => JpegSampling::Yuv422V,
        _ => JpegSampling::Other,
    }
}

fn parse_sof_sampling(encoded: &[u8]) -> Option<(u32, u32, u8, [u8; 3], [u8; 3])> {
    if encoded.len() < 4 || encoded[0] != 0xFF || encoded[1] != 0xD8 {
        return None;
    }

    let mut idx = 2usize;
    while idx + 3 < encoded.len() {
        if encoded[idx] != 0xFF {
            idx += 1;
            continue;
        }
        while idx < encoded.len() && encoded[idx] == 0xFF {
            idx += 1;
        }
        if idx >= encoded.len() {
            break;
        }
        let marker = encoded[idx];
        idx += 1;

        if marker == 0xD9 || marker == 0xDA {
            break;
        }
        if matches!(marker, 0x01 | 0xD0..=0xD7) {
            continue;
        }
        if idx + 1 >= encoded.len() {
            break;
        }
        let segment_len = u16::from_be_bytes([encoded[idx], encoded[idx + 1]]) as usize;
        idx += 2;
        if segment_len < 2 || idx + segment_len - 2 > encoded.len() {
            break;
        }

        if matches!(marker, 0xC0..=0xC2 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF)
            && segment_len >= 8
        {
            let height = u16::from_be_bytes([encoded[idx + 1], encoded[idx + 2]]) as u32;
            let width = u16::from_be_bytes([encoded[idx + 3], encoded[idx + 4]]) as u32;
            let components = encoded[idx + 5].min(3);
            let mut h = [0u8; 3];
            let mut v = [0u8; 3];
            let need = 6usize.saturating_add(usize::from(components).saturating_mul(3));
            if width == 0 || height == 0 || segment_len < need {
                return None;
            }
            for component_idx in 0..usize::from(components) {
                let hv = encoded[idx + 7 + component_idx * 3];
                h[component_idx] = hv >> 4;
                v[component_idx] = hv & 0x0F;
            }
            return Some((width, height, components, h, v));
        }

        idx += segment_len - 2;
    }

    None
}
