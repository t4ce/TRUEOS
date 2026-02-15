use alloc::vec::Vec;
use embassy_time::{Duration, Timer};
use crate::shell::{CommandAction, ShellBackend};
use crate::shell::cmd::registry::{ParsedArgs, ShellCommandCtx};

fn braille_from_mask(mask: u8) -> char {
    char::from_u32(0x2800 + mask as u32).unwrap()
}

fn braille_sliding_run(n: u8) -> Vec<char> {
    assert!((1..=8).contains(&n));
    let n = n as u32;
    (0..=8 - n)
        .map(|start| {
            let mask = ((1u16 << n) - 1) << start;
            braille_from_mask(mask as u8)
        })
        .collect()
}

fn braille_increasing_density_seeded(seed: u8) -> Vec<char> {
    (1..=8)
        .map(|n| {
            let x = seed.wrapping_mul(73).wrapping_add((n as u8).wrapping_mul(41));
            let perm = x ^ x.rotate_left(3) ^ x.rotate_right(2);
            let mask = if n == 8 { 0xFF } else { (1u16 << n) as u8 - 1 };
            braille_from_mask(perm & mask)
        })
        .collect()
}

pub(crate) fn cmd_rain(_ctx: &mut ShellCommandCtx<'_>, _: Option<&ParsedArgs<'_>>) -> CommandAction {
    CommandAction::EnterRain
}

struct RainDrop {
    col: usize,
    row: usize,
    sequence: Vec<char>,
    seq_idx: usize,
    forward: bool,
    trail_len: usize,
    color: (u8, u8, u8),
}

struct SimpleRng {
    state: u64,
}

impl SimpleRng {
    fn new(seed: u64) -> Self {
        Self { state: seed | 1 } // Ensure non-zero
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
        if count == 0 { return min; }
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
    io.write_str(crate::ecma48::CLEAR_SCREEN);

    let mut drops: Vec<RainDrop> = Vec::new();
    let seed = crate::time::unix_time_seconds().unwrap_or(12345);
    let mut rng = SimpleRng::new(seed);

    // Default color requested: (255, 55, 255) - Magenta/Pinkish
    let base_color = (255, 55, 255);

    loop {
        // Check for input to exit
        if io.read_byte().is_some() {
            break;
        }

        // Spawn 1 or 2 new drops
        let spawn_count = rng.gen_range(1, 2);
        for _ in 0..spawn_count {
            let col = rng.gen_range(1, cols); 
            
            // Randomly choose sequence type
            let seq = if rng.gen_bool() {
                let n = rng.gen_range(1, 8) as u8;
                braille_sliding_run(n)
            } else {
                let seed = rng.gen_u8();
                braille_increasing_density_seeded(seed)
            };

            let trail_len = rng.gen_range(3, 5);

            drops.push(RainDrop {
                col,
                row: 1, // Start at row 1
                sequence: seq,
                seq_idx: 0,
                forward: true,
                trail_len,
                color: base_color,
            });
            
            // Note: We don't draw immediately on spawn, we let the main loop handle it consistently.
            // Or we can draw just the head. The main loop clears and redraws anyway.
        }
        
        // Wait a bit
        Timer::after(Duration::from_millis(50)).await;

        // Advance drops
        let mut i = 0;
        while i < drops.len() {
            let drop = &mut drops[i];

            // Clear the tail of the trail before advancing
            if drop.row > drop.trail_len {
                 let clear_row = drop.row - drop.trail_len;
                 if clear_row > 0 && clear_row <= rows {
                     io.write_fmt(format_args!("{} ", crate::ecma48::pos(clear_row, drop.col)));
                 }
            }

            // Update position
            drop.row += 1;
            
            // Update sequence index
            if drop.forward {
                if drop.seq_idx + 1 < drop.sequence.len() {
                    drop.seq_idx += 1;
                } else {
                    drop.forward = false;
                    if drop.seq_idx > 0 {
                        drop.seq_idx -= 1;
                    } 
                }
            } else {
                if drop.seq_idx > 0 {
                    drop.seq_idx -= 1;
                } else {
                     drop.forward = true;
                     drop.seq_idx += 1; 
                }
            }

            // Remove if head is far off screen (allowing trail to finish falling off?)
            // "UNTIL the sequence reaches the last visible row" usually implies head reaches bottom.
            // But visually it looks better if it falls off completely.
            if drop.row > rows + drop.trail_len {
                drops.swap_remove(i);
                continue;
            }

            // Draw Head and Trail
            if let Some(ch) = drop.sequence.get(drop.seq_idx) {
                let mut current_color = drop.color;

                for k in 0..=drop.trail_len {
                    // Check if segment is spatially valid (above head but on screen)
                    if drop.row <= k {
                        continue; 
                    }
                    let r = drop.row - k;

                    // Skip if off screen
                    if r < 1 || r > rows {
                        continue;
                    }

                    if k == 0 {
                        let mut buf = [0u8; 4];
                        let s = ch.encode_utf8(&mut buf);
                        // Head: White FG, with drop color as BG (cursor effect)
                        io.write_fmt(format_args!(
                            "{}{}", 
                            crate::ecma48::pos(r, drop.col),
                            crate::ecma48::style(s)
                                .fg((255, 255, 255))
                                .bg(drop.color)
                        ));
                    } else {
                        // Trail: Dimming
                        current_color.0 /= 2;
                        current_color.1 /= 2;
                        current_color.2 /= 2;
                        
                        let mut buf = [0u8; 4];
                        let s = ch.encode_utf8(&mut buf);
                        
                        io.write_fmt(format_args!(
                            "{}{}", 
                            crate::ecma48::pos(r, drop.col),
                            crate::ecma48::color(s, current_color)
                        ));
                    }
                }
            }

            i += 1;
        }
    }
    
    io.write_str(crate::ecma48::SHOW_CURSOR);
    io.write_str(crate::ecma48::RESET);
}


