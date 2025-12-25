use super::{header_height, FramebufferSurface, FRAMEBUFFER};

pub const HEADER_INDICATOR_PINK: u32 = super::PINK_FG_COLOR;
const HEADER_INDICATOR_SIZE: usize = 25;
const TRIG_SCALE: i32 = 1024;
const SIN_DEG: [i16; 360] = [
    0, 18, 36, 54, 71, 89, 107, 125, 143, 160, 178, 195, 213, 230, 248, 265, 282, 299, 316, 333,
    350, 367, 384, 400, 416, 433, 449, 465, 481, 496, 512, 527, 543, 558, 573, 587, 602, 616, 630,
    644, 658, 672, 685, 698, 711, 724, 737, 749, 761, 773, 784, 796, 807, 818, 828, 839, 849, 859,
    868, 878, 887, 896, 904, 912, 920, 928, 935, 943, 949, 956, 962, 968, 974, 979, 984, 989, 994,
    998, 1002, 1005, 1008, 1011, 1014, 1016, 1018, 1020, 1022, 1023, 1023, 1024, 1024, 1024, 1023,
    1023, 1022, 1020, 1018, 1016, 1014, 1011, 1008, 1005, 1002, 998, 994, 989, 984, 979, 974, 968,
    962, 956, 949, 943, 935, 928, 920, 912, 904, 896, 887, 878, 868, 859, 849, 839, 828, 818, 807,
    796, 784, 773, 761, 749, 737, 724, 711, 698, 685, 672, 658, 644, 630, 616, 602, 587, 573, 558,
    543, 527, 512, 496, 481, 465, 449, 433, 416, 400, 384, 367, 350, 333, 316, 299, 282, 265, 248,
    230, 213, 195, 178, 160, 143, 125, 107, 89, 71, 54, 36, 18, 0, -18, -36, -54, -71, -89, -107,
    -125, -143, -160, -178, -195, -213, -230, -248, -265, -282, -299, -316, -333, -350, -367, -384,
    -400, -416, -433, -449, -465, -481, -496, -512, -527, -543, -558, -573, -587, -602, -616, -630,
    -644, -658, -672, -685, -698, -711, -724, -737, -749, -761, -773, -784, -796, -807, -818, -828,
    -839, -849, -859, -868, -878, -887, -896, -904, -912, -920, -928, -935, -943, -949, -956, -962,
    -968, -974, -979, -984, -989, -994, -998, -1002, -1005, -1008, -1011, -1014, -1016, -1018,
    -1020, -1022, -1023, -1023, -1024, -1024, -1024, -1023, -1023, -1022, -1020, -1018, -1016,
    -1014, -1011, -1008, -1005, -1002, -998, -994, -989, -984, -979, -974, -968, -962, -956, -949,
    -943, -935, -928, -920, -912, -904, -896, -887, -878, -868, -859, -849, -839, -828, -818, -807,
    -796, -784, -773, -761, -749, -737, -724, -711, -698, -685, -672, -658, -644, -630, -616, -602,
    -587, -573, -558, -543, -527, -512, -496, -481, -465, -449, -433, -416, -400, -384, -367, -350,
    -333, -316, -299, -282, -265, -248, -230, -213, -195, -178, -160, -143, -125, -107, -89, -71,
    -54, -36, -18,
];

#[inline(always)]
fn sin_cos_scaled(angle_deg: u16) -> (i32, i32) {
    let a = (angle_deg as usize) % 360;
    let sin = SIN_DEG[a] as i32;
    let cos = SIN_DEG[(a + 90) % 360] as i32;
    (sin, cos)
}

fn draw_rotated_square_outline(
    fb: &mut FramebufferSurface,
    origin_x: usize,
    origin_y: usize,
    size: usize,
    angle_deg: u16,
    color: u32,
) {
    let half = (((size as i32 - 1) / 2) - 1).max(1);
    let (sin, cos) = sin_cos_scaled(angle_deg);
    let cx = origin_x as i32 + (size as i32 / 2);
    let cy = origin_y as i32 + (size as i32 / 2);
    let corners = [(-half, -half), (half, -half), (half, half), (-half, half)];
    let mut pts: [(i32, i32); 4] = [(0, 0); 4];
    for (i, (dx, dy)) in corners.iter().copied().enumerate() {
        let rx = (dx * cos - dy * sin) / TRIG_SCALE;
        let ry = (dx * sin + dy * cos) / TRIG_SCALE;
        pts[i] = (cx + rx, cy + ry);
    }

    for i in 0..4 {
        let (x0, y0) = pts[i];
        let (x1, y1) = pts[(i + 1) % 4];
        fb.draw_line(x0, y0, x1, y1, color);
    }
}

pub fn render_header_indicator(
    slot: usize,
    total_slots: usize,
    color: u32,
    angle_deg: u16,
) -> bool {
    if let Some(mut guard) = FRAMEBUFFER.try_lock() {
        if let Some(fb) = guard.as_mut() {
            let w = fb.width;
            let block = HEADER_INDICATOR_SIZE;
            let step = block;
            let total_slots = total_slots.max(1);
            let strip_w = total_slots.saturating_mul(block);
            let start_x = (w / 2).saturating_sub(strip_w / 2);
            let x = start_x.saturating_add(slot.saturating_mul(step));
            let y = 0;
            if x < w {
                let max_h = header_height().min(fb.height);
                let bh = block.min(max_h.saturating_sub(y));
                if bh > 0 {
                    let bw = block.min(w.saturating_sub(x));
                    let size = bw.min(bh);
                    fb.clear_rect(x, y, bw, bh, 0x0000_0000);
                    draw_rotated_square_outline(fb, x, y, size, angle_deg, color);
                    return true;
                }
            }
            return false;
        }
    }
    true
}

pub fn try_framebuffer_dimensions() -> Option<(u32, u32)> {
    FRAMEBUFFER
        .try_lock()
        .and_then(|guard| guard.as_ref().map(|fb| (fb.width as u32, fb.height as u32)))
}
