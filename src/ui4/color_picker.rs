//! Kernel-internal UI4 picker for the primary display bottom color.
//!
//! The service owns no frame while closed. The ordinary default context menu
//! queues an open request; Escape closes the session and transfers its frame
//! ring back to UI4 for SURFLIVE-safe retirement.

use core::sync::atomic::{AtomicBool, Ordering};
use spin::Mutex;
use trueos_time::{Duration, Timer};

use super::{
    DamageRect, FrameBuffering, FrameCadence, FrameContent, FrameHandle, FrameSpec, OutputId,
    PremultipliedRgba8, ScanoutFormat, Ui4CursorSource, Ui4InputEvent, WindowCreate,
    WindowInteraction, WindowOwner, WindowPlacement, WindowPlane, WindowSessionCloseRequest,
    WindowSessionId, acquire_frame_buffer, begin_window_session, create_frame, create_window,
    destroy_frame, finish_window_session, finish_window_session_with_request, publish_frame_buffer,
    publish_window_frame, take_owner_input_events, writable_rgba_view,
};

const OWNER: WindowOwner = WindowOwner::COLOR_PICKER_SERVICE;
const PRIMARY_BUTTON_MASK: u32 = 1;
const SERVICE_POLL_MS: u64 = 16;
pub(super) const PICKER_WIDTH: u32 = 256;
const SV_HEIGHT: u32 = 256;
const PANEL_GAP: u32 = 8;
const HUE_HEIGHT: u32 = 32;
const HUE_Y: u32 = SV_HEIGHT + PANEL_GAP;
const GAMMA_Y: u32 = HUE_Y + HUE_HEIGHT + PANEL_GAP;
const GAMMA_HEIGHT: u32 = 48;
pub(super) const PICKER_HEIGHT: u32 = GAMMA_Y + GAMMA_HEIGHT + PANEL_GAP;
const PICKER_MARGIN: u32 = 24;
const MAX_ACTIVE_GESTURES: usize = 32;
// Tiger Lake PRM Vol 2c: Pipe A precision palette and pipe gamma mode.
const PIPE_A_PREC_INDEX: usize = 0x4A400;
const PIPE_A_PREC_DATA: usize = 0x4A404;
const PIPE_A_GAMMA_MODE: usize = 0x4A480;
const POST_CSC_GAMMA_ENABLE: u32 = 1 << 30;
const GAMMA_MODE_MASK: u32 = 3;
const GAMMA_MODE_10_BIT: u32 = 1;
const PRECISION_PALETTE_ENTRIES: usize = 1024;

static OPEN_REQUEST: Mutex<Option<ColorPickerOpenRequest>> = Mutex::new(None);
static ESCAPE_REQUESTED: AtomicBool = AtomicBool::new(false);

#[derive(Copy, Clone)]
struct ColorPickerOpenRequest {
    source: Ui4CursorSource,
    anchor: (u32, u32),
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum PickerPanel {
    SaturationValue,
    Hue,
    Gamma,
}

struct PipeGammaSnapshot {
    mode: u32,
    precision_palette: [u32; PRECISION_PALETTE_ENTRIES],
}

#[derive(Copy, Clone)]
struct PickerGesture {
    source: Ui4CursorSource,
    panel: PickerPanel,
}

#[derive(Copy, Clone)]
struct PickerColor {
    hue: u16,
    saturation: u8,
    value: u8,
}

impl PickerColor {
    const INITIAL: Self = Self {
        hue: 512,
        saturation: u8::MAX,
        value: u8::MAX,
    };

    fn rgb(self) -> [u8; 3] {
        hsv_to_rgb(self.hue, self.saturation, self.value)
    }
}

static SELECTED_COLOR: Mutex<PickerColor> = Mutex::new(PickerColor::INITIAL);
static SELECTED_GAMMA_X: Mutex<u8> = Mutex::new(127);

struct ActiveColorPicker {
    session: WindowSessionId,
    picker_frame: FrameHandle,
    picker_window: super::WindowId,
    escape_hook: super::GlobalKeyboardHookId,
    color: PickerColor,
    gamma_x: u8,
    gamma_at_open: Option<PipeGammaSnapshot>,
    gestures: [Option<PickerGesture>; MAX_ACTIVE_GESTURES],
    picker_dirty: bool,
}

pub(super) fn request_open(source: Ui4CursorSource, anchor: (u32, u32)) {
    *OPEN_REQUEST.lock() = Some(ColorPickerOpenRequest { source, anchor });
}

fn capture_escape(
    event: &crate::r::keyboard::TrueosKeyboardOutputEvent,
) -> super::GlobalKeyboardDisposition {
    if event.kind == crate::r::keyboard::KEYBOARD_OUTPUT_KIND_KEY
        && event.key_code == crate::r::keyboard::KEYBOARD_KEY_ESCAPE
    {
        ESCAPE_REQUESTED.store(true, Ordering::Release);
        super::GlobalKeyboardDisposition::Consume
    } else {
        super::GlobalKeyboardDisposition::PassThrough
    }
}

#[trueos_executor::task]
pub(crate) async fn ui4_color_picker_service_task() {
    crate::log_info!(target: "ui4/color-picker";
        "ui4/color-picker: service online lifecycle=context-menu-open+escape-close owner=kernel-internal presentation=slot4-software-cursor-plane/fixed/no-application-plane-fallback controls=sv256+hue32+gamma48 commit=button-release target=pipe-a-bottom-color+pipe-a-gamma color=rgb\n"
    );
    let mut active = None;
    loop {
        if let Some(request) = OPEN_REQUEST.lock().take() {
            if active.is_none() {
                match open_picker(request) {
                    Ok(picker) => active = Some(picker),
                    Err(reason) => crate::log_warn!(target: "ui4/color-picker";
                        "ui4/color-picker: open rejected reason={}\n", reason,
                    ),
                }
            }
        }

        if let Some(picker) = active.as_mut() {
            service_active_picker(picker);
        }
        if ESCAPE_REQUESTED.swap(false, Ordering::AcqRel) {
            if active
                .as_ref()
                .is_some_and(|picker| close_picker(picker, "escape"))
            {
                active = None;
            }
        }
        Timer::after(Duration::from_millis(SERVICE_POLL_MS)).await;
    }
}

fn service_active_picker(picker: &mut ActiveColorPicker) {
    for event in take_owner_input_events(OWNER) {
        match event {
            Ui4InputEvent::Pointer(event) if event.window == picker.picker_window => {
                picker.handle_pointer(event);
            }
            _ => {}
        }
    }

    if picker.picker_dirty && render_picker(picker).is_ok() {
        picker.picker_dirty = false;
    }
}

fn open_picker(request: ColorPickerOpenRequest) -> Result<ActiveColorPicker, &'static str> {
    let output = OutputId::from_slot(0).ok_or("output-unavailable")?;
    let (screen_width, screen_height) = crate::intel::active_scanout_dimensions()
        .or_else(crate::virtio_gpu_logo::output_dimensions)
        .ok_or("scanout-unavailable")?;
    if screen_width == 0 || screen_height == 0 {
        return Err("scanout-empty");
    }

    let session = begin_window_session(OWNER).map_err(|_| "session-create")?;
    let picker_frame = match create_picker_frame(output, PICKER_WIDTH, PICKER_HEIGHT) {
        Ok(frame) => frame,
        Err(_) => {
            let _ = finish_window_session(OWNER, session);
            return Err("control-frame-create");
        }
    };

    let picker_x = request
        .anchor
        .0
        .saturating_add(PICKER_MARGIN)
        .min(screen_width.saturating_sub(PICKER_WIDTH));
    let picker_y = request
        .anchor
        .1
        .saturating_add(PICKER_MARGIN)
        .min(screen_height.saturating_sub(PICKER_HEIGHT));
    let picker_window = match create_window(WindowCreate {
        owner: OWNER,
        session,
        frame: picker_frame,
        output,
        plane: WindowPlane::Interaction,
        placement: WindowPlacement {
            x: picker_x as i32,
            y: picker_y as i32,
            width: PICKER_WIDTH,
            height: PICKER_HEIGHT,
            z: 100,
            opacity: u8::MAX,
            visible: true,
        },
        interaction: WindowInteraction::APPLICATION_FIXED_FRAME,
    }) {
        Ok(window) => window,
        Err(_) => {
            cleanup_failed_open(session, picker_frame);
            return Err("control-window-create");
        }
    };

    let escape_hook = match super::register_global_keyboard_hook(u8::MAX, capture_escape) {
        Ok(hook) => hook,
        Err(_) => {
            cleanup_failed_open(session, picker_frame);
            return Err("escape-hook-register");
        }
    };

    let mut picker = ActiveColorPicker {
        session,
        picker_frame,
        picker_window,
        escape_hook,
        color: *SELECTED_COLOR.lock(),
        gamma_x: *SELECTED_GAMMA_X.lock(),
        gamma_at_open: read_pipe_a_gamma(),
        gestures: [None; MAX_ACTIVE_GESTURES],
        picker_dirty: true,
    };
    let initial = picker.color.rgb();
    if !set_bottom_color(initial) {
        let _ = close_picker(&picker, "initial-bottom-color-program-failed");
        return Err("initial-bottom-color-program");
    }
    if render_picker(&picker).is_err() {
        let _ = close_picker(&picker, "initial-control-render-failed");
        return Err("initial-control-render");
    }
    picker.picker_dirty = false;
    let _ = super::input_broker::select_window_for_cursor_at(
        request.source,
        OWNER,
        picker_window,
        request.anchor.0,
        request.anchor.1,
    );

    let rgb = picker.color.rgb();
    if let Some(gamma) = picker.gamma_at_open.as_ref() {
        crate::log_info!(target: "ui4/color-picker";
            "ui4/color-picker: pipe-a-gamma opened mode=0x{:08X} precision_midpoint=0x{:08X}\n",
            gamma.mode,
            gamma.precision_palette[512],
        );
    }
    crate::log_info!(target: "ui4/color-picker";
        "ui4/color-picker: opened session={} picker_frame={} picker_window={} plane=slot4-software-cursor/fixed picker={}x{}@{},{} rgb={},{},{} target=pipe-a-bottom-color commit=release fallback_application_plane=0\n",
        session.raw(),
        picker_frame.raw(),
        picker_window.raw(),
        PICKER_WIDTH,
        PICKER_HEIGHT,
        picker_x,
        picker_y,
        rgb[0],
        rgb[1],
        rgb[2],
    );
    Ok(picker)
}

fn create_picker_frame(
    output: OutputId,
    width: u32,
    height: u32,
) -> Result<FrameHandle, super::FramePoolError> {
    create_frame(FrameSpec {
        output,
        content: FrameContent::Image,
        cadence: FrameCadence::Dirty,
        buffering: FrameBuffering::Double,
        format: ScanoutFormat::Rgba8888Premultiplied,
        width,
        height,
        base_color: Some(PremultipliedRgba8::TRANSPARENT),
    })
}

fn cleanup_failed_open(session: WindowSessionId, picker: FrameHandle) {
    let _ = finish_window_session(OWNER, session);
    let _ = destroy_frame(picker);
}

fn close_picker(picker: &ActiveColorPicker, reason: &'static str) -> bool {
    let close = WindowSessionCloseRequest::default().animate_and_retire_frames();
    match finish_window_session_with_request(OWNER, picker.session, close) {
        Ok(closed) => {
            let _ = super::unregister_global_keyboard_hook(picker.escape_hook);
            super::input_broker::notify_slot4_visual_change();
            crate::log_info!(target: "ui4/color-picker";
                "ui4/color-picker: closed reason={} session={} windows={} frame_retirement=ui4-owned\n",
                reason,
                picker.session.raw(),
                closed,
            );
            true
        }
        Err(error) => {
            crate::log_warn!(target: "ui4/color-picker";
                "ui4/color-picker: close failed reason={} session={} error={:?}\n",
                reason,
                picker.session.raw(),
                error,
            );
            false
        }
    }
}

impl ActiveColorPicker {
    fn handle_pointer(&mut self, event: super::input_broker::Ui4PointerEvent) {
        if event.buttons_pressed & PRIMARY_BUTTON_MASK != 0 {
            if let Some(panel) = panel_at(event.local_x, event.local_y) {
                self.begin_gesture(event.source, panel);
            }
        }
        if event.buttons_released & PRIMARY_BUTTON_MASK == 0 {
            return;
        }
        let Some(panel) = self.take_gesture(event.source) else {
            return;
        };
        self.commit(panel, event.local_x, event.local_y);
    }

    fn begin_gesture(&mut self, source: Ui4CursorSource, panel: PickerPanel) {
        if let Some(gesture) = self
            .gestures
            .iter_mut()
            .flatten()
            .find(|g| g.source == source)
        {
            gesture.panel = panel;
            return;
        }
        if let Some(slot) = self.gestures.iter_mut().find(|slot| slot.is_none()) {
            *slot = Some(PickerGesture { source, panel });
        }
    }

    fn take_gesture(&mut self, source: Ui4CursorSource) -> Option<PickerPanel> {
        let slot = self
            .gestures
            .iter_mut()
            .find(|slot| slot.is_some_and(|gesture| gesture.source == source))?;
        slot.take().map(|gesture| gesture.panel)
    }

    fn commit(&mut self, panel: PickerPanel, local_x: i32, local_y: i32) {
        match panel {
            PickerPanel::SaturationValue => {
                self.color.saturation = panel_axis(local_x, 0, PICKER_WIDTH);
                self.color.value = u8::MAX - panel_axis(local_y, 0, SV_HEIGHT);
            }
            PickerPanel::Hue => {
                let x = u32::from(panel_axis(local_x, 0, PICKER_WIDTH));
                self.color.hue = ((x * 1535 + 127) / 255) as u16;
            }
            PickerPanel::Gamma => {
                let x = panel_axis(local_x, 0, PICKER_WIDTH);
                if self.gamma_at_open.is_some() {
                    let programmed = set_pipe_a_gamma(x);
                    if !programmed {
                        crate::log_warn!(target: "ui4/color-picker";
                            "ui4/color-picker: pipe-a-gamma readback mismatch x={}\n", x,
                        );
                        return;
                    }
                    self.gamma_x = x;
                    *SELECTED_GAMMA_X.lock() = x;
                    self.picker_dirty = true;
                    crate::log_info!(target: "ui4/color-picker";
                        "ui4/color-picker: selected panel=Gamma x={} exponent_x1000={} pipe_a_gamma_programmed=1\n",
                        x, gamma_exponent_milli(x),
                    );
                }
                return;
            }
        }
        *SELECTED_COLOR.lock() = self.color;
        self.picker_dirty = true;
        let rgb = self.color.rgb();
        let programmed = set_bottom_color(rgb);
        crate::log_info!(target: "ui4/color-picker";
            "ui4/color-picker: selected panel={:?} hsv={},{},{} rgb={},{},{} pipe_a_bottom_programmed={} picker_publish=pending\n",
            panel,
            self.color.hue,
            self.color.saturation,
            self.color.value,
            rgb[0],
            rgb[1],
            rgb[2],
            programmed as u8,
        );
    }
}

fn panel_at(x: i32, y: i32) -> Option<PickerPanel> {
    if x < 0 || x >= PICKER_WIDTH as i32 || y < 0 {
        return None;
    }
    let y = y as u32;
    if y < SV_HEIGHT {
        Some(PickerPanel::SaturationValue)
    } else if (HUE_Y..HUE_Y + HUE_HEIGHT).contains(&y) {
        Some(PickerPanel::Hue)
    } else if (GAMMA_Y..GAMMA_Y + GAMMA_HEIGHT).contains(&y) {
        Some(PickerPanel::Gamma)
    } else {
        None
    }
}

fn panel_axis(value: i32, origin: u32, extent: u32) -> u8 {
    if extent <= 1 {
        return 0;
    }
    let local = i64::from(value)
        .saturating_sub(i64::from(origin))
        .clamp(0, i64::from(extent - 1)) as u32;
    ((local * 255 + (extent - 1) / 2) / (extent - 1)) as u8
}

fn render_picker(picker: &ActiveColorPicker) -> Result<(), ()> {
    render_and_publish(
        picker.picker_frame,
        picker.picker_window,
        |pixels, width, _height, pitch| {
            pixels.fill(0);
            for y in 0..SV_HEIGHT {
                let value = u8::MAX - panel_axis(y as i32, 0, SV_HEIGHT);
                for x in 0..PICKER_WIDTH {
                    let saturation = panel_axis(x as i32, 0, PICKER_WIDTH);
                    let rgb = hsv_to_rgb(picker.color.hue, saturation, value);
                    write_opaque_pixel(pixels, pitch, x, y, rgb);
                }
            }
            for x in 0..PICKER_WIDTH {
                let hue =
                    ((u32::from(panel_axis(x as i32, 0, PICKER_WIDTH)) * 1535 + 127) / 255) as u16;
                let rgb = hsv_to_rgb(hue, u8::MAX, u8::MAX);
                for y in HUE_Y..HUE_Y + HUE_HEIGHT {
                    write_opaque_pixel(pixels, pitch, x, y, rgb);
                }
            }
            for x in 0..PICKER_WIDTH {
                let input = x as u8;
                let level = gamma_entry(input, picker.gamma_x);
                let rgb = if picker.gamma_at_open.is_some() {
                    [level; 3]
                } else {
                    [48; 3]
                };
                for y in GAMMA_Y..GAMMA_Y + GAMMA_HEIGHT {
                    write_opaque_pixel(pixels, pitch, x, y, rgb);
                }
            }
            for y in GAMMA_Y..GAMMA_Y + GAMMA_HEIGHT {
                write_opaque_pixel(pixels, pitch, u32::from(picker.gamma_x), y, [255, 64, 0]);
            }
            let _ = width;
        },
    )
}

fn render_and_publish(
    frame: FrameHandle,
    window: super::WindowId,
    render: impl FnOnce(&mut [u8], u32, u32, usize),
) -> Result<(), ()> {
    let lease = acquire_frame_buffer(frame).map_err(|_| ())?;
    let view = match writable_rgba_view(lease) {
        Ok(view) => view,
        Err(_) => {
            let _ = super::cancel_frame_buffer(lease);
            return Err(());
        }
    };
    let pixels = unsafe { core::slice::from_raw_parts_mut(view.virt, view.byte_len) };
    render(pixels, view.width, view.height, view.pitch as usize);
    crate::intel::dma_flush(view.virt, view.byte_len);
    publish_frame_buffer(lease).map_err(|_| ())?;
    publish_window_frame(OWNER, window, DamageRect::FULL).map_err(|_| ())?;
    super::input_broker::notify_slot4_visual_change();
    Ok(())
}

fn write_opaque_pixel(pixels: &mut [u8], pitch: usize, x: u32, y: u32, rgb: [u8; 3]) {
    let offset = y as usize * pitch + x as usize * 4;
    if let Some(pixel) = pixels.get_mut(offset..offset + 4) {
        pixel.copy_from_slice(&[rgb[0], rgb[1], rgb[2], u8::MAX]);
    }
}

/// HSV hue uses six 256-step sectors: 0 is red, 512 green, 1024 blue.
fn hsv_to_rgb(hue: u16, saturation: u8, value: u8) -> [u8; 3] {
    let hue = u32::from(hue.min(1535));
    let sector = hue / 256;
    let fraction = hue % 256;
    let saturation = u32::from(saturation);
    let value = u32::from(value);
    let p = value * (255 - saturation) / 255;
    let q = value * (255 - saturation * fraction / 255) / 255;
    let t = value * (255 - saturation * (255 - fraction) / 255) / 255;
    let (red, green, blue) = match sector {
        0 => (value, t, p),
        1 => (q, value, p),
        2 => (p, value, t),
        3 => (p, q, value),
        4 => (t, p, value),
        _ => (value, p, q),
    };
    [red as u8, green as u8, blue as u8]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsv_primaries_are_exact() {
        assert_eq!(hsv_to_rgb(0, 255, 255), [255, 0, 0]);
        assert_eq!(hsv_to_rgb(512, 255, 255), [0, 255, 0]);
        assert_eq!(hsv_to_rgb(1024, 255, 255), [0, 0, 255]);
    }

    #[test]
    fn panels_are_disjoint() {
        assert_eq!(panel_at(0, 0), Some(PickerPanel::SaturationValue));
        assert_eq!(panel_at(255, 255), Some(PickerPanel::SaturationValue));
        assert_eq!(panel_at(0, HUE_Y as i32), Some(PickerPanel::Hue));
        assert_eq!(panel_at(0, (HUE_Y + HUE_HEIGHT - 1) as i32), Some(PickerPanel::Hue));
        assert_eq!(panel_at(0, (HUE_Y - 1) as i32), None);
        assert_eq!(panel_at(0, (HUE_Y + HUE_HEIGHT) as i32), None);
        assert_eq!(panel_at(0, GAMMA_Y as i32), Some(PickerPanel::Gamma));
        assert_eq!(panel_at(255, (GAMMA_Y + GAMMA_HEIGHT - 1) as i32), Some(PickerPanel::Gamma));
    }

    #[test]
    fn gamma_identity_and_endpoints() {
        assert_eq!(gamma_entry(0, 127), 0);
        assert_eq!(gamma_entry(255, 127), 255);
        assert_eq!(gamma_exponent_milli(127), 1000);
        assert!(gamma_entry(128, 0) > gamma_entry(128, 127));
        assert!(gamma_entry(128, 255) < gamma_entry(128, 127));
    }
}

fn set_bottom_color(rgb: [u8; 3]) -> bool {
    crate::intel::set_pipe_a_bottom_color_rgb8(rgb[0], rgb[1], rgb[2])
        || crate::virtio_gpu_logo::set_background_color(rgb[0], rgb[1], rgb[2])
}

// Keep this first gamma experiment local to the UI4 picker. Reading the
// precision palette captures the hardware state before any UI interaction.
fn read_pipe_a_gamma() -> Option<PipeGammaSnapshot> {
    let dev = crate::intel::claimed_device()?;
    let mode = crate::intel::mmio_read(dev, PIPE_A_GAMMA_MODE);
    let precision_index = crate::intel::mmio_read(dev, PIPE_A_PREC_INDEX);
    let mut precision_palette = [0; PRECISION_PALETTE_ENTRIES];
    for (index, entry) in precision_palette.iter_mut().enumerate() {
        crate::intel::mmio_write(dev, PIPE_A_PREC_INDEX, index as u32);
        *entry = crate::intel::mmio_read(dev, PIPE_A_PREC_DATA);
    }
    crate::intel::mmio_write(dev, PIPE_A_PREC_INDEX, precision_index);
    Some(PipeGammaSnapshot {
        mode,
        precision_palette,
    })
}

fn gamma_exponent_milli(x: u8) -> u32 {
    if x <= 127 {
        500 + u32::from(x) * 500 / 127
    } else {
        1000 + (u32::from(x) - 127) * 500 / 128
    }
}

fn gamma_entry(input: u8, x: u8) -> u8 {
    if x == 127 {
        return input;
    }
    let exponent = gamma_exponent_milli(x) as f32 / 1000.0;
    (libm::powf(f32::from(input) / 255.0, exponent) * 255.0 + 0.5).clamp(0.0, 255.0) as u8
}

fn gamma_entry_10(input: u16, x: u8) -> u16 {
    if x == 127 {
        return input;
    }
    let exponent = gamma_exponent_milli(x) as f32 / 1000.0;
    (libm::powf(f32::from(input) / 1023.0, exponent) * 1023.0 + 0.5).clamp(0.0, 1023.0) as u16
}

fn set_pipe_a_gamma(x: u8) -> bool {
    let entries = core::array::from_fn(|input| {
        let value = u32::from(gamma_entry_10(input as u16, x));
        (value << 20) | (value << 10) | value
    });
    program_pipe_a_precision_gamma(&entries)
}

pub(crate) fn program_pipe_a_precision_gamma(entries: &[u32; PRECISION_PALETTE_ENTRIES]) -> bool {
    let Some(dev) = crate::intel::claimed_device() else {
        return false;
    };
    let old_mode = crate::intel::mmio_read(dev, PIPE_A_GAMMA_MODE);
    let old_index = crate::intel::mmio_read(dev, PIPE_A_PREC_INDEX);
    // Stop the post-CSC lookup while replacing all entries, then switch to
    // the PRM's 1024-entry precision gamma mode on the next vertical blank.
    crate::intel::mmio_write(dev, PIPE_A_GAMMA_MODE, old_mode & !POST_CSC_GAMMA_ENABLE);
    for (input, rgb) in entries.iter().copied().enumerate() {
        crate::intel::mmio_write(dev, PIPE_A_PREC_INDEX, input as u32);
        crate::intel::mmio_write(dev, PIPE_A_PREC_DATA, rgb);
    }
    let mode = (old_mode & !GAMMA_MODE_MASK) | GAMMA_MODE_10_BIT | POST_CSC_GAMMA_ENABLE;
    crate::intel::mmio_write(dev, PIPE_A_GAMMA_MODE, mode);
    let expected_midpoint = entries[512];
    crate::intel::mmio_write(dev, PIPE_A_PREC_INDEX, 512);
    let readback = crate::intel::mmio_read(dev, PIPE_A_PREC_DATA);
    let verified =
        crate::intel::mmio_read(dev, PIPE_A_GAMMA_MODE) == mode && readback == expected_midpoint;
    // Keep the indirect palette port exactly as another display user left it.
    crate::intel::mmio_write(dev, PIPE_A_PREC_INDEX, old_index);
    verified
}
