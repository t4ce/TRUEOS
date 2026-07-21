//! The three fixed circuits fused into the TRUEGA firmware.
//!
//! This module is compiled and executed only by the Ubuntu firmware build. RustHDL
//! lowers the `#[hdl_gen]` kernels to Verilog; TRUEOS never links RustHDL and never
//! compiles hardware at runtime.

use rust_hdl::prelude::*;

pub const HEARTBEAT_REPLY: u32 = 0x5453_4154;

/// Build contract for one physical function slot.
///
/// The catalogue lives beside the circuits so the RTL selector, binary manifest, and
/// generated host bindings cannot acquire independent slot declarations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FunctionSpec {
    pub id: u16,
    pub rust_name: &'static str,
    pub rust_module: &'static str,
    pub signature: &'static str,
    pub input_bytes: u16,
    pub output_bytes: u16,
}

pub const FUNCTIONS: [FunctionSpec; trueos_fpga_abi::FUNCTION_COUNT] = [
    FunctionSpec {
        id: 0,
        rust_name: "LED_STEP_HEARTBEAT",
        rust_module: "led_step_heartbeat",
        signature: "led_step_heartbeat()->u32",
        input_bytes: 0,
        output_bytes: 4,
    },
    FunctionSpec {
        id: 1,
        rust_name: "ADD_U32",
        rust_module: "add_u32",
        signature: "add_u32(u32,u32)->u32",
        input_bytes: 8,
        output_bytes: 4,
    },
    FunctionSpec {
        id: 2,
        rust_name: "XOR_U32",
        rust_module: "xor_u32",
        signature: "xor_u32(u32,u32)->u32",
        input_bytes: 8,
        output_bytes: 4,
    },
];

/// Slot 0: rotate one visibly lit LED and return the protocol magic.
#[derive(LogicBlock, Default)]
pub struct Heartbeat {
    pub led_state: Signal<In, Bits<5>>,
    pub next_led: Signal<Out, Bits<5>>,
    pub result: Signal<Out, Bits<32>>,
}

impl Logic for Heartbeat {
    #[hdl_gen]
    fn update(&mut self) {
        // Keep exactly one LED lit.  The fallback also seeds the heartbeat after
        // reset or recovers from any non-one-hot value written through the debug
        // BAR.  State advances only when the surrounding VHDL retires slot 0.
        if self.led_state.val() == 0x01 {
            self.next_led.next = 0x02.into();
        } else if self.led_state.val() == 0x02 {
            self.next_led.next = 0x04.into();
        } else if self.led_state.val() == 0x04 {
            self.next_led.next = 0x08.into();
        } else if self.led_state.val() == 0x08 {
            self.next_led.next = 0x10.into();
        } else {
            self.next_led.next = 0x01.into();
        }
        self.result.next = 0x5453_4154.into();
    }
}

/// Slot 1: one 32-bit adder circuit.
#[derive(LogicBlock, Default)]
pub struct AddU32 {
    pub a: Signal<In, Bits<32>>,
    pub b: Signal<In, Bits<32>>,
    pub result: Signal<Out, Bits<32>>,
}

impl Logic for AddU32 {
    #[hdl_gen]
    fn update(&mut self) {
        self.result.next = self.a.val() + self.b.val();
    }
}

/// Slot 2: one 32-bit XOR circuit.
#[derive(LogicBlock, Default)]
pub struct XorU32 {
    pub a: Signal<In, Bits<32>>,
    pub b: Signal<In, Bits<32>>,
    pub result: Signal<Out, Bits<32>>,
}

impl Logic for XorU32 {
    #[hdl_gen]
    fn update(&mut self) {
        self.result.next = self.a.val() ^ self.b.val();
    }
}

/// Fixed slot selector around the three circuits.
///
/// This is a mux, not an instruction decoder: all three function circuits exist in the
/// bitstream at once. `function_id` selects which already-wired result is retired into
/// the work package by the surrounding VHDL handoff state machine.
#[derive(LogicBlock, Default)]
pub struct TruegaFunctions {
    pub function_id: Signal<In, Bits<16>>,
    pub arg0: Signal<In, Bits<32>>,
    pub arg1: Signal<In, Bits<32>>,
    pub led_state: Signal<In, Bits<5>>,
    pub next_led: Signal<Out, Bits<5>>,
    pub result: Signal<Out, Bits<32>>,
    pub required_input_bytes: Signal<Out, Bits<16>>,
    pub output_bytes: Signal<Out, Bits<16>>,
    pub valid: Signal<Out, Bit>,
    heartbeat: Heartbeat,
    add_u32: AddU32,
    xor_u32: XorU32,
}

impl Logic for TruegaFunctions {
    #[hdl_gen]
    fn update(&mut self) {
        self.heartbeat.led_state.next = self.led_state.val();
        self.add_u32.a.next = self.arg0.val();
        self.add_u32.b.next = self.arg1.val();
        self.xor_u32.a.next = self.arg0.val();
        self.xor_u32.b.next = self.arg1.val();

        self.result.next = 0.into();
        self.next_led.next = self.led_state.val();
        self.required_input_bytes.next = 0.into();
        self.output_bytes.next = 0.into();
        self.valid.next = false.into();

        if self.function_id.val() == 0 {
            self.result.next = self.heartbeat.result.val();
            self.next_led.next = self.heartbeat.next_led.val();
            self.output_bytes.next = 4.into();
            self.valid.next = true.into();
        } else if self.function_id.val() == 1 {
            self.result.next = self.add_u32.result.val();
            self.required_input_bytes.next = 8.into();
            self.output_bytes.next = 4.into();
            self.valid.next = true.into();
        } else if self.function_id.val() == 2 {
            self.result.next = self.xor_u32.result.val();
            self.required_input_bytes.next = 8.into();
            self.output_bytes.next = 4.into();
            self.valid.next = true.into();
        }
    }
}

pub fn generate() -> String {
    let mut firmware = TruegaFunctions::default();
    firmware.connect_all();
    generate_verilog(&firmware)
}

/// Software reference behavior used only by host-side generator tests.
#[cfg(test)]
pub mod reference {
    use super::HEARTBEAT_REPLY;

    pub const fn led_step_heartbeat(led_state: u8) -> (u8, u32) {
        let next = match led_state & 0x1f {
            0x01 => 0x02,
            0x02 => 0x04,
            0x04 => 0x08,
            0x08 => 0x10,
            _ => 0x01,
        };
        (next, HEARTBEAT_REPLY)
    }

    pub const fn add_u32(a: u32, b: u32) -> u32 {
        a.wrapping_add(b)
    }

    pub const fn xor_u32(a: u32, b: u32) -> u32 {
        a ^ b
    }
}

#[cfg(test)]
mod tests {
    use super::reference;

    #[test]
    fn reference_functions_are_exact() {
        assert_eq!(reference::led_step_heartbeat(0x01), (0x02, 0x5453_4154));
        assert_eq!(reference::led_step_heartbeat(0x10), (0x01, 0x5453_4154));
        assert_eq!(reference::led_step_heartbeat(0x00), (0x01, 0x5453_4154));
        assert_eq!(reference::add_u32(u32::MAX, 2), 1);
        assert_eq!(reference::xor_u32(0xAA55_AA55, 0xFFFF_0000), 0x55AA_AA55);
    }
}
