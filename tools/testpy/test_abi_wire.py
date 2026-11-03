#!/usr/bin/env python3
"""Compile the production ABI wire codec with extracted real ABI records."""

from pathlib import Path
import re
import subprocess
import tempfile


ROOT = Path(__file__).resolve().parents[2]
V_ABI = ROOT / "crates/trueos-v/src/bp_abi.rs"
KERNEL_ABI = ROOT / "src/r/keyboard.rs"
UI4_ABI = ROOT / "src/ui4/blueprint_ui4.rs"
WINDOW_ABI = ROOT / "src/ui4/blueprint_ui4/window_api.rs"
CODEC = ROOT / "src/r/io/abi_wire.rs"
GUEST_TRANSPORT = ROOT / "src/ui4/blueprint_ui4/guest_transport.rs"
VM_CALL = ROOT / "crates/trueos-vm/src/vmcall.rs"

RECORDS = [
    "TrueosUi4ContextMenuEvent",
    "TrueosUi4ContextMenuEventV2",
    "TrueosUi4CursorSource",
    "TrueosUi4CursorStep",
    "TrueosUi4WindowStateV1",
    "TrueosUi4PanEvent",
    "TrueosUi4ResizeEvent",
    "TrueosUi4PointerEvent",
    "TrueosUi4KeyboardState",
    "TrueosUi4InputRouteState",
    "MouseMotionCursorInfo",
    "MouseMotionCommand",
    "KeyboardControlDeviceInfo",
    "KeyboardControlCommand",
    "GamepadControlDeviceInfo",
    "GamepadControlCommand",
    "GamepadControlSnapshot",
    "TrueosHidCursorEvent",
    "TrueosKeyboardOutputEvent",
]

LOCAL_UI4_RECORDS = [
    "TrueosUi4ContextMenuEvent",
    "TrueosUi4ContextMenuEventV2",
    "TrueosUi4CursorSource",
    "TrueosUi4CursorStep",
    "TrueosUi4PanEvent",
    "TrueosUi4ResizeEvent",
    "TrueosUi4PointerEvent",
    "TrueosUi4KeyboardState",
    "TrueosUi4InputRouteState",
]


def extract_struct(source: str, name: str) -> str:
    match = re.search(
        rf"\b(?:pub(?:\s*\([^)]*\))?\s+)?struct\s+{re.escape(name)}\s*\{{",
        source,
    )
    if match is None:
        raise RuntimeError(f"could not find struct {name}")
    start = match.start()
    repr_start = source.rfind("#[repr(", 0, start)
    if repr_start < 0 or "}" in source[repr_start:start]:
        raise RuntimeError(f"struct {name} has no directly preceding repr attribute")
    repr_end = source.find("]", repr_start) + 1
    repr_attr = source[repr_start:repr_end]
    if repr_attr != "#[repr(C)]":
        raise RuntimeError(f"struct {name} has unexpected layout attribute {repr_attr!r}")
    depth = 0
    in_string = False
    escaped = False
    for index in range(match.end() - 1, len(source)):
        char = source[index]
        if in_string:
            if escaped:
                escaped = False
            elif char == "\\":
                escaped = True
            elif char == '"':
                in_string = False
            continue
        if char == '"':
            in_string = True
        elif char == "{":
            depth += 1
        elif char == "}":
            depth -= 1
            if depth == 0:
                body = source[start : index + 1]
                return repr_attr + "\n" + body
    raise RuntimeError(f"unterminated struct {name}")


def extract_constant(source: str, name: str) -> str:
    match = re.search(rf"^pub(?:\([^)]*\))? const {re.escape(name)}\b[^;]*;", source, re.M)
    if match is None:
        match = re.search(rf"^const {re.escape(name)}\b[^;]*;", source, re.M)
    if match is None:
        raise RuntimeError(f"could not find constant {name}")
    return match.group(0)


def main() -> None:
    v_source = V_ABI.read_text()
    kernel_source = KERNEL_ABI.read_text()
    ui4_source = UI4_ABI.read_text()
    window_source = WINDOW_ABI.read_text()
    guest_transport_source = GUEST_TRANSPORT.read_text()
    vm_call_source = VM_CALL.read_text()
    structs = "\n\n".join(extract_struct(v_source, record) for record in RECORDS)
    keyboard = extract_struct(kernel_source, "TrueosKeyboardOutputEvent")
    local_ui4 = "\n\n".join(
        extract_struct(ui4_source, record) for record in LOCAL_UI4_RECORDS
    )
    image_source = extract_struct(ui4_source, "BlueprintImageSourceInfo")
    window_state = extract_struct(window_source, "TrueosUi4WindowStateV1")
    context_menu_entry = extract_struct(ui4_source, "TrueosUi4ContextMenuEntry")
    parent_constants = "\n".join(
        extract_constant(ui4_source, name)
        for name in [
            "ERROR_INVALID",
            "ERROR_UI4",
            "IMAGE_SOURCE_READ_CHUNK_BYTES",
            "MAX_INPUT_ROUTES",
            "MAX_CONTEXT_MENU_LABEL_BYTES",
            "CONTEXT_MENU_WIRE_HEADER_BYTES",
            "CONTEXT_MENU_ENTRY_WIRE_HEADER_BYTES",
        ]
    )
    vm_ops = sorted(set(re.findall(r"trueos_vm::vmcall::(OP_[A-Z0-9_]+)", guest_transport_source)))
    vm_constants = "\n".join(
        re.search(rf"^pub const {name}: u32 = [^;]+;", vm_call_source, re.M).group(0)
        for name in vm_ops
    )
    generated = f'''#![allow(dead_code)]
extern crate self as trueos_vm;
extern crate self as v;
extern crate alloc;

use core::alloc::Layout;
use core::cell::Cell;
use core::sync::atomic::{{AtomicU32, AtomicU64, AtomicUsize, Ordering}};
use std::sync::Mutex;
use std::alloc::{{GlobalAlloc, System}};

struct CountingAllocator;
static TEST_LOCK: Mutex<()> = Mutex::new(());
std::thread_local! {{
    static TRACK_ALLOCATIONS: Cell<bool> = const {{ Cell::new(false) }};
    static THREAD_ALLOCATIONS: Cell<usize> = const {{ Cell::new(0) }};
}}
unsafe impl GlobalAlloc for CountingAllocator {{
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {{
        let _ = TRACK_ALLOCATIONS.try_with(|active| {{
            if active.get() {{
                let _ = THREAD_ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
            }}
        }});
        unsafe {{ System.alloc(layout) }}
    }}
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {{
        unsafe {{ System.dealloc(ptr, layout) }}
    }}
}}
#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

pub mod vmcall {{
    use super::{{AtomicU32, AtomicU64, AtomicUsize, Ordering}};
    pub const STATUS_OK: u32 = 0;
    pub const STATUS_BAD_ARG: u32 = 2;
    pub const PAYLOAD_CAP: usize = 160 * 1024 - 56;
    {vm_constants}
    const RESPONSE_CAP: usize = 256;
    static STATUS: AtomicU32 = AtomicU32::new(STATUS_OK);
    static DATA: AtomicU64 = AtomicU64::new(0);
    static RESPONSE_LEN: AtomicUsize = AtomicUsize::new(0);
    pub static CALLS: AtomicUsize = AtomicUsize::new(0);
    static RESPONSE: [AtomicU32; RESPONSE_CAP] = [const {{ AtomicU32::new(0) }}; RESPONSE_CAP];

    pub fn configure(status: u32, data: u64, bytes: &[u8], reported_len: usize) {{
        STATUS.store(status, Ordering::Relaxed);
        DATA.store(data, Ordering::Relaxed);
        let len = bytes.len().min(RESPONSE_CAP);
        for (index, byte) in bytes.iter().copied().take(len).enumerate() {{
            RESPONSE[index].store(u32::from(byte), Ordering::Relaxed);
        }}
        RESPONSE_LEN.store(reported_len, Ordering::Relaxed);
    }}

    pub fn call_with_payload_reported_len(
        _: u32,
        _: u64,
        _: u64,
        _: &[u8],
        out: &mut [u8],
    ) -> (u32, u64, usize) {{
        CALLS.fetch_add(1, Ordering::Relaxed);
        let len = RESPONSE_LEN.load(Ordering::Relaxed).min(out.len());
        for (index, byte) in out.iter_mut().take(len).enumerate() {{
            *byte = RESPONSE[index].load(Ordering::Relaxed) as u8;
        }}
        (
            STATUS.load(Ordering::Relaxed),
            DATA.load(Ordering::Relaxed),
            RESPONSE_LEN.load(Ordering::Relaxed),
        )
    }}

    pub fn call_with_payload(
        op: u32,
        arg0: u64,
        arg1: u64,
        request: &[u8],
        out: &mut [u8],
    ) -> (u32, u64) {{
        let (status, data, _) = call_with_payload_reported_len(op, arg0, arg1, request, out);
        (status, data)
    }}
}}

pub mod bp_abi {{
{structs}
}}

pub mod r {{
    pub mod io {{
        pub(crate) use crate::abi_wire;
    }}
    pub mod keyboard {{
{keyboard}
    }}
}}

pub mod ui4 {{
    pub mod blueprint_ui4 {{
{local_ui4}
{context_menu_entry}
{image_source}
{parent_constants}
        use crate::abi_wire::WireRecord;
        #[path = "{GUEST_TRANSPORT}"]
        pub mod guest_transport;

        pub(crate) fn test_guest_record<T: WireRecord, const N: usize>(
            op: u32,
            arg0: u64,
            arg1: u64,
            request: &[u8],
        ) -> Result<T, i32> {{
            guest_transport::guest_record::<T, N>(op, arg0, arg1, request)
        }}
        pub(crate) fn test_guest_pointer_event_take(
            window_id: u32,
        ) -> Result<TrueosUi4PointerEvent, i32> {{
            guest_transport::guest_pointer_event_take(window_id)
        }}
        pub(crate) fn test_guest_context_menu_event_take(
            window_id: u32,
        ) -> Result<TrueosUi4ContextMenuEvent, i32> {{
            guest_transport::guest_context_menu_event_take(window_id)
        }}
        pub mod window_api {{
{window_state}
        }}
        pub use window_api::TrueosUi4WindowStateV1;
    }}
}}

#[path = "{CODEC}"]
mod abi_wire;

#[cfg(test)]
mod tests {{
    use super::abi_wire::WireRecord;
    use super::bp_abi::*;
    use super::r::keyboard::TrueosKeyboardOutputEvent as KernelKeyboardOutputEvent;
    use super::ui4::blueprint_ui4::{{
        BlueprintImageSourceInfo,
        TrueosUi4ContextMenuEvent as LocalContextMenuEvent,
        TrueosUi4ContextMenuEventV2 as LocalContextMenuEventV2,
        TrueosUi4CursorSource as LocalCursorSource,
        TrueosUi4CursorStep as LocalCursorStep,
        TrueosUi4InputRouteState as LocalInputRouteState,
        TrueosUi4KeyboardState as LocalKeyboardState,
        TrueosUi4PanEvent as LocalPanEvent,
        TrueosUi4PointerEvent as LocalPointerEvent,
        TrueosUi4ResizeEvent as LocalResizeEvent,
        TrueosUi4WindowStateV1 as LocalWindowStateV1,
    }};

    fn arbitrary_round_trip<R: WireRecord>() {{
        let bytes: Vec<u8> = (0..R::WIRE_SIZE)
            .map(|index| (index as u8).wrapping_mul(73).wrapping_add(0xA5))
            .collect();
        let value = R::decode(&bytes).expect("full record decodes");
        let mut encoded = vec![0xCC; R::WIRE_SIZE + 3];
        assert!(value.encode(&mut encoded));
        let mut expected = bytes.clone();
        for (index, byte) in expected.iter_mut().enumerate() {{
            if !R::WIRE_FIELDS
                .iter()
                .any(|(offset, len)| *offset <= index && index < *offset + *len)
            {{
                *byte = 0;
            }}
        }}
        assert_eq!(&encoded[..R::WIRE_SIZE], expected);
        assert_eq!(&encoded[R::WIRE_SIZE..], &[0xCC; 3]);
        let decoded = R::decode(&encoded).expect("encoded record decodes");
        let mut again = vec![0xDD; R::WIRE_SIZE];
        assert!(decoded.encode(&mut again));
        assert_eq!(&encoded[..R::WIRE_SIZE], &again);

        assert!(R::decode(&bytes[..R::WIRE_SIZE - 1]).is_none());
        let mut short = vec![0x5A; R::WIRE_SIZE - 1];
        let before = short.clone();
        assert!(!value.encode(&mut short));
        assert_eq!(short, before, "failed short encode must not mutate output");
    }}

    #[test]
    fn all_declared_records_round_trip_arbitrary_scalar_bits() {{
        let _lock = super::TEST_LOCK.lock().unwrap();
        macro_rules! check {{ ($($ty:ty),+ $(,)?) => {{ $(arbitrary_round_trip::<$ty>();)+ }} }}
        check!(
            TrueosUi4ContextMenuEvent, TrueosUi4ContextMenuEventV2,
            TrueosUi4CursorSource, TrueosUi4CursorStep, TrueosUi4WindowStateV1,
            TrueosUi4PanEvent, TrueosUi4ResizeEvent, TrueosUi4PointerEvent,
            TrueosUi4KeyboardState, TrueosUi4InputRouteState,
            MouseMotionCursorInfo, MouseMotionCommand, KeyboardControlDeviceInfo,
            KeyboardControlCommand, GamepadControlDeviceInfo, GamepadControlCommand,
            GamepadControlSnapshot, TrueosHidCursorEvent, TrueosKeyboardOutputEvent,
            KernelKeyboardOutputEvent,
            LocalContextMenuEvent, LocalContextMenuEventV2,
            LocalCursorSource, LocalCursorStep, LocalWindowStateV1,
            LocalPanEvent, LocalResizeEvent, LocalPointerEvent,
            LocalKeyboardState, LocalInputRouteState, BlueprintImageSourceInfo,
        );
    }}

    #[test]
    fn implicit_tail_padding_is_zeroed_and_nan_payloads_survive() {{
        let _lock = super::TEST_LOCK.lock().unwrap();
        let mut bytes = [0xA7; TrueosHidCursorEvent::WIRE_SIZE];
        bytes[32..40].copy_from_slice(&0x7ff8_1234_5678_9abcu64.to_le_bytes());
        bytes[40..48].copy_from_slice(&0xfff8_0fed_cba9_8765u64.to_le_bytes());
        let decoded = TrueosHidCursorEvent::decode(&bytes).unwrap();
        assert!(decoded.x.is_nan());
        assert!(decoded.y.is_nan());
        assert_eq!(decoded.x.to_bits(), u64::from_le_bytes(bytes[32..40].try_into().unwrap()));
        assert_eq!(decoded.y.to_bits(), u64::from_le_bytes(bytes[40..48].try_into().unwrap()));
        assert!(decoded.encode(&mut bytes));
        assert_eq!(&bytes[52..], &[0; 4]);
    }}

    #[test]
    fn explicit_reserved_fields_are_preserved() {{
        let _lock = super::TEST_LOCK.lock().unwrap();
        let mut bytes = [0u8; TrueosUi4ContextMenuEventV2::WIRE_SIZE];
        bytes[28..32].copy_from_slice(&0xA1B2_C3D4u32.to_le_bytes());
        let decoded = TrueosUi4ContextMenuEventV2::decode(&bytes).unwrap();
        assert_eq!(decoded.reserved, 0xA1B2_C3D4);
        let mut encoded = [0u8; TrueosUi4ContextMenuEventV2::WIRE_SIZE];
        assert!(decoded.encode(&mut encoded));
        assert_eq!(&encoded[28..32], &0xA1B2_C3D4u32.to_le_bytes());
    }}

    #[test]
    fn guest_record_maps_transport_and_service_errors_without_writing_a_value() {{
        let _lock = super::TEST_LOCK.lock().unwrap();
        use super::ui4::blueprint_ui4::test_guest_record as guest_record;
        use super::ui4::blueprint_ui4::TrueosUi4ResizeEvent;
        super::vmcall::configure(super::vmcall::STATUS_BAD_ARG, 0, &[], 0);
        assert!(matches!(guest_record::<TrueosUi4ResizeEvent, 16>(1, 0, 0, &[]), Err(-6)));
        super::vmcall::configure(super::vmcall::STATUS_OK, (-17i64) as u64, &[], 0);
        assert!(matches!(guest_record::<TrueosUi4ResizeEvent, 16>(1, 0, 0, &[]), Err(-17)));
    }}

    #[test]
    fn guest_record_decodes_real_record_without_allocating() {{
        let _lock = super::TEST_LOCK.lock().unwrap();
        use super::abi_wire::WireRecord;
        use super::ui4::blueprint_ui4::test_guest_record as guest_record;
        use super::ui4::blueprint_ui4::TrueosUi4ResizeEvent;
        let expected = TrueosUi4ResizeEvent {{ old_width: 640, old_height: 480, width: 800, height: 600 }};
        let mut response = [0u8; TrueosUi4ResizeEvent::WIRE_SIZE];
        assert!(expected.encode(&mut response));
        super::vmcall::configure(
            super::vmcall::STATUS_OK,
            0,
            &response,
            TrueosUi4ResizeEvent::WIRE_SIZE,
        );
        let before = super::THREAD_ALLOCATIONS.with(super::Cell::get);
        super::TRACK_ALLOCATIONS.with(|active| active.set(true));
        let actual = guest_record::<TrueosUi4ResizeEvent, 16>(1, 2, 3, &[]).unwrap();
        super::TRACK_ALLOCATIONS.with(|active| active.set(false));
        let after = super::THREAD_ALLOCATIONS.with(super::Cell::get);
        assert_eq!(actual.width, expected.width);
        assert_eq!(actual.height, expected.height);
        assert_eq!(after, before, "guest_record must stay allocation-free");
    }}

    #[test]
    fn guest_record_rejects_short_and_oversized_successful_records() {{
        let _lock = super::TEST_LOCK.lock().unwrap();
        use super::abi_wire::WireRecord;
        use super::ui4::blueprint_ui4::test_guest_record as guest_record;
        use super::ui4::blueprint_ui4::TrueosUi4ResizeEvent;
        let expected = TrueosUi4ResizeEvent {{ old_width: 640, old_height: 480, width: 800, height: 600 }};
        let mut response = [0u8; TrueosUi4ResizeEvent::WIRE_SIZE];
        assert!(expected.encode(&mut response));

        super::vmcall::configure(super::vmcall::STATUS_OK, 0, &response, response.len() - 1);
        assert!(matches!(guest_record::<TrueosUi4ResizeEvent, 16>(1, 0, 0, &[]), Err(-6)));

        super::vmcall::configure(super::vmcall::STATUS_OK, 0, &response, response.len() + 1);
        assert!(matches!(guest_record::<TrueosUi4ResizeEvent, 16>(1, 0, 0, &[]), Err(-6)));
    }}

    #[test]
    fn guest_record_rejects_bad_sizes_before_calling_vmcall() {{
        let _lock = super::TEST_LOCK.lock().unwrap();
        use super::ui4::blueprint_ui4::test_guest_record as guest_record;
        use super::ui4::blueprint_ui4::TrueosUi4ResizeEvent;
        super::vmcall::configure(super::vmcall::STATUS_OK, 0, &[], 0);
        let before = super::vmcall::CALLS.load(super::Ordering::Relaxed);
        assert!(matches!(guest_record::<TrueosUi4ResizeEvent, 15>(1, 0, 0, &[]), Err(-1)));
        let oversized_request = vec![0; super::vmcall::PAYLOAD_CAP + 1];
        assert!(matches!(
            guest_record::<TrueosUi4ResizeEvent, 16>(1, 0, 0, &oversized_request),
            Err(-1)
        ));
        let after = super::vmcall::CALLS.load(super::Ordering::Relaxed);
        assert_eq!(after, before, "invalid records must not enter vmcall");
    }}

    #[test]
    fn compiled_guest_transport_wrappers_use_real_types_and_preserve_errors() {{
        let _lock = super::TEST_LOCK.lock().unwrap();
        use super::abi_wire::WireRecord;
        use super::ui4::blueprint_ui4::{{
            test_guest_context_menu_event_take, test_guest_pointer_event_take,
            TrueosUi4ContextMenuEvent, TrueosUi4PointerEvent,
        }};

        let pointer = TrueosUi4PointerEvent {{
            controller_id: 7,
            slot_id: 2,
            ep_target: 11,
            hid_kind: 1,
            x: 31,
            y: 47,
            local_x: -3,
            local_y: 4,
            dx: 5,
            dy: -6,
            wheel: 1,
            buttons_down: 1,
            buttons_pressed: 0,
            buttons_released: 0,
            combo_id: 9,
            vcursor: 1,
        }};
        let mut bytes = [0u8; TrueosUi4PointerEvent::WIRE_SIZE];
        assert!(pointer.encode(&mut bytes));
        super::vmcall::configure(
            super::vmcall::STATUS_OK,
            0,
            &bytes,
            TrueosUi4PointerEvent::WIRE_SIZE,
        );
        let decoded = test_guest_pointer_event_take(55).unwrap();
        assert_eq!(decoded.controller_id, 7);
        assert_eq!(decoded.local_x, -3);
        assert_eq!(decoded.vcursor, 1);

        super::vmcall::configure(super::vmcall::STATUS_OK, (-12i64) as u64, &[], 0);
        assert!(matches!(test_guest_context_menu_event_take(55), Err(-12)));

        let _ = core::mem::size_of::<TrueosUi4ContextMenuEvent>();
    }}
}}
'''
    with tempfile.TemporaryDirectory(prefix="trueos-abi-wire-") as temporary:
        harness = Path(temporary) / "harness.rs"
        binary = Path(temporary) / "abi_wire_tests"
        harness.write_text(generated)
        subprocess.run(
            ["rustc", "--edition=2024", "--test", str(harness), "-o", str(binary)],
            check=True,
        )
        subprocess.run([str(binary)], check=True)


if __name__ == "__main__":
    main()
