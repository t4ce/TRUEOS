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
    ticks_since_update: usize,
    pulse: u8,
    pulse_up: bool,
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

        let spawn_count = rng.gen_range(2, 3);
        for _ in 0..spawn_count {
            let col = rng.gen_range(1, cols); 
            
            let mut seq = if rng.gen_bool() {
                let n = rng.gen_range(1, 8) as u8;
                braille_sliding_run(n)
            } else {
                let seed = rng.gen_u8();
                braille_increasing_density_seeded(seed)
            };
            
            // Mirror sequence: append the first part in reverse to make it loop smoothly
            // Original: [A, B, C, D] -> [A, B, C, D, D, C, B, A]
            // Or maybe [A, B, C, D, C, B] to avoid double peaks?
            // "copy in the first 8 chars in reverse" implies doubling length.
            let mut rev = seq.clone();
            rev.reverse();
            seq.extend(rev);

            let trail_len = rng.gen_range(3, 5);

            // Spawn randomly in the top quarter
            // But if we start below row 1, we should be careful not to draw glitches.
            // Actually, if we start lower, 'row' is just the head position.
            // The drawing loop handles visibility (r < 1 || r > rows).
            let start_row = rng.gen_range(1, rows / 4 + 1);

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
            });
        }
        
        Timer::after(Duration::from_millis(33)).await;

        let mut i = 0;
        while i < drops.len() {
            let drop = &mut drops[i];
            
            // Should update this drop?
            drop.ticks_since_update += 1;
            // 20% update rate OR force update if waiting too long
            // Actually user requested "randomly", 1/3 is approx 33%.
            // "if a seqence wasnt advanced for 4 steps it gets for sure advanced"
            let should_update = drop.ticks_since_update >= 4 || rng.gen_range(0, 3) == 0;

            if !should_update {
                i += 1;
                continue;
            }
            drop.ticks_since_update = 0;

            // Pulse logic: 0-48 steps of 2
            if drop.pulse_up {
                if drop.pulse >= 48 {
                    drop.pulse_up = false;
                    drop.pulse = 46;
                } else {
                    drop.pulse += 2;
                }
            } else {
                if drop.pulse <= 0 {
                    drop.pulse_up = true;
                    drop.pulse = 2;
                } else {
                    drop.pulse = drop.pulse.saturating_sub(2);
                }
            }

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
                         // Pulsing background for head
                         io.write_fmt(format_args!(
                            "{}\x1b[48;2;{};{};{}m{}{}", // pulse rgb
                            crate::ecma48::pos(r, drop.col),
                            drop.pulse, drop.pulse, drop.pulse,
                            ch,
                            "\x1b[48;2;0;0;0m" // reset bg
                        ));
                    } else {
                        // Trail fade: fade from white (255) to black (0) over trail_len
                        let intensity = 255 - ((k as u32 * 255) / (drop.trail_len as u32 + 1));
                        
                        io.write_fmt(format_args!(
                            "{}\x1b[38;2;{};{};{}m{}", 
                            crate::ecma48::pos(r, drop.col),
                            intensity, intensity, intensity,
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


