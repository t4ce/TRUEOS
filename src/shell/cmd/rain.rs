use alloc::vec::Vec;
use embassy_time::{Duration, Timer};
use crate::shell::{CommandAction, ShellBackend};
use crate::shell::cmd::registry::{ParsedArgs, ShellCommandCtx};

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
                if bit >= 0 && bit < 8 {
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
    io.write_str("\x1b[48;2;0;0;0m");
    io.write_str("\x1b[38;2;255;255;255m");
    io.write_str(crate::ecma48::CLEAR_SCREEN);

    let mut drops: Vec<RainDrop> = Vec::new();
    let seed = crate::time::unix_time_seconds().unwrap_or(12345);
    let mut rng = SimpleRng::new(seed);

    loop {
        if io.read_byte().is_some() {
            break;
        }

        let spawn_count = rng.gen_range(0, 1);
        for _ in 0..spawn_count {
            let col = rng.gen_range(1, cols); 
            
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
                row: 1,
                sequence: seq,
                seq_idx: 0,
                forward: true,
                trail_len,
            });
        }
        
        Timer::after(Duration::from_millis(67)).await;

        let mut i = 0;
        while i < drops.len() {
            // Only update ~1/3 of drops per tick
            if rng.gen_range(0, 3) != 0 {
                i += 1;
                continue;
            }

            let drop = &mut drops[i];

            if drop.row > drop.trail_len {
                 let clear_row = drop.row - drop.trail_len;
                 if clear_row > 0 && clear_row <= rows {
                     io.write_fmt(format_args!("{} ", crate::ecma48::pos(clear_row, drop.col)));
                 }
            }

            drop.row += 1;
            
            // Update sequence index (looping)
            drop.seq_idx = (drop.seq_idx + 1) % drop.sequence.len();

            if drop.row > rows + drop.trail_len {
                drops.swap_remove(i);
                continue;
            }

            for k in 0..=drop.trail_len {
                if drop.row <= k {
                    continue; 
                }
                let r = drop.row - k;

                if r < 1 || r > rows {
                    continue;
                }

                // Calculate index for this trail position (cycling backwards)
                let len = drop.sequence.len();
                let curr_idx = (drop.seq_idx + len - (k % len)) % len;
                
                if let Some(ch) = drop.sequence.get(curr_idx) {
                    if k == 0 {
                        // Leading character: 33% gray background (approx 85, 85, 85)
                         io.write_fmt(format_args!(
                            "{}\x1b[48;2;85;85;85m{}\x1b[48;2;0;0;0m", 
                            crate::ecma48::pos(r, drop.col),
                            ch
                        ));
                    } else {
                        io.write_fmt(format_args!(
                            "{}{}", 
                            crate::ecma48::pos(r, drop.col),
                            ch
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


