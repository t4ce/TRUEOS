pub mod althlasfont;
mod font;
mod ui3_asset_service;
mod ui3_service;

pub use self::ui3_asset_service::ui3_asset_service_task;
pub use self::ui3_service::ui3_service_task;

#[derive(Copy, Clone, Debug, Default)]
pub(in crate::ui3) struct Ui3Point {
    pub(in crate::ui3) x: f32,
    pub(in crate::ui3) y: f32,
}

#[derive(Copy, Clone, Debug, Default)]
pub(in crate::ui3) struct Ui3Rect {
    pub(in crate::ui3) x: f32,
    pub(in crate::ui3) y: f32,
    pub(in crate::ui3) w: f32,
    pub(in crate::ui3) h: f32,
}
