use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use embassy_time::{Duration as EmbassyDuration, Timer};

const TASK_NAME: &str = "ui3-asset-service";
const UI3_ASSET_SERVICE_PARK_MS: u64 = 1_000;
const UI3_ASSET_SERVICE_RETRY_MS: u64 = 50;
const UI3_FONT_SPRITE64_CELL_PX: u16 = 64;

static UI3_ASSET_SERVICE_READY: AtomicBool = AtomicBool::new(false);
static UI3_FONT_SPRITE64_READY: AtomicBool = AtomicBool::new(false);
static UI3_FONT_SPRITE64_READY_SEQ: AtomicU32 = AtomicU32::new(0);
static UI3_FONT_SPRITE64_WARM_ATTEMPTS: AtomicU32 = AtomicU32::new(0);
static UI3_FONT_SPRITE64_ATLAS_SLOTS: AtomicU32 = AtomicU32::new(0);
static UI3_FONT_SPRITE64_ATLAS_BYTES: AtomicU32 = AtomicU32::new(0);
static UI3_FONT_SPRITE64_ATLAS_GPU: AtomicU64 = AtomicU64::new(0);

#[derive(Copy, Clone, Debug, Default)]
pub(crate) struct Ui3FontSprite64AssetStatus {
    pub(crate) ready: bool,
    pub(crate) ready_seq: u32,
    pub(crate) warm_attempts: u32,
    pub(crate) atlas_slots: u32,
    pub(crate) atlas_bytes: u32,
    pub(crate) atlas_gpu: u64,
}

pub fn ui3_asset_service_ready() -> bool {
    UI3_ASSET_SERVICE_READY.load(Ordering::Acquire)
}

pub(crate) fn ui3_font_sprite64_assets_ready() -> bool {
    UI3_FONT_SPRITE64_READY.load(Ordering::Acquire)
}

pub(crate) fn ui3_font_sprite64_asset_status() -> Ui3FontSprite64AssetStatus {
    Ui3FontSprite64AssetStatus {
        ready: UI3_FONT_SPRITE64_READY.load(Ordering::Acquire),
        ready_seq: UI3_FONT_SPRITE64_READY_SEQ.load(Ordering::Acquire),
        warm_attempts: UI3_FONT_SPRITE64_WARM_ATTEMPTS.load(Ordering::Acquire),
        atlas_slots: UI3_FONT_SPRITE64_ATLAS_SLOTS.load(Ordering::Acquire),
        atlas_bytes: UI3_FONT_SPRITE64_ATLAS_BYTES.load(Ordering::Acquire),
        atlas_gpu: UI3_FONT_SPRITE64_ATLAS_GPU.load(Ordering::Acquire),
    }
}

#[embassy_executor::task]
pub async fn ui3_asset_service_task() {
    let _task_guard = crate::r::spawn_service::task_run_guard(TASK_NAME);
    UI3_ASSET_SERVICE_READY.store(false, Ordering::Release);
    UI3_FONT_SPRITE64_READY.store(false, Ordering::Release);

    crate::log!(
        "ui3-asset-service: starting font_backend=sprite64 faces={} cell={}px omit=2x\n",
        crate::gfx::althlasfont::bitmapfont::ATHLAS_UI3_SPRITE64_FONT_FACES.len(),
        UI3_FONT_SPRITE64_CELL_PX
    );

    loop {
        if crate::r::spawn_service::task_stop_requested(TASK_NAME) {
            crate::log!("ui3-asset-service: stop requested before-ready; exit\n");
            return;
        }

        let attempt = UI3_FONT_SPRITE64_WARM_ATTEMPTS.fetch_add(1, Ordering::AcqRel) + 1;
        match warm_font_sprite64_assets(attempt) {
            Some(status) => {
                publish_font_sprite64_ready(status);
                crate::r::readiness::set(crate::r::readiness::UI3_ASSET_SERVICE_READY);
                UI3_ASSET_SERVICE_READY.store(true, Ordering::Release);
                break;
            }
            None => {
                if attempt <= 8 || attempt.is_multiple_of(32) {
                    crate::log!(
                        "ui3-asset-service: font warm pending attempt={} reason=gpgpu-atlas-not-ready\n",
                        attempt
                    );
                }
                Timer::after(EmbassyDuration::from_millis(UI3_ASSET_SERVICE_RETRY_MS)).await;
            }
        }
    }

    loop {
        if crate::r::spawn_service::task_stop_requested(TASK_NAME) {
            crate::log!("ui3-asset-service: stop requested; parked exit\n");
            break;
        }
        Timer::after(EmbassyDuration::from_millis(UI3_ASSET_SERVICE_PARK_MS)).await;
    }
}

fn warm_font_sprite64_assets(attempt: u32) -> Option<Ui3FontSprite64AssetStatus> {
    let inventory = validate_sprite64_font_metrics()?;
    let warm = crate::intel::gpgpu::warm_sprite64_font_atlas()?;
    if !warm.ok {
        return None;
    }

    let status = Ui3FontSprite64AssetStatus {
        ready: true,
        ready_seq: UI3_FONT_SPRITE64_READY_SEQ
            .load(Ordering::Acquire)
            .wrapping_add(1)
            .max(1),
        warm_attempts: attempt,
        atlas_slots: u32::from(warm.slots),
        atlas_bytes: warm.bytes.min(u32::MAX as usize) as u32,
        atlas_gpu: warm.atlas_gpu,
    };

    crate::log!(
        "ui3-asset-service: font warm ok attempt={} faces={} buckets={} font_cells={} wide_cells={} max_cell={}x{} atlas={}x{} pitch={} slots={} bytes={} gpu=0x{:X}\n",
        attempt,
        inventory.face_count,
        inventory.bucket_count,
        inventory.cell_count,
        inventory.wide_cell_count,
        inventory.max_cell_w,
        inventory.max_cell_h,
        warm.width,
        warm.height,
        warm.pitch_bytes,
        warm.slots,
        warm.bytes,
        warm.atlas_gpu
    );

    Some(status)
}

fn publish_font_sprite64_ready(status: Ui3FontSprite64AssetStatus) {
    UI3_FONT_SPRITE64_ATLAS_SLOTS.store(status.atlas_slots, Ordering::Release);
    UI3_FONT_SPRITE64_ATLAS_BYTES.store(status.atlas_bytes, Ordering::Release);
    UI3_FONT_SPRITE64_ATLAS_GPU.store(status.atlas_gpu, Ordering::Release);
    UI3_FONT_SPRITE64_READY_SEQ.store(status.ready_seq, Ordering::Release);
    UI3_FONT_SPRITE64_READY.store(true, Ordering::Release);
}

fn validate_sprite64_font_metrics()
-> Option<crate::gfx::althlasfont::bitmapfont::AthlasFontSprite64Inventory> {
    crate::gfx::althlasfont::bitmapfont::athlas_validate_ui3_sprite64_faces(
        UI3_FONT_SPRITE64_CELL_PX,
    )
}
