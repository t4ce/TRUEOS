#!/usr/bin/env python3
"""One-shot source editor. Removed once the validated changes are committed."""
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
PATHS = ['src/log_os.rs', 'src/intel/gpu_font.rs',
         'src/intel/gpgpu/operations/primitives.rs',
         'src/intel/gpgpu/operations/submission_2d.rs',
         'src/intel/gpgpu/rcs/commands.rs']
S = {p: (ROOT / p).read_text() for p in PATHS}


def once(s, old, new):
    assert s.count(old) == 1, (old[:100], s.count(old))
    return s.replace(old, new, 1)


def edit_function(s, name, edit):
    m = list(re.finditer(r'(?m)^(?:pub(?:\([^\n]*\))? )?(?:async )?fn ' + name + r'\(', s))
    assert len(m) == 1, name
    start = m[0].start()
    end = s.index('\n}', start) + 2
    return s[:start] + edit(s[start:end]) + s[end:]


def annotate_returns(body, phase, reasons):
    pattern = r'(?m)^(\s*)return GpgpuDispatchRetirement::NotSubmitted;'
    matches = list(re.finditer(pattern, body))
    assert len(matches) == len(reasons), (phase, len(matches), len(reasons))
    for match, reason in reversed(list(zip(matches, reasons))):
        # Keep every validation predicate and its original return unchanged.
        indent = match.group(1)
        line = f'{indent}crate::log_font_warm_diag!("phase={phase} reject={reason}\\n");\n'
        body = body[:match.start()] + line + body[match.start():]
    return body


p = 'src/log_os.rs'
S[p] = once(S[p], '    pub(crate) const TGL_GPU_DIAG_PROFILE_ENABLED: bool = true;',
'''    pub(crate) const TGL_GPU_DIAG_PROFILE_ENABLED: bool = true;

    /// Compact font warm/coverage breadcrumbs, using the existing Gpgpu/Info
    /// policy and rate limiter. No Render/Info or screen-specific filter.
    pub(crate) const FONT_WARM_DIAG_PROFILE_ENABLED: bool = TGL_GPU_DIAG_PROFILE_ENABLED;''')
macro = '''/// Font warm-up breadcrumbs reach the ordinary TCP/UART policy first; the
/// MicroFont service mirrors those accepted bytes. Sample per callsite so a
/// busy/quarantined retry does not become another per-frame log stream.
#[macro_export]
macro_rules! log_font_warm_diag {
    ($($tt:tt)*) => {{
        if $crate::log_os::flags::FONT_WARM_DIAG_PROFILE_ENABLED {
            $crate::log_rate_limited!(
                target: "intel/gpgpu"; level: $crate::log_os::LogLevel::Info;
                first: 2; every: 128;
                "font-warm {}", format_args!($($tt)*)
            );
        }
    }};
}

'''
S[p] = once(S[p], '/// Rate-bounded record for the Shell2 -> UI4 -> FontKernel latency hunt.',
            macro + '/// Rate-bounded record for the Shell2 -> UI4 -> FontKernel latency hunt.')
S[p] = once(S[p], '    log_os_core::install_global_log_dispatch(&TRUEOS_LOG_ROUTER);',
'''    log_os_core::install_global_log_dispatch(&TRUEOS_LOG_ROUTER);
    crate::log_font_warm_diag!("phase=profile version=1 first=2 every=128 clip_budget=320\\n");''')

p = 'src/intel/gpu_font.rs'
def face(body):
    body = once(body, '    match crate::graphics::font::ensure_font_available(font.registry_name()) {',
'''    crate::log_font_warm_diag!("phase=raw-face-enter face={}\\n", font.registry_name());
    let result = match crate::graphics::font::ensure_font_available(font.registry_name()) {''')
    assert body.endswith('    }\n}')
    return body[:-7] + '''    };
    crate::log_font_warm_diag!("phase=raw-face-return face={} result={:?}\\n", font.registry_name(), result);
    result
}'''
S[p] = edit_function(S[p], 'ensure_font_face_available', face)
S[p] = once(S[p], '''        let mut retirement =
            crate::intel::gpgpu::font_outline_coverage_runs_r8(&storage, runs.as_slice());''',
'''        crate::log_font_warm_diag!("phase=coverage-request runs={}\\n", runs.len());
        let mut retirement =
            crate::intel::gpgpu::font_outline_coverage_runs_r8(&storage, runs.as_slice());
        crate::log_font_warm_diag!("phase=coverage-return result={:?} runs={}\\n", retirement, runs.len());''')
S[p] = once(S[p], '''            for run in &runs {
                retirement = crate::intel::gpgpu::font_outline_coverage_r8(''',
'''            crate::log_font_warm_diag!("phase=coverage-single-run-fallback runs={} cause=NotSubmitted\\n", runs.len());
            for run in &runs {
                retirement = crate::intel::gpgpu::font_outline_coverage_r8(''')

p = 'src/intel/gpgpu/operations/primitives.rs'
def primitive(body):
    body = annotate_returns(body, 'coverage-input', [
        'run-count', 'outline-or-rectangle-contract', 'ops-size-overflow',
        'ops-alignment-overflow', 'packed-size-overflow', 'ops-window-capacity', 'ops-dma-allocation'])
    return once(body, '    let surface = mask.surface();',
'''    let surface = mask.surface();
    crate::log_font_warm_diag!("phase=coverage-input runs={} mask={}x{} pitch={} gpu=0x{:X} bytes={}\\n",
        runs.len(), surface.width, surface.height, surface.pitch_bytes, surface.gpu, surface.bytes);''')
S[p] = edit_function(S[p], 'font_outline_coverage_runs_r8', primitive)

p = 'src/intel/gpgpu/operations/submission_2d.rs'
def coverage(body):
    body = annotate_returns(body, 'coverage-admission', [
        'run-count', 'mask-binding-offset', 'mask-binding-range', 'dispatch-shape',
        'ops-binding-offset', 'mask-size-overflow', 'run-binding-contract',
        'font-submit-lock-busy', 'no-claimed-device', 'coverage-kernel-upload', 'font-context'])
    # A context rejection after an earlier timeout is different from cold allocation failure.
    body = once(body, 'crate::log_font_warm_diag!("phase=coverage-admission reject=font-context\\n");',
        'crate::log_font_warm_diag!("phase=coverage-admission reject=font-context quarantined={}\\n", font_rcs_context_is_quarantined() as u8);')
    body = once(body, '    let forcewake_ok = direct_rcs_forcewake(dev);',
'''    crate::log_font_warm_diag!("phase=coverage-ready device=0x{:04X} rev=0x{:02X} kernel_gpu=0x{:X} kernel_bytes={} ops_bytes={}\\n",
        dev.device_id, dev.revision_id, upload.gpu, upload.mapped_bytes, ops_bytes);
    let forcewake_ok = direct_rcs_forcewake(dev);''')
    body = once(body, '    let submission = if batch_ok {',
'''    if !batch_ok {
        crate::log_font_warm_diag!("phase=coverage-prepare-failed fw={} control={} ppgtt={} kernel={} ops={} mask={} batch={}\\n",
            forcewake_ok as u8, mapped_ok as u8, ppgtt_ok as u8, kernel_ppgtt_ok as u8,
            ops_ppgtt_ok as u8, mask_ppgtt_ok as u8, batch_ok as u8);
    } else {
        crate::log_font_warm_diag!("phase=coverage-prepared runs={} ops={} groups={} batch_gpu=0x{:X} result_gpu=0x{:X}\\n",
            runs.len(), total_ops, total_groups, state.gpu_va.batch, state.gpu_va.result);
    }
    let submission = if batch_ok {''')
    body = once(body, '    let submitted = submission.may_have_submitted();',
'''    crate::log_font_warm_diag!("phase=coverage-submit state={:?} poll={} timeout_ms={}\\n",
        submission, submission.can_poll() as u8, FONT_OUTLINE_COVERAGE_R8_COMPLETION_TIMEOUT_MS);
    let submitted = submission.may_have_submitted();''')
    body = once(body, '    let completed = observed == COPY_RECT_POST_MARKER;',
'''    let completed = observed == COPY_RECT_POST_MARKER;
    if completed {
        crate::log_font_warm_diag!("phase=coverage-marker post=0x{:08X} result=Complete\\n", observed);
    } else {
        crate::log_font_warm_diag!("phase=coverage-marker state={:?} batch={} pre=0x{:08X}/0x{:08X} post=0x{:08X}/0x{:08X} quarantined={}\\n",
            submission, batch_ok as u8,
            if batch_ok { direct_rcs_read_result_slot(state, COPY_RECT_PRE_MARKER_SLOT) } else { 0 },
            COPY_RECT_PRE_MARKER, observed, COPY_RECT_POST_MARKER, font_rcs_context_is_quarantined() as u8);
    }''')
    return body
S[p] = edit_function(S[p], 'submit_font_outline_coverage_runs_r8_mapped_2d', coverage)

p = 'src/intel/gpgpu/rcs/commands.rs'
def submit(body):
    body = once(body, '''    if quarantined.load(Ordering::Acquire) {
        return DirectRcsSubmissionState::Rejected;''',
'''    if quarantined.load(Ordering::Acquire) {
        if lane == DirectRcsLane::Font {
            crate::log_font_warm_diag!("phase=font-rcs-submit reject=quarantined\\n");
        }
        return DirectRcsSubmissionState::Rejected;''')
    body = once(body, '''        let mut runtime = runtime.lock();
        direct_rcs_submit_batch_with_runtime_inner''',
'''        let mut runtime = runtime.lock();
        if lane == DirectRcsLane::Font {
            crate::log_font_warm_diag!("phase=font-rcs-enter pending={} initialized={} seq={} tail={}\\n",
                runtime.pending.is_some() as u8, runtime.context_initialized as u8,
                runtime.submissions, runtime.ring_tail_bytes);
        }
        direct_rcs_submit_batch_with_runtime_inner''')
    body = once(body, '    match attempt {',
'''    if lane == DirectRcsLane::Font {
        let outcome = match &attempt {
            DirectRcsSubmitAttempt::Submitted(_) => "Submitted",
            DirectRcsSubmitAttempt::Deferred => "Deferred",
            DirectRcsSubmitAttempt::Rejected => "Rejected",
            DirectRcsSubmitAttempt::Ambiguous { .. } => "Ambiguous",
        };
        crate::log_font_warm_diag!("phase=font-rcs-attempt outcome={}\\n", outcome);
    }
    match attempt {''')
    return body
S[p] = edit_function(S[p], 'direct_rcs_submit_batch_on_lane_state', submit)

for p, content in S.items():
    (ROOT / p).write_text(content)
    print('updated', p)
