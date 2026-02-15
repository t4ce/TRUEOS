extern crate alloc;

use core::ffi::{c_char, c_int, CStr};

use crate as qjs;

extern "C" {
    fn trueos_cabi_write(stream: u32, bytes: *const u8, len: usize);
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
    let exc = qjs::JS_GetException(ctx);
    let cstr = qjs::js_to_cstring(ctx, exc);
    log_str("quickjs: exception: ");
    if !cstr.is_null() {
        let bytes = CStr::from_ptr(cstr).to_bytes();
        // Best-effort: assume utf8 for logs; fallback to raw bytes.
        if let Ok(s) = core::str::from_utf8(bytes) {
            log_str(s);
        } else {
            log_bytes(bytes);
        }
        qjs::JS_FreeCString(ctx, cstr);
    } else {
        log_str("<toString failed>");
    }
    log_nl();

    // Best-effort: print `exc.stack` if present.
    // Many JS libraries throw rich Error objects; the stack often contains the missing global.
    {
        let stack_key = b"stack\0";
        let stack_val = qjs::JS_GetPropertyStr(ctx, exc, stack_key.as_ptr() as *const c_char);
        if !stack_val.is_exception() {
            let stack_cstr = qjs::js_to_cstring(ctx, stack_val);
            if !stack_cstr.is_null() {
                let bytes = CStr::from_ptr(stack_cstr).to_bytes();
                log_str("quickjs: exception.stack: ");
                if let Ok(s) = core::str::from_utf8(bytes) {
                    log_str(s);
                } else {
                    log_bytes(bytes);
                }
                log_nl();
                qjs::JS_FreeCString(ctx, stack_cstr);
            }
        }
        qjs::js_free_value(ctx, stack_val);
    }

    qjs::js_free_value(ctx, exc);
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

/// TRUEOS kernel QuickJS module-loader smoke test:
/// - Installs the TRUEOS module loader.
/// - Evaluates an ES module that imports a bare specifier from esm.sh.
///
/// This exercises:
/// - `trueos_module_normalize` (bare specifier -> https://esm.sh/...)
/// - URL module caching to `/qjs/cdn/<hash>.mjs` via async net-fetch C-ABI
/// - Recursive URL import handling (origin-relative specifiers like "/pkg@ver/..." from esm.sh)
pub unsafe fn run_module_loader_smoke() {
    let Some(vm) = qjs::vm::QjsVm::new_node() else {
        log_str("quickjs: JS_NewRuntime failed\n");
        return;
    };
    let ctx = vm.ctx_ptr();

    install_print(ctx);
    qjs::node::install_globals(ctx);

    let mod_filename = b"<smoke-module-loader>\0";
    let mod_script = b"import proc from 'node:process';\n\
if (typeof proc !== 'object' || proc === null) throw new Error('node:process missing');\n\
import * as path from 'path';\n\
if (typeof path.join !== 'function') throw new Error('path.join missing');\n\
const joined = path.join('a', 'b');\n\
if (joined !== 'a/b' && joined !== 'a\\b') throw new Error('path.join unexpected: ' + joined);\n\
import leftPad from 'left-pad@1.3.0';\n\
const out = leftPad('a', 3, '.');\n\
if (out !== '..a') throw new Error('left-pad unexpected: ' + out);\n\
globalThis.print('module-loader ok', out);\n\
0\n\0";

    let mod_ret = qjs::JS_Eval(
        ctx,
        mod_script.as_ptr() as *const c_char,
        mod_script.len() - 1,
        mod_filename.as_ptr() as *const c_char,
        qjs::JS_EVAL_TYPE_MODULE,
    );

    if mod_ret.is_exception() {
        log_str("quickjs: module-loader JS_Eval exception\n");
        dump_exception(ctx);
    } else {
        qjs::js_free_value(ctx, mod_ret);
        log_str("quickjs: module-loader eval ok\n");
    }

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
    let ctx = vm.ctx_ptr();

    install_print(ctx);
    qjs::node::install_globals(ctx);

        // IMPORTANT: In an ES module, imports must come first.
        // Use __trueos_print so logs remain stable even if libraries overwrite globalThis.print.
        let mod_filename = b"<smoke-pixi-tri>\0";
        let mod_script = br#"import * as PIXI from 'pixi.js@7.4.0?bundle&target=es2022';

// Allow getContext('webgl') one-arg calls during this smoke.
globalThis.__trueos_webgl_force = 1;

const log = globalThis.__trueos_print || globalThis.print;
log('pixi-tri: start');

try {
    const canvas = document.createElement('canvas');
    canvas.width = 320;
    canvas.height = 200;

    const renderer = new PIXI.Renderer({
        view: canvas,
        width: canvas.width,
        height: canvas.height,
        antialias: false,
        backgroundColor: 0x081830
    });

    // Triangle in pixel coords.
    const positions = new Float32Array([
        40, 40,
        280, 60,
        160, 170,
    ]);

    // Per-vertex RGBA (normalized u8): R, G, B corners.
    const colors = new Uint8Array([
        255, 0, 0, 255,
        0, 255, 0, 255,
        0, 0, 255, 255,
    ]);

    const indices = new Uint16Array([0, 1, 2]);

    const geometry = new PIXI.Geometry()
        .addAttribute('aVertexPosition', positions, 2, false, PIXI.TYPES.FLOAT)
        .addAttribute('aColor', colors, 4, true, PIXI.TYPES.UNSIGNED_BYTE)
        .addIndex(indices);

    const vs = `precision mediump float;
attribute vec2 aVertexPosition;
attribute vec4 aColor;
uniform mat3 translationMatrix;
uniform mat3 projectionMatrix;
varying vec4 vColor;
void main(){
    vColor = aColor;
    vec3 pos = projectionMatrix * translationMatrix * vec3(aVertexPosition, 1.0);
    gl_Position = vec4(pos.xy, 0.0, 1.0);
}`;

    const fs = `precision mediump float;
varying vec4 vColor;
void main(){
    gl_FragColor = vColor;
}`;

    const program = PIXI.Program.from(vs, fs);
    const shader = new PIXI.Shader(program, {});
    const mesh = new PIXI.Mesh(geometry, shader);

    const stage = new PIXI.Container();
    stage.addChild(mesh);
    renderer.render(stage);

    log('pixi-tri: ok');
} catch (e) {
    log('pixi-tri: error', (e && e.stack) ? e.stack : (e && e.message) ? e.message : String(e));
}

0
"#;

        // NUL-terminate: QuickJS parser tends to be more reliable with C-string style buffers.
        let mut owned: alloc::vec::Vec<u8> = alloc::vec::Vec::with_capacity(mod_script.len() + 1);
        owned.extend_from_slice(mod_script);
        owned.push(0);

        let mod_ret = qjs::JS_Eval(
                ctx,
                owned.as_ptr() as *const c_char,
                owned.len() - 1,
                mod_filename.as_ptr() as *const c_char,
                qjs::JS_EVAL_TYPE_MODULE,
        );

    if mod_ret.is_exception() {
        log_str("quickjs: pixi-rect JS_Eval exception\n");
        dump_exception(ctx);
    } else {
        qjs::js_free_value(ctx, mod_ret);
        log_str("quickjs: pixi-rect eval ok\n");
    }

    drop(vm);
}


