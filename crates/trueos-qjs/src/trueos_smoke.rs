extern crate alloc;

use core::ffi::{c_char, c_int, CStr};

use crate as qjs;

extern "C" {
    fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
    fn trueos_cabi_poll_once();
}

unsafe fn drain_pending_jobs(rt: *mut qjs::JSRuntime, fallback_ctx: *mut qjs::JSContext) -> bool {
    if rt.is_null() {
        return true;
    }

    loop {
        let mut job_ctx: *mut qjs::JSContext = core::ptr::null_mut();
        let rc = qjs::JS_ExecutePendingJob(rt, &mut job_ctx as *mut *mut qjs::JSContext);
        if rc > 0 {
            continue;
        }
        if rc < 0 {
            let ctx = if !job_ctx.is_null() {
                job_ctx
            } else {
                fallback_ctx
            };
            if !ctx.is_null() {
                log_str("quickjs: exception while executing pending job\n");
                dump_exception(ctx);
            } else {
                log_str("quickjs: exception while executing pending job (no ctx)\n");
            }
            return false;
        }
        break;
    }

    true
}

/// Drive QuickJS microtasks + TRUEOS async ops (net/FS fetches used by the module loader).
///
/// Unlike the shell REPL, boot smokes are not async; they must explicitly yield to the kernel
/// via `trueos_cabi_poll_once()` while waiting for completions.
unsafe fn drain_jobs_and_promises(rt: *mut qjs::JSRuntime, ctx: *mut qjs::JSContext, max_wait_ms: u64) -> bool {
    if rt.is_null() || ctx.is_null() {
        return true;
    }

    let start = embassy_time_driver::now();
    let hz = embassy_time_driver::TICK_HZ as u64;
    let max_ticks = if max_wait_ms == 0 || hz == 0 {
        0
    } else {
        (max_wait_ms.saturating_mul(hz) + 999) / 1000
    };
    let deadline = start.saturating_add(max_ticks);

    let mut last_status_tick: u64 = start;
    loop {
        let mut progress = false;
        progress |= qjs::async_ops::pump(ctx);
        progress |= qjs::workers::pump(ctx);

        if !drain_pending_jobs(rt, ctx) {
            return false;
        }

        let pending_async = qjs::async_ops::has_pending(ctx);
        let pending_workers = qjs::workers::has_pending_for_ctx(ctx);
        let jobs_pending = qjs::JS_IsJobPending(rt) > 0;
        if !pending_async && !pending_workers && !jobs_pending {
            break;
        }

        // Coarse status: once per ~1s while we're still waiting.
        if hz != 0 {
            let now_tick = embassy_time_driver::now();
            let one_sec = hz;
            if now_tick.saturating_sub(last_status_tick) >= one_sec {
                last_status_tick = now_tick;
                log_str("quickjs: waiting async=");
                log_str(if pending_async { "1" } else { "0" });
                log_str(" workers=");
                log_str(if pending_workers { "1" } else { "0" });
                log_str(" jobs=");
                log_str(if jobs_pending { "1" } else { "0" });
                log_str("\n");
            }
        }

        if max_ticks != 0 && embassy_time_driver::now() >= deadline {
            log_str("quickjs: async wait timeout\n");
            break;
        }

        // Yield to the kernel so async net/FS tasks can run.
        // We do this even when we made progress to avoid starving the executor.
        trueos_cabi_poll_once();

        // If we are spinning but nothing is happening, keep yielding.
        if !progress {
            trueos_cabi_poll_once();
        }
    }

    true
}

#[inline]
fn log_bytes(bytes: &[u8]) {
    if bytes.is_empty() {
        return;
    }
    unsafe { trueos_cabi_write(1, bytes.as_ptr(), bytes.len()) };
}

#[inline]
fn log_str(s: &str) {
    log_bytes(s.as_bytes());
}

#[inline]
fn log_nl() {
    log_bytes(b"\n");
}

unsafe fn dump_exception(ctx: *mut qjs::JSContext) {
    qjs::qjs_diag::dump_last_exception(ctx, "exception");
}

unsafe fn install_print(ctx: *mut qjs::JSContext) {
    unsafe extern "C" fn qjs_print(
        ctx: *mut qjs::JSContext,
        _this_val: qjs::JSValueConst,
        argc: c_int,
        argv: *const qjs::JSValueConst,
    ) -> qjs::JSValue {
        log_str("qjs: ");
        if !argv.is_null() && argc > 0 {
            let args = core::slice::from_raw_parts(argv, argc as usize);
            for (i, arg) in args.iter().enumerate() {
                if i != 0 {
                    log_str(" ");
                }
                let cstr = qjs::js_to_cstring(ctx, *arg);
                if cstr.is_null() {
                    log_str("<toString failed>");
                    continue;
                }
                let bytes = CStr::from_ptr(cstr).to_bytes();
                if let Ok(s) = core::str::from_utf8(bytes) {
                    log_str(s);
                } else {
                    log_bytes(bytes);
                }
                qjs::JS_FreeCString(ctx, cstr);
            }
        }
        log_nl();
        qjs::JSValue::undefined()
    }

    let global = qjs::JS_GetGlobalObject(ctx);
    let name = b"print\0";
    let func = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_print),
        name.as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(ctx, global, name.as_ptr() as *const c_char, func);

    // Stable alias so libraries can’t clobber our primary log hook.
    let alias = b"__trueos_print\0";
    let func2 = qjs::JS_NewCFunction2(
        ctx,
        Some(qjs_print),
        alias.as_ptr() as *const c_char,
        1,
        qjs::JS_CFUNC_GENERIC,
        0,
    );
    let _ = qjs::JS_SetPropertyStr(ctx, global, alias.as_ptr() as *const c_char, func2);
    qjs::js_free_value(ctx, global);
}

/// TRUEOS kernel QuickJS smoke test:
/// - Installs a minimal `print()` bridge.
/// - Installs a module loader that serves a native `complex` module.
/// - Evaluates an ES module that imports `complex` and asserts add/square results.
pub unsafe fn run() {
    let Some(vm) = qjs::vm::QjsVm::new_node() else {
        log_str("quickjs: JS_NewRuntime failed\n");
        return;
    };
    let rt = vm.rt_ptr();
    let ctx = vm.ctx_ptr();

    install_print(ctx);
    qjs::node::install_globals(ctx);

    let mod_filename = b"<smoke-module>\0";
    let mod_script = b"import proc, { argv, env, cwd, hrtime, nextTick, uptime } from 'node:process';\n\
if (proc !== globalThis.process) throw new Error('process global mismatch');\n\
if (!Array.isArray(argv)) throw new Error('process.argv not array');\n\
if (typeof env !== 'object' || env === null) throw new Error('process.env not object');\n\
if (typeof cwd() !== 'string') throw new Error('process.cwd not string');\n\
if (typeof uptime() !== 'number') throw new Error('process.uptime not number');\n\
const ht = hrtime();\n\
if (!Array.isArray(ht) || ht.length !== 2) throw new Error('process.hrtime not [s,ns]');\n\
nextTick(() => globalThis.print('nextTick ok'));\n\
globalThis.print('process ok', cwd());\n\
import { make, add, square } from 'complex';\n\
const a = make(3, 4);\n\
const b = make(1, 2);\n\
const s = add(a, b);\n\
if (s.re !== 4 || s.im !== 6) throw new Error('complex add failed');\n\
const q = square(a);\n\
if (q.re !== -7 || q.im !== 24) throw new Error('complex square failed');\n\
globalThis.print('complex ok', s.re, s.im, q.re, q.im);\n\
0\n\0";

    let mod_ret = qjs::JS_Eval(
        ctx,
        mod_script.as_ptr() as *const c_char,
        mod_script.len() - 1,
        mod_filename.as_ptr() as *const c_char,
        qjs::JS_EVAL_TYPE_MODULE,
    );

    if mod_ret.is_exception() {
        log_str("quickjs: module JS_Eval exception\n");
        dump_exception(ctx);
    } else {
        qjs::js_free_value(ctx, mod_ret);
        log_str("quickjs: module eval ok\n");
        let _ = drain_jobs_and_promises(rt, ctx, 30_000);
    }

    // Keep the original minimal global eval as a baseline sanity check.
    let filename = b"<smoke>\0";
    let script = b"print('hello from quickjs'); 1 + 1\0";
    let ret = qjs::JS_Eval(
        ctx,
        script.as_ptr() as *const c_char,
        script.len() - 1,
        filename.as_ptr() as *const c_char,
        qjs::JS_EVAL_TYPE_GLOBAL,
    );

    if ret.is_exception() {
        log_str("quickjs: JS_Eval exception\n");
        dump_exception(ctx);
    } else {
        let out = qjs::js_to_cstring(ctx, ret);
        if !out.is_null() {
            let bytes = CStr::from_ptr(out).to_bytes();
            log_str("quickjs: eval ok => ");
            if let Ok(s) = core::str::from_utf8(bytes) {
                log_str(s);
            } else {
                log_bytes(bytes);
            }
            log_nl();
            qjs::JS_FreeCString(ctx, out);
        } else {
            log_str("quickjs: eval ok (toString failed)\n");
        }
    }

    qjs::js_free_value(ctx, ret);

    log_str("quickjs: runtime/context ok\n");
    drop(vm);
}

/// Temporary boot-time smoke for parse5 HTML parsing.
///
/// Goal: validate esm.sh ESM imports and DOM-like output parsing via parse5.
pub unsafe fn run_parse5_smoke() {
    let Some(vm) = qjs::vm::QjsVm::new_node() else {
        log_str("quickjs: JS_NewRuntime failed\n");
        return;
    };
    let rt = vm.rt_ptr();
    let ctx = vm.ctx_ptr();

    install_print(ctx);
    qjs::node::install_globals(ctx);

    let mod_filename = b"<smoke-parse5>\0";
    let mod_script = b"import * as parse5 from 'parse5@7.1.2';\n\
const html = '<!doctype html><html><head><title>x</title></head><body><div id=\\\"x\\\"><span class=\\\"y\\\">hi</span></div></body></html>';\n\
const doc = parse5.parse(html);\n\
function countNodes(node) {\n\
  let n = 1;\n\
  if (node && node.childNodes) {\n\
    for (const c of node.childNodes) n += countNodes(c);\n\
  }\n\
  return n;\n\
}\n\
const root = (doc.childNodes || []).find(n => n.nodeName === 'html') || doc;\n\
const count = countNodes(doc);\n\
globalThis.print('parse5 ok', root.nodeName, count);\n\
0\n\
\0";

    let mod_ret = qjs::JS_Eval(
        ctx,
        mod_script.as_ptr() as *const c_char,
        mod_script.len() - 1,
        mod_filename.as_ptr() as *const c_char,
        qjs::JS_EVAL_TYPE_MODULE,
    );

    if mod_ret.is_exception() {
        log_str("quickjs: parse5 JS_Eval exception\n");
        dump_exception(ctx);
    } else {
        qjs::js_free_value(ctx, mod_ret);
        log_str("quickjs: parse5 eval ok\n");
        let _ = drain_jobs_and_promises(rt, ctx, 30_000);
    }

    drop(vm);
}

/// Boot-time smoke for importing a few very common, small ESM modules via jsDelivr.
///
/// Goal:
/// - Exercise URL module loading + caching (including relative URL imports inside a package).
/// - Keep the workload small enough for boot-time use.
pub unsafe fn run_common_modules_smoke() {
    let Some(vm) = qjs::vm::QjsVm::new_node() else {
        log_str("quickjs: JS_NewRuntime failed\n");
        return;
    };
    let rt = vm.rt_ptr();
    let ctx = vm.ctx_ptr();

    install_print(ctx);
    qjs::node::install_globals(ctx);

    let mod_filename = b"<smoke-common-modules>\0";
    let mod_script = b"globalThis.print('common-modules: start');\n\
import { __extends } from 'https://cdn.jsdelivr.net/npm/tslib@2.6.2/tslib.es6.mjs';\n\
if (typeof __extends !== 'function') throw new Error('tslib.__extends missing');\n\
globalThis.print('common-modules: tslib ok');\n\
import _ from 'https://cdn.jsdelivr.net/npm/lodash@4.17.21/+esm';\n\
if (typeof _ !== 'function' && (typeof _ !== 'object' || _ === null)) throw new Error('lodash default export unexpected');\n\
if (typeof _.camelCase !== 'function') throw new Error('lodash.camelCase missing');\n\
const cc = _.camelCase('Foo Bar');\n\
if (cc !== 'fooBar') throw new Error('lodash.camelCase unexpected: ' + cc);\n\
globalThis.print('common-modules: lodash ok', cc);\n\
0\n\
\0";

    let mod_ret = qjs::JS_Eval(
        ctx,
        mod_script.as_ptr() as *const c_char,
        mod_script.len() - 1,
        mod_filename.as_ptr() as *const c_char,
        qjs::JS_EVAL_TYPE_MODULE,
    );

    if mod_ret.is_exception() {
        log_str("quickjs: common-modules JS_Eval exception\n");
        dump_exception(ctx);
    } else {
        qjs::js_free_value(ctx, mod_ret);
        log_str("quickjs: common-modules eval ok\n");
        let _ = drain_jobs_and_promises(rt, ctx, 30_000);
    }

    drop(vm);
}

/// Temporary boot-time smoke for importing PixiJS via esm.sh.
///
/// Goal: validate that our ESM URL loader can fetch a large real-world UI library.
/// Rendering is intentionally out of scope here; this is import-only.
pub unsafe fn run_pixi_import_smoke() {
    let Some(vm) = qjs::vm::QjsVm::new_node() else {
        log_str("quickjs: JS_NewRuntime failed\n");
        return;
    };
    let rt = vm.rt_ptr();
    let ctx = vm.ctx_ptr();

    install_print(ctx);
    qjs::node::install_globals(ctx);

    // Pin a specific version so smoke output is stable and caching is effective.
    // Use esm.sh's bundled build to reduce the number of separate fetches (more reliable
    // under tight boot-time networking constraints).
    let mod_filename = b"<smoke-pixi-import>\0";
    let mod_script = b"import * as PIXI from 'pixi.js@7.4.0?bundle&target=es2022';\n\
globalThis.print('pixi import: ok');\n\
globalThis.print('pixi VERSION', (PIXI && PIXI.VERSION) ? PIXI.VERSION : 'unknown');\n\
globalThis.print('pixi exports', Object.keys(PIXI || {}).length);\n\
0\n\
\0";

    let mod_ret = qjs::JS_Eval(
        ctx,
        mod_script.as_ptr() as *const c_char,
        mod_script.len() - 1,
        mod_filename.as_ptr() as *const c_char,
        qjs::JS_EVAL_TYPE_MODULE,
    );

    if mod_ret.is_exception() {
        log_str("quickjs: pixi-import JS_Eval exception\n");
        dump_exception(ctx);
    } else {
        qjs::js_free_value(ctx, mod_ret);
        log_str("quickjs: pixi-import eval ok\n");
        let _ = drain_jobs_and_promises(rt, ctx, 60_000);
    }

    drop(vm);
}

/// Boot-time / on-demand smoke for rendering a single rectangle via PixiJS.
///
/// Goal: exercise the WebGL shim through a real library (Pixi) and get a visible draw
/// when the kernel gfx backend is switched to a virtio-backed scanout.
pub unsafe fn run_pixi_rect_smoke() {
    let Some(vm) = qjs::vm::QjsVm::new_node() else {
        log_str("quickjs: JS_NewRuntime failed\n");
        return;
    };
    let rt = vm.rt_ptr();
    let ctx = vm.ctx_ptr();

    log_str("quickjs: pixi-tri: vm ok\n");

    install_print(ctx);
    qjs::node::install_globals(ctx);

    // Preflight: prove JS -> print() -> kernel log bridge works in this VM.
    {
        let filename = b"<pixi-tri-preflight>\0";
        let script = b"print('pixi-tri: preflight print ok'); 0\0";
        let v = qjs::JS_Eval(
            ctx,
            script.as_ptr() as *const c_char,
            script.len() - 1,
            filename.as_ptr() as *const c_char,
            qjs::JS_EVAL_TYPE_GLOBAL,
        );
        if v.is_exception() {
            log_str("quickjs: pixi-tri preflight JS_Eval exception\n");
            dump_exception(ctx);
        } else {
            qjs::js_free_value(ctx, v);
        }
    }

    let mod_filename = b"<smoke-pixi-tri-bundle>\0";
    let mut owned: alloc::vec::Vec<u8> = include_str!("../app/pixi/bundle.mjs")
        .as_bytes()
        .to_vec();
    // NUL-terminate for parser stability.
    owned.push(0);

    log_str("quickjs: pixi-tri: JS_Eval begin\n");
    let mod_ret = qjs::JS_Eval(
        ctx,
        owned.as_ptr() as *const c_char,
        owned.len() - 1,
        mod_filename.as_ptr() as *const c_char,
        qjs::JS_EVAL_TYPE_MODULE,
    );

    log_str("quickjs: pixi-tri: JS_Eval end\n");

    if mod_ret.is_exception() {
        log_str("quickjs: pixi-rect JS_Eval exception\n");
        dump_exception(ctx);
    } else {
        qjs::js_free_value(ctx, mod_ret);
        log_str("quickjs: pixi-rect eval ok\n");
        let _ = drain_jobs_and_promises(rt, ctx, 60_000);

        // Postflight: if the module ran, logs should have appeared; still verify print works.
        let filename = b"<pixi-tri-postflight>\0";
        let script = b"print('pixi-tri: postflight print ok'); 0\0";
        let v = qjs::JS_Eval(
            ctx,
            script.as_ptr() as *const c_char,
            script.len() - 1,
            filename.as_ptr() as *const c_char,
            qjs::JS_EVAL_TYPE_GLOBAL,
        );
        if v.is_exception() {
            log_str("quickjs: pixi-tri postflight JS_Eval exception\n");
            dump_exception(ctx);
        } else {
            qjs::js_free_value(ctx, v);
        }
    }

    drop(vm);
}
