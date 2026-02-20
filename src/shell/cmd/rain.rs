use crate::shell::cmd::registry::{ParsedArgs, ShellCommandCtx};
use crate::shell::{CommandAction, ShellBackend};
use alloc::vec::Vec;
use embassy_time::{Duration, Timer};

fn braille_from_mask(mask: u8) -> char {
    char::from_u32(0x2800 + mask as u32).unwrap()
}

fn braille_sliding_run(n: u8) -> Vec<char> {
    assert!((1..=8).contains(&n));
    let n = n as i32;
    ((1 - n)..=7)
        .map(|start| {
            let mut mask = 0u8;
            for i in 0..n {
                let bit = start + i;
                if (0..8).contains(&bit) {
                    mask |= 1 << bit;
                }
            }
            braille_from_mask(mask)
        })
        .filter(|&c| c != '\u{2800}')
        .collect()
}

fn braille_increasing_density_seeded(seed: u8) -> Vec<char> {
    (1..=8)
        .map(|n| {
            let x = seed
                .wrapping_mul(73)
                .wrapping_add((n as u8).wrapping_mul(41));
            let perm = x ^ x.rotate_left(3) ^ x.rotate_right(2);
            let mask = if n == 8 { 0xFF } else { (1u16 << n) as u8 - 1 };
            braille_from_mask(perm & mask)
        })
        .collect()
}

pub(crate) fn cmd_rain(
    _ctx: &mut ShellCommandCtx<'_>,
    _: Option<&ParsedArgs<'_>>,
) -> CommandAction {
    CommandAction::EnterRain
}

struct RainDrop {
    col: usize,
    row: usize,
    sequence: Vec<char>,
    seq_idx: usize,
    forward: bool,
    trail_len: usize,
    ticks_since_update: usize,
    pulse: u8,
    pulse_up: bool,
    revealed_count: u8,
}

struct SimpleRng {
    state: u64,
}

impl SimpleRng {
    fn new(seed: u64) -> Self {
        Self { state: seed | 1 }
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545F4914F6CDD1D)
    }

    fn gen_range(&mut self, min: usize, max: usize) -> usize {
        let count = max - min + 1;
        if count == 0 {
            return min;
        }
        (self.next_u64() as usize % count) + min
    }

    fn gen_bool(&mut self) -> bool {
        (self.next_u64() & 1) == 0
    }

    fn gen_u8(&mut self) -> u8 {
        self.next_u64() as u8
    }
}

pub(crate) async fn run(io: &dyn ShellBackend, cols: usize, rows: usize) {
    io.write_str(crate::ecma48::HIDE_CURSOR);
    io.write_str("\x1b[48;2;0;0;0m");
    io.write_str("\x1b[38;2;255;255;255m");
    io.write_str(crate::ecma48::CLEAR_SCREEN);

    let mut drops: Vec<RainDrop> = Vec::new();
    let seed = crate::time::unix_time_seconds().unwrap_or(12345);
    let mut rng = SimpleRng::new(seed);

    let (logo, logo_w, logo_h) = crate::vga::get_logo_buffer();
    let offset_x = cols.saturating_sub(logo_w) / 2;
    let offset_y = rows.saturating_sub(logo_h) / 2;

    // Track which pixels are permanently revealed
    let mut revealed = alloc::vec![false; logo.len()];

    // Don't draw initial background - start pitch black as requested

    loop {
        if io.read_byte().is_some() {
            break;
        }

        if drops.len() < 175 {
            let spawn_count = rng.gen_range(1, 2);
            for _ in 0..spawn_count {
                // Simple bell curve (triangular distribution)
                // Summing two random numbers creates a distribution peaked at the center
                let half_width = cols / 2;
                if half_width > 0 {
                    let r1 = rng.gen_range(0, half_width);
                    let r2 = rng.gen_range(0, half_width + (cols % 2)); // Handle odd widths
                    let col = 1 + r1 + r2; // 1-based index

                    if col > 0 && col <= cols {
                        let mut seq = if rng.gen_bool() {
                            let n = rng.gen_range(1, 8) as u8;
                            braille_sliding_run(n)
                        } else {
                            let seed = rng.gen_u8();
                            braille_increasing_density_seeded(seed)
                        };

                        let mut rev = seq.clone();
                        rev.reverse();
                        seq.extend(rev);

                        let trail_len = rng.gen_range(3, 5);
                        let start_row = rng.gen_range(11, 14); // Restricted top spawn as requested

                        drops.push(RainDrop {
                            col,
                            row: start_row,
                            sequence: seq,
                            seq_idx: 0,
                            forward: true,
                            trail_len,
                            ticks_since_update: 0,
                            pulse: 0,
                            pulse_up: true,
                            revealed_count: 0,
                        });
                    }
                }
            }
        }

        Timer::after(Duration::from_millis(33)).await;

        let mut i = 0;
        while i < drops.len() {
            let mut remove_drop = false;
            let mut hit_detected = false;
            let mut should_update = false;

            // Block 1: Check update necessity
            {
                let drop = &mut drops[i];
                drop.ticks_since_update += 1;
                should_update = drop.ticks_since_update >= 4 || rng.gen_range(0, 3) == 0;
            }

            if !should_update {
                i += 1;
                continue;
            }

            // Block 2: Logic update (mutable borrow scope)
            {
                let drop = &mut drops[i];
                drop.ticks_since_update = 0;

                // Pulse logic
                if drop.pulse_up {
                    if drop.pulse >= 48 {
                        drop.pulse_up = false;
                        drop.pulse = 46;
                    } else {
                        drop.pulse += 2;
                    }
                } else {
                    if drop.pulse == 0 {
                        drop.pulse_up = true;
                        drop.pulse = 2;
                    } else {
                        drop.pulse = drop.pulse.saturating_sub(2);
                    }
                }

                // Clear tail logic (background/revealed pixels)
                if drop.row > drop.trail_len {
                    // trail covers row - k where k in 0..=trail_len
                    // means it covers [row, row-1, ..., row-trail_len]
                    // so we need to clear (row - trail_len - 1)
                    if drop.row > drop.trail_len + 1 {
                        let clear_row = drop.row - drop.trail_len - 1;
                        if clear_row > 0 && clear_row <= rows {
                            let mut drawn_bg = false;
                            if clear_row > offset_y && clear_row <= offset_y + logo_h {
                                // Check overlap
                                if drop.col > offset_x && drop.col <= offset_x + logo_w {
                                    let ly = clear_row - 1 - offset_y;
                                    let lx = drop.col - 1 - offset_x;
                                    let idx = ly * logo_w + lx;

                                    if revealed[idx] {
                                        let val = logo[idx];
                                        let intensity = ((val >> 24) & 0xFF) as u8;
                                        if intensity > 0 {
                                            let r = ((val >> 16) & 0xFF) as u8;
                                            let g = ((val >> 8) & 0xFF) as u8;
                                            let b = (val & 0xFF) as u8;
                                            let r = ((r as u16 * intensity as u16) / 255) as u8;
                                            let g = ((g as u16 * intensity as u16) / 255) as u8;
                                            let b = ((b as u16 * intensity as u16) / 255) as u8;

                                            io.write_fmt(format_args!(
                                                "{}\x1b[38;2;{};{};{}m█",
                                                crate::ecma48::pos(clear_row, drop.col),
                                                r,
                                                g,
                                                b
                                            ));
                                            drawn_bg = true;
                                        }
                                    }
                                }
                            }
                            let min_row = 11;
                            if !drawn_bg && clear_row >= min_row {
                                io.write_fmt(format_args!(
                                    "{} ",
                                    crate::ecma48::pos(clear_row, drop.col)
                                ));
                            }
                        }
                    }
                }

                // Advance head
                drop.row += 1;

                // Collision Detection
                if drop.row > offset_y && drop.row <= offset_y + logo_h
                    && drop.col > offset_x && drop.col <= offset_x + logo_w {
                        let ly = drop.row - 1 - offset_y;
                        let lx = drop.col - 1 - offset_x;
                        let idx = ly * logo_w + lx;

                        let val = logo[idx];
                        let intensity = ((val >> 24) & 0xFF) as u8;

                        if intensity > 0 && !revealed[idx] {
                            // HIT!
                            revealed[idx] = true;
                            drop.revealed_count += 1;

                            // Draw revealed pixel
                            let r = ((val >> 16) & 0xFF) as u8;
                            let g = ((val >> 8) & 0xFF) as u8;
                            let b = (val & 0xFF) as u8;
                            let r = ((r as u16 * intensity as u16) / 255) as u8;
                            let g = ((g as u16 * intensity as u16) / 255) as u8;
                            let b = ((b as u16 * intensity as u16) / 255) as u8;

                            io.write_fmt(format_args!(
                                "{}\x1b[38;2;{};{};{}m█",
                                crate::ecma48::pos(drop.row, drop.col),
                                r,
                                g,
                                b
                            ));

                            if drop.revealed_count >= 2 {
                                hit_detected = true;
                                // Trail clearing is handled in the removal block below
                            }
                        }
                    }

                // Determine if we remove or update sequence
                let max_row = rows.saturating_sub(10);
                if hit_detected || drop.row > max_row + drop.trail_len {
                    remove_drop = true;
                    // Clear the remaining trail before removing
                    // We need to clear from k=0 because the head (k=0) might have been drawn in a previous step
                    // or if it's a hit, the head was just handled.
                    // But for "falling off bottom", we might need to clear everything including where the head *would* be if it were visible,
                    // or rather, we just need to clear the full extent of the drop's visual footprint.
                    for k in 0..=drop.trail_len {
                        let trail_r = drop.row.saturating_sub(k);
                        if trail_r > 0 && trail_r <= rows {
                            // Check if this position is a revealed pixel
                            let mut is_revealed_pixel = false;
                            if trail_r > offset_y
                                && trail_r <= offset_y + logo_h
                                && drop.col > offset_x
                                && drop.col <= offset_x + logo_w
                            {
                                let t_ly = trail_r - 1 - offset_y;
                                let t_lx = drop.col - 1 - offset_x;
                                let t_idx = t_ly * logo_w + t_lx;
                                if revealed[t_idx] {
                                    is_revealed_pixel = true;
                                    // It's a revealed pixel, redraw it correctly instead of clearing
                                    let t_val = logo[t_idx];
                                    let t_int = ((t_val >> 24) & 0xFF) as u8;
                                    if t_int > 0 {
                                        let tr = ((t_val >> 16) & 0xFF) as u8;
                                        let tg = ((t_val >> 8) & 0xFF) as u8;
                                        let tb = (t_val & 0xFF) as u8;
                                        let tr = ((tr as u16 * t_int as u16) / 255) as u8;
                                        let tg = ((tg as u16 * t_int as u16) / 255) as u8;
                                        let tb = ((tb as u16 * t_int as u16) / 255) as u8;
                                        io.write_fmt(format_args!(
                                            "{}\x1b[38;2;{};{};{}m█",
                                            crate::ecma48::pos(trail_r, drop.col),
                                            tr,
                                            tg,
                                            tb
                                        ));
                                    }
                                }
                            }

                            if !is_revealed_pixel {
                                io.write_fmt(format_args!(
                                    "{} ",
                                    crate::ecma48::pos(trail_r, drop.col)
                                ));
                            }
                        }
                    }
                } else {
                    drop.seq_idx = (drop.seq_idx + 1) % drop.sequence.len();
                }
            } // End of mutable borrow scope

            if remove_drop {
                drops.swap_remove(i);
                // Do not increment i, as swap_remove moves the last element to i.
            } else {
                // Block 3: Draw logic (new immutable borrow)
                let drop = &drops[i];
                for k in 0..=drop.trail_len {
                    if drop.row <= k {
                        continue;
                    }
                    let r = drop.row - k;
                    if r < 11 || r > rows {
                        continue;
                    } // Restricted top 10 rows

                    let len = drop.sequence.len();
                    let curr_idx = (drop.seq_idx + len - (k % len)) % len;

                    if let Some(ch) = drop.sequence.get(curr_idx) {
                        if k == 0 {
                            io.write_fmt(format_args!(
                                "{}\x1b[48;2;{};{};{}m{}{}",
                                crate::ecma48::pos(r, drop.col),
                                drop.pulse,
                                drop.pulse,
                                drop.pulse,
                                ch,
                                "\x1b[48;2;0;0;0m"
                            ));
                        } else {
                            let intensity = 255 - ((k as u32 * 255) / (drop.trail_len as u32 + 1));
                            io.write_fmt(format_args!(
                                "{}\x1b[38;2;{};{};{}m{}",
                                crate::ecma48::pos(r, drop.col),
                                intensity,
                                intensity,
                                intensity,
                                ch
                            ));
                        }
                    }
                }

                i += 1;
            }
        }
    }

    io.write_str(crate::ecma48::SHOW_CURSOR);
    io.write_str(crate::ecma48::RESET);
}
