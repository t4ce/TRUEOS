use super::TrianglePipeline;

pub(crate) const TRIANGLE_PIPELINE_NOTE: &str =
    "offline-baked Xe-LP VS/PS blobs have not been imported yet";

pub(crate) fn triangle_pipeline() -> Option<&'static TrianglePipeline> {
    None
}
