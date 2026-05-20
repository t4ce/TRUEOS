//! Game Boy APU register shadow.
//!
//! Silent for now. The HDA-backed 4-bit renderer was too rough for boot audio;
//! keep the register surface so ROMs can run while audio gets a proper design.

use alloc::vec::Vec;

const NR52_INDEX: usize = (0xFF26 - 0xFF10) as usize;

pub struct Apu {
    regs: [u8; 0x30],
}

impl Apu {
    pub fn new() -> Self {
        let mut regs = [0u8; 0x30];
        regs[NR52_INDEX] = 0xF1;
        Self { regs }
    }

    pub fn read(&self, addr: u16) -> u8 {
        match addr {
            0xFF26 => self.regs[NR52_INDEX] | 0x70,
            0xFF10..=0xFF3F => self.regs[(addr - 0xFF10) as usize],
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, addr: u16, val: u8) {
        match addr {
            0xFF26 => {
                self.regs[NR52_INDEX] = (self.regs[NR52_INDEX] & 0x0F) | (val & 0x80);
            }
            0xFF10..=0xFF3F => {
                self.regs[(addr - 0xFF10) as usize] = val;
            }
            _ => {}
        }
    }

    pub fn step(&mut self, _t_cycles: u32) {}

    pub fn drain_samples_into(&mut self, _out: &mut Vec<i16>) {}
}
