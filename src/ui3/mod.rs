//! UI3 shared assets.

mod ui3_asset_service;

pub use self::ui3_asset_service::ui3_asset_service_task;

pub(crate) const TRUESURFER_SMOKE_HTML_URL: &str = "inline://trueos/input.html";
pub(crate) const TRUESURFER_SMOKE_HTML_SOURCE: &str =
    include_str!("../../crates/trueos-qjs/src/html/input.html");
