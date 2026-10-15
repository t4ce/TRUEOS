"""GDB command: source this file, then codex-records VM(0..63).

Read-only, bounded numeric snapshot of kernel-owned native-job diagnostics.
No inferior calls, allocation/locking, thread names, or guest payload reads.
Layout is x86_64 Rust layout verified against kernel5/6 JobDiagnostic::new
assembly (Arc header 16, Record size176, roster mutex/vector stride32).
Revalidate offsets after changes to Record/compiler/target; it is not an ABI.
Use a coordinated batch attach and `-ex detach` immediately after the command.
"""
import json

# Offsets include the ArcInner strong/weak header. The identity tuple's numeric
# thread ID is read, but its adjacent string pointer/content is never read.
FIELDS = {'job': 0x10, 'generation': 0x18, 'thread_id': 0x38,
          'carrier': 0x68, 'phase': 0x70, 'since_ms': 0x78,
          'boundary': 0x80, 'wait_object': 0x88, 'timeout_ms': 0x90,
          'progress_enabled': 0x98, 'progress_stage': 0xa0,
          'progress_since_ms': 0xa8, 'progress_sequence': 0xb0,
          'interrupts_enabled': 0xb8}


def kernel_pointer(value):
    return value >= 0xffff800000000000 and value <= 0xfffffffffffffff8 and value % 8 == 0


def snapshot(read, base, vm, limit=32):
    if not 0 <= vm < 64 or not 1 <= limit <= 32:
        raise ValueError('VM must be 0..63; limit must be 1..32')
    def word(address):
        return int.from_bytes(read(address, 8), 'little')
    roster = base + vm * 32
    if read(roster, 1) != b'\0':
        return {'vm': vm, 'roster': 'busy', 'records': []}
    capacity, pointer, length = [word(roster + offset) for offset in (8, 16, 24)]
    if length > capacity or capacity > 65536 or (length and not kernel_pointer(pointer)):
        raise ValueError('roster layout/bounds rejected')
    result = {'vm': vm, 'record_count': length, 'truncated': length > limit, 'records': []}
    for index in range(min(length, limit)):
        record = word(pointer + index * 8)
        if not kernel_pointer(record):
            raise ValueError('record pointer rejected')
        item = {name: word(record + offset) for name, offset in FIELDS.items()}
        if item['phase'] > 7 or item['progress_stage'] > 9 or item['progress_enabled'] > 1 or item['interrupts_enabled'] > 1:
            raise ValueError('record layout rejected')
        item['wait_object'] = hex(item['wait_object'])
        item['progress_snapshot_stable'] = item['progress_sequence'] % 2 == 0
        result['records'].append(item)
    return result


try:
    import gdb
except ImportError:
    gdb = None

if gdb is not None:
    class CodexRecords(gdb.Command):
        def __init__(self):
            super().__init__('codex-records', gdb.COMMAND_DATA)
        def invoke(self, argument, from_tty):
            vm = int(argument.strip() or '0')
            base = int(gdb.parse_and_eval("&'TRUEOS::r::blocking::diagnostics::RECORDS'"))
            inferior = gdb.selected_inferior()
            def read(address, size):
                return bytes(inferior.read_memory(address, size))
            try:
                gdb.write(json.dumps(snapshot(read, base, vm), sort_keys=True) + '\n')
            except (ValueError, gdb.MemoryError) as error:
                gdb.write(json.dumps({'error': str(error)}) + '\n')
    CodexRecords()
