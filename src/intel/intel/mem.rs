// Memory / MI-command bring-up notes.
//
// Current evidence from the bring-up transcript is still only a partial
// `memory-proof`: streamout writes, result markers, and display scanout all hit
// expected GPU addresses.  Render bring-up now emits one source-level proof
// line after the warm buffers are mapped:
//
// `intel/render: memory-proof accepted=1 map=1 ggtt_invalidated=1 flush=all
//  readback=cpu-first-dword ring[...] context[...] batch[...] state[...]
//  vertex[...] result[...] streamout[...]`
//
// MI commands currently relevant to the proof ladder:
//
// - `MI_BATCH_BUFFER_START`: enters a batch; part of `batch-submit-proof`.
// - `MI_BATCH_BUFFER_END`: should retire the final marker; currently not fully
//   proven for 3D paths (`final_marker=0` in captures).
// - `MI_STORE_DATA_IMM`: writes proof markers into result memory.
// - `MI_LOAD_REGISTER_IMM`: programs small MMIO state needed by probes.
// - `MI_STORE_REGISTER_MEM`: future useful path for direct register snapshots.
// - `MI_FORCE_WAKEUP`: related to forcewake/MMIO proof, though current code
//   mostly uses explicit MMIO forcewake registers.
//
// Reference command names kept from the old notes:
// MI_NOOP
// MI_ARB_CHECK
// MI_ARB_ON_OFF
// MI_BATCH_BUFFER_START
// MI_BATCH_BUFFER_END
// MI_CONDITIONAL_BATCH_BUFFER_END
// MI_DISPLAY_FLIP
// MI_LOAD_SCAN_LINES_EXCL
// MI_LOAD_SCAN_LINES_INCL
// MI_MATH
// MI_REPORT_HEAD
// MI_STORE_DATA_IMM
// MI_ATOMIC
// MI_COPY_MEM_MEM
// MI_LOAD_REGISTER_REG
// MI_LOAD_REGISTER_MEM
// MI_STORE_REGISTER_MEM
// MI_SUSPEND_FLUSH
// MI_USER_INTERRUPT
// MI_WAIT_FOR_EVENT
// MI_SEMAPHORE_SIGNAL
// MI_SEMAPHORE_WAIT
// MI_FORCE_WAKEUP
