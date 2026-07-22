//! Game Boy timer — DIV, TIMA, TMA, and TAC.
//!
//! The timer is driven by the falling edge of a bit in the internal 16-bit
//! system counter.  This module is stepped in M-cycles, so the counter advances
//! by four T-cycles for every cycle passed to [`Timer::step`].

pub struct Timer {
    pub div: u16,        // Internal 16-bit divider (upper byte readable at $FF04)
    pub tima: u8,        // Timer counter ($FF05)
    pub tma: u8,         // Timer modulo ($FF06)
    pub tac: u8,         // Timer control ($FF07), low three bits only
    pub interrupt: bool, // Timer overflow interrupt request
    overflow_cycles: u8, // M-cycles until TIMA reload/interrupt
}

impl Timer {
    pub fn new() -> Self {
        Self {
            // Common post-DMG-boot value.  Software must not rely on the exact
            // power-on phase, but keeping it deterministic makes ROM behavior
            // and tests repeatable.
            div: 0xABCC,
            tima: 0,
            tma: 0,
            tac: 0,
            interrupt: false,
            overflow_cycles: 0,
        }
    }

    /// Advance the system timer by `m_cycles` CPU machine cycles.
    pub fn step(&mut self, m_cycles: u32) {
        for _ in 0..m_cycles {
            // Hardware exposes TIMA=$00 for one M-cycle after overflow.  The
            // modulo reload and IF request happen at the start of the next one.
            if self.overflow_cycles != 0 {
                self.overflow_cycles -= 1;
                if self.overflow_cycles == 0 {
                    self.tima = self.tma;
                    self.interrupt = true;
                }
            }

            let old_signal = self.timer_input();
            self.div = self.div.wrapping_add(4);
            let new_signal = self.timer_input();
            if old_signal && !new_signal {
                self.increment_tima();
            }
        }
    }

    pub fn read_div(&self) -> u8 {
        (self.div >> 8) as u8
    }

    pub fn read_tac(&self) -> u8 {
        self.tac | 0xF8
    }

    /// Reset DIV.  On a DMG this can create the timer's selected falling edge.
    pub fn write_div(&mut self) {
        let old_signal = self.timer_input();
        self.div = 0;
        if old_signal && !self.timer_input() {
            self.increment_tima();
        }
    }

    /// A TIMA write during the overflow wait cycle cancels the pending reload.
    pub fn write_tima(&mut self, value: u8) {
        self.tima = value;
        self.overflow_cycles = 0;
    }

    pub fn write_tma(&mut self, value: u8) {
        self.tma = value;
    }

    /// Change TAC, including the DMG falling-edge behavior caused by a clock
    /// source change or by disabling an active timer input.
    pub fn write_tac(&mut self, value: u8) {
        let old_signal = self.timer_input();
        self.tac = value & 0x07;
        if old_signal && !self.timer_input() {
            self.increment_tima();
        }
    }

    fn timer_input(&self) -> bool {
        if self.tac & 0x04 == 0 {
            return false;
        }

        let bit = match self.tac & 0x03 {
            0 => 9, // 4096 Hz
            1 => 3, // 262144 Hz
            2 => 5, // 65536 Hz
            3 => 7, // 16384 Hz
            _ => unreachable!(),
        };
        self.div & (1 << bit) != 0
    }

    fn increment_tima(&mut self) {
        // Further timer edges do not alter TIMA while an overflow reload is
        // pending.  The shortest selectable period is four M-cycles, so this is
        // mostly documentary, but it keeps the state transition unambiguous.
        if self.overflow_cycles != 0 {
            return;
        }

        let (value, overflowed) = self.tima.overflowing_add(1);
        self.tima = value;
        if overflowed {
            self.overflow_cycles = 1;
        }
    }
}

impl Default for Timer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::Timer;

    fn timer_at_zero(tac: u8) -> Timer {
        let mut timer = Timer::new();
        timer.div = 0;
        timer.write_tac(tac);
        timer
    }

    #[test]
    fn selected_divider_bit_ticks_at_the_documented_rate() {
        let mut timer = timer_at_zero(0x05);

        timer.step(3);
        assert_eq!(timer.tima, 0);
        timer.step(1);
        assert_eq!(timer.tima, 1);

        timer.step(4);
        assert_eq!(timer.tima, 2);
    }

    #[test]
    fn overflow_is_visible_for_one_m_cycle_before_reload() {
        let mut timer = timer_at_zero(0x05);
        timer.div = 12;
        timer.tima = 0xFF;
        timer.tma = 0x42;

        timer.step(1);
        assert_eq!(timer.tima, 0);
        assert!(!timer.interrupt);

        timer.step(1);
        assert_eq!(timer.tima, 0x42);
        assert!(timer.interrupt);
    }

    #[test]
    fn tima_write_cancels_a_pending_reload() {
        let mut timer = timer_at_zero(0x05);
        timer.div = 12;
        timer.tima = 0xFF;
        timer.tma = 0x42;

        timer.step(1);
        timer.write_tima(0x77);
        timer.step(1);

        assert_eq!(timer.tima, 0x77);
        assert!(!timer.interrupt);
    }

    #[test]
    fn div_reset_and_tac_change_can_create_falling_edges() {
        let mut timer = timer_at_zero(0x05);
        timer.div = 8;
        timer.write_div();
        assert_eq!(timer.tima, 1);

        timer.div = 8;
        timer.write_tac(0x04);
        assert_eq!(timer.tima, 2);
    }
}
