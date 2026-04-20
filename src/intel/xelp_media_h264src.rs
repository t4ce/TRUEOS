use super::xelp_media_matroska::{
    MatroskaH264Summary, get_sample_data as get_matroska_sample_data, looks_like_matroska,
    parse_h264_matroska_summary,
};
use super::xelp_media_mp4::{
    AvccSummary, Mp4H264Summary, get_sample_data as get_mp4_sample_data, parse_h264_mp4_summary,
};

pub(crate) enum H264SourceSummary<'a> {
    Mp4(Mp4H264Summary<'a>),
    Matroska(MatroskaH264Summary<'a>),
}

impl<'a> H264SourceSummary<'a> {
    pub(crate) fn container_name(&self) -> &'static str {
        match self {
            Self::Mp4(_) => "mp4",
            Self::Matroska(_) => "matroska",
        }
    }

    pub(crate) fn width(&self) -> u16 {
        match self {
            Self::Mp4(summary) => summary.width,
            Self::Matroska(summary) => summary.width,
        }
    }

    pub(crate) fn height(&self) -> u16 {
        match self {
            Self::Mp4(summary) => summary.height,
            Self::Matroska(summary) => summary.height,
        }
    }

    pub(crate) fn sample_count(&self) -> u32 {
        match self {
            Self::Mp4(summary) => summary.sample_count,
            Self::Matroska(summary) => summary.sample_count,
        }
    }

    pub(crate) fn first_sample(&self) -> &'a [u8] {
        match self {
            Self::Mp4(summary) => summary.first_sample,
            Self::Matroska(summary) => summary.first_sample,
        }
    }

    pub(crate) fn avcc(&self) -> &AvccSummary<'a> {
        match self {
            Self::Mp4(summary) => &summary.avcc,
            Self::Matroska(summary) => &summary.avcc,
        }
    }

    pub(crate) fn sample_data(&'a self, index: u32) -> Option<&'a [u8]> {
        match self {
            Self::Mp4(summary) => get_mp4_sample_data(summary, index),
            Self::Matroska(summary) => get_matroska_sample_data(summary, index),
        }
    }
}

pub(crate) fn parse_h264_source_summary<'a>(
    bytes: &'a [u8],
) -> Result<H264SourceSummary<'a>, &'static str> {
    if looks_like_matroska(bytes) {
        return parse_h264_matroska_summary(bytes)
            .map(H264SourceSummary::Matroska)
            .map_err(|_| "matroska");
    }
    parse_h264_mp4_summary(bytes)
        .map(H264SourceSummary::Mp4)
        .map_err(|_| "mp4")
}
