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
            let x = seed.wrapping_mul(73).wrapping_add(n as u8 * 41);
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

            drops.push(RainDrop {
                col,
                row: 1, // Start at row 1
                sequence: seq,
                seq_idx: 0,
                forward: true,
            });
            
            // Draw initial char
            if let Some(drop) = drops.last() {
                if let Some(ch) = drop.sequence.get(drop.seq_idx) {
                    io.write_fmt(format_args!("{}{}", crate::ecma48::pos(drop.row, drop.col), ch));
                }
            }
        }
        
        // Wait a bit
        Timer::after(Duration::from_millis(50)).await;

        // Advance drops
        let mut i = 0;
        while i < drops.len() {
            let drop = &mut drops[i];

            // Clear current position
            io.write_fmt(format_args!("{} ", crate::ecma48::pos(drop.row, drop.col)));

            // Update position
            drop.row += 1;
            
            // Update sequence index
            if drop.forward {
                if drop.seq_idx + 1 < drop.sequence.len() {
                    drop.seq_idx += 1;
                } else {
                    // Reached end, reverse
                    drop.forward = false;
                    if drop.seq_idx > 0 {
                        drop.seq_idx -= 1;
                    } 
                }
            } else {
                if drop.seq_idx > 0 {
                    drop.seq_idx -= 1;
                } else {
                     // Reached 0 going backwards. 
                     // Reset to forward for ping-pong effect or just keep oscillating?
                     // "the sequence runs backwards" implies one way trip back?
                     // Let's assume it bounces.
                     drop.forward = true;
                     drop.seq_idx += 1; 
                }
            }

            // Remove if off screen
            if drop.row > rows {
                drops.swap_remove(i);
                continue;
            }

            // Draw new char
            if let Some(ch) = drop.sequence.get(drop.seq_idx) {
                 // Use a color? Let's use Cyan.
                 io.write_fmt(format_args!("{}{}", crate::ecma48::pos(drop.row, drop.col), crate::ecma48::color(*ch, (100, 200, 255))));
            }

            i += 1;
        }
    }
    
    io.write_str(crate::ecma48::SHOW_CURSOR);
    io.write_str(crate::ecma48::RESET);
}


