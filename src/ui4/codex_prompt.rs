//! One-way user input for an already running Codex conversation.
//!
//! This is a kernel UI4 frame, not an application launch or clipboard paste.
//! It retains the exact receiver selected at open, so changing or retiring a
//! Codex conversation cannot silently redirect the user's message.

#[path = "codex_prompt_editor.rs"]
mod editor;

use alloc::{
    collections::{BTreeMap, BTreeSet},
    vec::Vec,
};
use embassy_sync::signal::Signal;
use spin::Mutex;
use trueos_time::{Duration, Instant, Timer};

use super::{
    DamageRect, FrameBuffering, FrameCadence, FrameContent, FrameHandle, FrameSpec, OutputId,
    PremultipliedRgba8, ScanoutFormat, Ui4CursorSource, Ui4InputEvent, WindowCreate, WindowId,
    WindowInteraction, WindowOwner, WindowPlacement, WindowPlane, WindowSessionCloseRequest,
    WindowSessionId,
};
use crate::r::services::{codex_input_service as codex, shell3_font_service as font};

const OWNER: WindowOwner = WindowOwner::CODEX_PROMPT_SERVICE;
use crate::spirit::{
    GREETING_GRID_HEIGHT_MM as HEIGHT_MM, GREETING_GRID_SCALE as GRID_SCALE,
    GREETING_GRID_WIDTH_MM as WIDTH_MM,
};
const POLL_MS: u64 = 16;
const CARET_BLINK_MS: u64 = 500;

struct OpenRequest {
    source: Ui4CursorSource,
    anchor: (u32, u32),
    target: codex::Target,
}

static OPEN_REQUEST: Mutex<Option<OpenRequest>> = Mutex::new(None);
static OPEN_SIGNAL: Signal<crate::wait::EmbassySpinRawMutex, ()> = Signal::new();

struct ActivePrompt {
    session: WindowSessionId,
    frame: FrameHandle,
    window: WindowId,
    target: codex::Target,
    width: u32,
    height: u32,
    editor: editor::Editor,
    glyphs: BTreeMap<char, Vec<u8>>,
    dirty: bool,
    caret_visible: bool,
    blink: Instant,
    submitted: bool,
}

/// The context menu does nothing unless a live native Codex receiver exists.
/// Capture its identity here, before asynchronous font/frame preparation.
pub(crate) fn request_open(source: Ui4CursorSource, anchor: (u32, u32)) {
    if let Some(target) = codex::current_target() {
        *OPEN_REQUEST.lock() = Some(OpenRequest {
            source,
            anchor,
            target,
        });
        OPEN_SIGNAL.signal(());
    }
}

#[trueos_executor::task(pool_size = 1)]
pub(crate) async fn ui4_codex_prompt_service_task() {
    let mut active: Option<ActivePrompt> = None;
    loop {
        // Closed is the usual state: consume no timer/CPU work until a live
        // context-menu action queues an actual request.
        if active.is_none() {
            OPEN_SIGNAL.wait().await;
        }
        let request = OPEN_REQUEST.lock().take();
        if let Some(request) = request {
            if active.is_none() && codex::is_target_live(&request.target) {
                let opened = match font::prepare().await {
                    Ok(()) => open_prompt(request).await,
                    Err(reason) => Err(reason),
                };
                match opened {
                    Ok(prompt) => active = Some(prompt),
                    Err(reason) => crate::log_warn!(target: "ui4/codex";
                        "ui4/codex: open rejected reason={}\n", reason),
                }
            }
        }

        if let Some(prompt) = active.as_mut() {
            let mut close = prompt.submitted
                || super::window_is_closed(OWNER, prompt.window)
                || !codex::is_target_live(&prompt.target);
            if !close {
                close = process_input(prompt);
            }
            if close {
                if close_prompt(prompt) {
                    active = None;
                }
            } else {
                if prompt.blink.elapsed().as_millis() >= CARET_BLINK_MS {
                    prompt.caret_visible = !prompt.caret_visible;
                    prompt.blink = Instant::now();
                    prompt.dirty = true;
                }
                if prompt.dirty && render_prompt(prompt).await.is_ok() {
                    prompt.dirty = false;
                }
            }
        }
        if active.is_some() {
            Timer::after(Duration::from_millis(POLL_MS)).await;
        }
    }
}

async fn open_prompt(request: OpenRequest) -> Result<ActivePrompt, &'static str> {
    if !codex::is_target_live(&request.target) {
        return Err("receiver-retired");
    }
    let output = OutputId::from_slot(0).ok_or("output-unavailable")?;
    let (screen_width, screen_height) = super::output_dimensions().ok_or("scanout-unavailable")?;
    let (width, height) =
        super::physical_extent_pixels(WIDTH_MM, HEIGHT_MM).ok_or("physical-size-unavailable")?;
    if width > screen_width || height > screen_height {
        return Err("sheet-exceeds-scanout");
    }
    let session = super::begin_window_session(OWNER).map_err(|_| "session-create")?;
    let frame = match super::create_frame(FrameSpec {
        output,
        content: FrameContent::Image,
        cadence: FrameCadence::Dirty,
        buffering: FrameBuffering::Double,
        format: ScanoutFormat::Rgba8888Premultiplied,
        width,
        height,
        base_color: Some(PremultipliedRgba8::TRANSPARENT),
    }) {
        Ok(frame) => frame,
        Err(_) => {
            let _ = super::finish_window_session(OWNER, session);
            return Err("frame-create");
        }
    };
    let x = request.anchor.0.min(screen_width - width);
    let y = request.anchor.1.min(screen_height - height);
    let window = match super::create_window(WindowCreate {
        owner: OWNER,
        session,
        frame,
        output,
        plane: WindowPlane::Primary,
        placement: WindowPlacement {
            x: x as i32,
            y: y as i32,
            width,
            height,
            z: 100,
            opacity: u8::MAX,
            visible: true,
        },
        interaction: WindowInteraction::APPLICATION_FIXED_FRAME,
    }) {
        Ok(window) => window,
        Err(_) => {
            let _ = super::finish_window_session(OWNER, session);
            let _ = super::destroy_frame(frame);
            return Err("window-create");
        }
    };
    let _ = super::set_window_title(
        OWNER,
        window,
        "Codex · Enter sends · Shift+Enter new line · Esc closes",
    );
    let mut prompt = ActivePrompt {
        session,
        frame,
        window,
        target: request.target,
        width,
        height,
        editor: editor::Editor::default(),
        glyphs: BTreeMap::new(),
        dirty: true,
        caret_visible: true,
        blink: Instant::now(),
        submitted: false,
    };
    if render_prompt(&mut prompt).await.is_err() {
        let _ = close_prompt(&prompt);
        return Err("initial-render");
    }
    prompt.dirty = false;
    // Select only through the same cursor/combo which requested the panel.
    // UI4 retains other cursors' independent selections and keyboard routes.
    if super::input_broker::select_window_for_cursor_at(
        request.source,
        OWNER,
        window,
        request.anchor.0,
        request.anchor.1,
    )
    .is_err()
    {
        let _ = close_prompt(&prompt);
        return Err("selection");
    }
    Ok(prompt)
}

fn process_input(prompt: &mut ActivePrompt) -> bool {
    use crate::r::keyboard::*;
    for event in super::take_owner_input_events(OWNER) {
        match event {
            Ui4InputEvent::Keyboard(keyboard) if keyboard.window == prompt.window => {
                let event = keyboard.event;
                if event.kind == KEYBOARD_OUTPUT_KIND_TEXT {
                    if let Some(character) = char::from_u32(event.codepoint) {
                        let _ = prompt.editor.insert(character);
                    }
                } else if event.kind == KEYBOARD_OUTPUT_KIND_KEY
                    && event.flags & KEYBOARD_OUTPUT_FLAG_PRESS != 0
                {
                    match event.key_code {
                        KEYBOARD_KEY_ENTER => {
                            const SHIFT: u8 = (1 << 1) | (1 << 5);
                            if event.modifiers & SHIFT != 0 {
                                let _ = prompt.editor.insert('\n');
                            } else if !prompt.editor.text().trim().is_empty() {
                                // Mark accepted before teardown. A temporarily
                                // busy close must never submit the same text twice.
                                if codex::submit(&prompt.target, prompt.editor.text()).is_ok() {
                                    prompt.submitted = true;
                                    return true;
                                }
                            }
                        }
                        KEYBOARD_KEY_BACKSPACE => prompt.editor.backspace(),
                        KEYBOARD_KEY_DELETE => prompt.editor.delete(),
                        KEYBOARD_KEY_ARROW_LEFT => prompt.editor.left(),
                        KEYBOARD_KEY_ARROW_RIGHT => prompt.editor.right(),
                        KEYBOARD_KEY_ARROW_UP => prompt.editor.vertical(-1),
                        KEYBOARD_KEY_ARROW_DOWN => prompt.editor.vertical(1),
                        KEYBOARD_KEY_HOME => prompt.editor.home(),
                        KEYBOARD_KEY_END => prompt.editor.end(),
                        KEYBOARD_KEY_TAB => {
                            let _ = prompt.editor.insert('\t');
                        }
                        // Space arrives as text through the keyboard translator.
                        _ => {}
                    }
                } else {
                    continue;
                }
                prompt.caret_visible = true;
                prompt.blink = Instant::now();
                prompt.dirty = true;
            }
            Ui4InputEvent::Pointer(pointer)
                if pointer.window == prompt.window
                    && pointer.buttons_pressed & 1 != 0
                    && pointer.local_x >= 0
                    && pointer.local_y >= 0 =>
            {
                let column = pointer.local_x as usize * editor::COLUMNS / prompt.width as usize;
                let row = pointer.local_y as usize * editor::ROWS / prompt.height as usize;
                prompt.editor.click(column, row);
                prompt.caret_visible = true;
                prompt.blink = Instant::now();
                prompt.dirty = true;
            }
            _ => {}
        }
    }
    false
}

fn close_prompt(prompt: &ActivePrompt) -> bool {
    // UI4 owns scanout-safe frame retirement, including the default Escape
    // close which may already have hidden this window before this poll.
    if super::finish_window_session_with_request(
        OWNER,
        prompt.session,
        WindowSessionCloseRequest::default().animate_and_retire_frames(),
    )
    .is_err()
    {
        return false;
    }
    let _ = super::take_owner_input_events(OWNER);
    true
}

async fn render_prompt(prompt: &mut ActivePrompt) -> Result<(), ()> {
    let cells = prompt.editor.visible_cells();
    let needed: BTreeSet<char> = cells.iter().map(|cell| cell.character).collect();
    // At most one visible sheet of cached glyphs, irrespective of edits or
    // message length. Every glyph samples Shell3's boot-warmed R8 atlas.
    prompt
        .glyphs
        .retain(|character, _| needed.contains(character));
    for character in needed {
        if !prompt.glyphs.contains_key(&character) {
            let glyph = font::grid_glyph(GRID_SCALE, character)
                .await
                .map_err(|_| ())?;
            prompt.glyphs.insert(character, glyph);
        }
    }
    let lease = super::acquire_frame_buffer(prompt.frame).map_err(|_| ())?;
    let view = match super::writable_rgba_view(lease) {
        Ok(view) => view,
        Err(_) => {
            let _ = super::cancel_frame_buffer(lease);
            return Err(());
        }
    };
    let pixels = unsafe { core::slice::from_raw_parts_mut(view.virt, view.byte_len) };
    pixels.fill(255);
    for column in 0..=editor::COLUMNS {
        let x = (column as u32 * prompt.width / editor::COLUMNS as u32).min(prompt.width - 1);
        fill_rect(
            pixels,
            view.pitch,
            prompt.width,
            prompt.height,
            x,
            0,
            1,
            prompt.height,
            [190, 190, 190, 255],
        );
    }
    for row in 0..=editor::ROWS {
        let y = (row as u32 * prompt.height / editor::ROWS as u32).min(prompt.height - 1);
        fill_rect(
            pixels,
            view.pitch,
            prompt.width,
            prompt.height,
            0,
            y,
            prompt.width,
            1,
            [190, 190, 190, 255],
        );
    }
    for cell in cells {
        if let Some(glyph) = prompt.glyphs.get(&cell.character) {
            let (x, y, width, height) =
                cell_bounds(cell.column, cell.row, prompt.width, prompt.height);
            draw_glyph(
                pixels,
                view.pitch,
                prompt.width,
                prompt.height,
                x,
                y,
                width,
                height,
                glyph,
            );
        }
    }
    if prompt.caret_visible {
        let (column, row) = prompt.editor.caret();
        let (x, y, width, height) = cell_bounds(column, row, prompt.width, prompt.height);
        fill_rect(
            pixels,
            view.pitch,
            prompt.width,
            prompt.height,
            x + width / 2,
            y + 2,
            1,
            height.saturating_sub(4),
            [0, 0, 0, 255],
        );
    }
    crate::intel::dma_flush(view.virt, view.byte_len);
    super::publish_frame_buffer(lease).map_err(|_| ())?;
    super::publish_window_frame(OWNER, prompt.window, DamageRect::FULL).map_err(|_| ())?;
    Ok(())
}

fn cell_bounds(column: usize, row: usize, width: u32, height: u32) -> (u32, u32, u32, u32) {
    let x = column as u32 * width / editor::COLUMNS as u32;
    let y = row as u32 * height / editor::ROWS as u32;
    let right = (column + 1) as u32 * width / editor::COLUMNS as u32;
    let bottom = (row + 1) as u32 * height / editor::ROWS as u32;
    (x, y, right - x, bottom - y)
}

fn draw_glyph(
    pixels: &mut [u8],
    pitch: u32,
    width: u32,
    height: u32,
    cell_x: u32,
    cell_y: u32,
    cell_width: u32,
    cell_height: u32,
    glyph: &[u8],
) {
    if glyph.len() < 8 {
        return;
    }
    let glyph_width = u32::from_le_bytes(glyph[..4].try_into().unwrap());
    let glyph_height = u32::from_le_bytes(glyph[4..8].try_into().unwrap());
    let left = cell_x + cell_width.saturating_sub(glyph_width) / 2;
    let top = cell_y + cell_height.saturating_sub(glyph_height) / 2;
    // Paper crops its white em border inside the ruling, preserving 1px lines.
    for y in 1..glyph_height.saturating_sub(1) {
        for x in 1..glyph_width.saturating_sub(1) {
            let px = left + x;
            let py = top + y;
            if px >= width
                || py >= height
                || px >= cell_x + cell_width
                || py >= cell_y + cell_height
            {
                continue;
            }
            let src = 8 + (y as usize * glyph_width as usize + x as usize) * 4;
            let dst = py as usize * pitch as usize + px as usize * 4;
            if let (Some(source), Some(destination)) =
                (glyph.get(src..src + 4), pixels.get_mut(dst..dst + 4))
            {
                destination.copy_from_slice(source);
            }
        }
    }
}

fn fill_rect(
    pixels: &mut [u8],
    pitch: u32,
    width: u32,
    height: u32,
    x: u32,
    y: u32,
    rect_width: u32,
    rect_height: u32,
    color: [u8; 4],
) {
    for row in y..y.saturating_add(rect_height).min(height) {
        for column in x..x.saturating_add(rect_width).min(width) {
            let offset = row as usize * pitch as usize + column as usize * 4;
            if let Some(pixel) = pixels.get_mut(offset..offset + 4) {
                pixel.copy_from_slice(&color);
            }
        }
    }
}
