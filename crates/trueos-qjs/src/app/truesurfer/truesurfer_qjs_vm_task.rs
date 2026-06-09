#![cfg(feature = "trueos")]

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::{BTreeMap, VecDeque};
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::ffi::c_char;
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_sync::blocking_mutex::raw::RawMutex;
use embassy_sync::signal::Signal;
use embassy_sync::zerocopy_channel::{Channel, Receiver, Sender};
use embassy_time::{Duration as EmbassyDuration, Timer, with_timeout};
use spin::{Mutex, Once};

use crate as qjs;

pub const MAX_BROWSER_INSTANCE_ID: u32 = 50;
pub const TRUESURFER_TASK_POOL_SIZE: usize = 100;
pub const BOOT_BROWSER_INSTANCE_IDS: [u32; 10] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];

pub const HOSTED_KEYBOARD_MOD_SHIFT: u8 = 1 << 0;
pub const HOSTED_KEYBOARD_MOD_CTRL: u8 = 1 << 1;
pub const HOSTED_KEYBOARD_MOD_ALT: u8 = 1 << 2;
pub const HOSTED_KEYBOARD_MOD_META: u8 = 1 << 3;

const TRUESURFER_IMPORT_FILENAME: &[u8] = b"<truesurfer-init>\0";
const TRUESURFER_PIXI_HOST_PRELUDE_FILENAME: &[u8] = b"<truesurfer-pixi-host-prelude>\0";
const TRUESURFER_PIXI_BUNDLE_FILENAME: &[u8] = b"<truesurfer-pixi-bundle>\0";
const TRUESURFER_PIXI_COLLECTOR_FILENAME: &[u8] = b"<truesurfer-pixi-collector>\0";
const TRUESURFER_PIXI_CAPTURE_ADAPTER_FILENAME: &[u8] = b"<truesurfer-pixi-capture-adapter>\0";
const TRUESURFER_PARSE5_TRUEOS_APP_FILENAME: &[u8] = b"<truesurfer-parse5-trueos-app.js>\0";
const TRUESURFER_PARSE5_VITE_HOST_CORE_FILENAME: &[u8] = b"<truesurfer-parse5-trueos-host-core>\0";
const TRUESURFER_PARSE5_VITE_HOST_EVENT_FILENAME: &[u8] =
    b"<truesurfer-parse5-trueos-host-event>\0";
const TRUESURFER_PARSE5_VITE_HOST_CANVAS_FILENAME: &[u8] =
    b"<truesurfer-parse5-trueos-host-canvas>\0";
const TRUESURFER_PARSE5_VITE_HOST_DOM_FILENAME: &[u8] = b"<truesurfer-parse5-trueos-host-dom>\0";
const TRUESURFER_PARSE5_VITE_HOST_FETCH_FILENAME: &[u8] =
    b"<truesurfer-parse5-trueos-host-fetch>\0";
const TRUESURFER_PARSE5_VITE_HOST_CAPTURE_FILENAME: &[u8] =
    b"<truesurfer-parse5-trueos-host-capture>\0";
const TRUESURFER_PIXI_HOST_PRELUDE_SOURCE: &[u8] =
    include_bytes!("../../../../../src/ui3/pixi_host_prelude.js");
const TRUESURFER_PIXI_BUNDLE_SOURCE: &[u8] =
    include_bytes!("../../../../../src/ui3/pixi_bundle.min.js");
const TRUESURFER_PIXI_CAPTURE_ADAPTER_SOURCE: &[u8] =
    include_bytes!("../../../../../src/ui3/pixi_capture_adapter.js");
const TRUESURFER_PARSE5_TRUEOS_APP_SOURCE: &[u8] =
    include_bytes!("../../../../../../Parse5/dist/trueos/index.js");

fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut hash = 0x811c9dc5u32;
    for b in bytes {
        hash ^= *b as u32;
        hash = hash.wrapping_mul(0x01000193);
    }
    hash
}

fn contains_bytes(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

#[inline]
fn now_ms() -> u64 {
    let hz = embassy_time_driver::TICK_HZ.max(1);
    embassy_time_driver::now().saturating_mul(1000) / hz
}

const TRUESURFER_PIXI_COLLECTOR_SOURCE: &[u8] = br#"
(function (G) {
  "use strict";
  var ops = [];
  var rootId = 0;

  function num(value, fallback) {
    var out = Number(value);
    return Number.isFinite(out) ? out : fallback;
  }

  function kindCode(kind) {
    kind = String(kind || "Container");
    if (kind === "Graphics") return 1;
    if (kind === "Text") return 2;
    return 0;
  }

  function pointerEventCode(event) {
    event = String(event || "");
    if (event === "pointerdown") return 1;
    if (event === "pointerup") return 2;
    if (event === "pointermove") return 3;
    if (event === "pointerover") return 4;
    if (event === "pointerout") return 5;
    if (event === "pointerupoutside") return 6;
    if (event === "contextmenu") return 7;
    return 1;
  }

  function push(op) {
    ops.push(op);
    return ops.length;
  }

  G.__trueosPixiResetScene = function () {
    ops = [];
    rootId = 0;
  };

  G.__trueosPixiTakeScene = function () {
    return {
      version: 1,
      commandSource: "pixi",
      rootId: rootId,
      opCount: ops.length,
      ops: ops.slice(),
    };
  };

  G.__trueosPixiOp = function (name) {
    name = String(name || "");
    var node = num(arguments[1], 0);
    switch (name) {
      case "node": return push({ code: 1, node: node, a: kindCode(arguments[2]) });
      case "addChild": return push({ code: 2, node: node, a: num(arguments[2], 0) });
      case "addChildAt": return push({ code: 10, node: node, a: num(arguments[2], 0), b: num(arguments[3], 0) });
      case "setChildIndex": return push({ code: 11, node: node, a: num(arguments[2], 0), b: num(arguments[3], 0) });
      case "removeChild": return push({ code: 12, node: node, a: num(arguments[2], 0) });
      case "removeFromParent": return push({ code: 13, node: node });
      case "removeChildren": return push({ code: 14, node: node });
      case "position": return push({ code: 3, node: node, a: num(arguments[2], 0), b: num(arguments[3], 0) });
      case "visible": return push({ code: 15, node: node, a: num(arguments[2], 1) });
      case "alpha": return push({ code: 23, node: node, a: num(arguments[2], 1) });
      case "scale": return push({ code: 28, node: node, a: num(arguments[2], 1), b: num(arguments[3], 1) });
      case "mask": return push({ code: 27, node: node, a: num(arguments[2], 0) });
      case "listen": return push({ code: 16, node: node, a: pointerEventCode(arguments[2]) });
      case "removeAllListeners": return push({ code: 17, node: node });
      case "clear": return push({ code: 4, node: node });
      case "rect": return push({ code: 5, node: node, a: num(arguments[2], 0), b: num(arguments[3], 0), c: num(arguments[4], 0), d: num(arguments[5], 0) });
      case "roundRect": return push({ code: 24, node: node, a: num(arguments[2], 0), b: num(arguments[3], 0), c: num(arguments[4], 0), d: num(arguments[5], 0), text: String(num(arguments[6], 0)) });
      case "circle": return push({ code: 18, node: node, a: num(arguments[2], 0), b: num(arguments[3], 0), c: num(arguments[4], 0) });
      case "ellipse": return push({ code: 26, node: node, a: num(arguments[2], 0), b: num(arguments[3], 0), c: num(arguments[4], 0), d: num(arguments[5], 0) });
      case "moveTo": return push({ code: 19, node: node, a: num(arguments[2], 0), b: num(arguments[3], 0) });
      case "lineTo": return push({ code: 20, node: node, a: num(arguments[2], 0), b: num(arguments[3], 0) });
      case "closePath": return push({ code: 25, node: node });
      case "image": return push({ code: 22, node: node, a: num(arguments[2], 0), b: num(arguments[3], 0), c: num(arguments[4], 0), d: num(arguments[5], 0), text: String(num(arguments[6], 0)) });
      case "fill": return push({ code: 6, node: node, a: num(arguments[2], 0xffffff), b: num(arguments[3], 1) });
      case "stroke": return push({ code: 7, node: node, a: num(arguments[2], 0xffffff), b: num(arguments[3], 1), c: num(arguments[4], 1) });
      case "text": return push({ code: 8, node: node, text: String(arguments[2] == null ? "" : arguments[2]) });
      case "textFill": return push({ code: 9, node: node, a: num(arguments[2], 0xffffff), b: num(arguments[3], 1) });
      default: return ops.length;
    }
  };

  G.__trueosRender = function (root) {
    rootId = root && root.__trueosPixiId ? Number(root.__trueosPixiId) || 0 : 0;
    return rootId;
  };
})(typeof globalThis !== "undefined" ? globalThis : this);
"#;
#[allow(dead_code)]
const TRUESURFER_PARSE5_VITE_HOST_SOURCE: &[u8] = br##"
(function (G) {
  "use strict";

  function num(value, fallback) {
    var out = Number(value);
    return Number.isFinite(out) ? out : fallback;
  }

  function makeEvent(type, init) {
    this.type = String(type || "");
    this.cancelable = !!(init && init.cancelable);
    this.defaultPrevented = false;
  }
  makeEvent.prototype.preventDefault = function () {
    if (this.cancelable) this.defaultPrevented = true;
  };
  if (typeof G.Event !== "function") G.Event = makeEvent;

  function CanvasContext2D() {
    this.font = "16px sans-serif";
    this.fillStyle = "black";
    this.strokeStyle = "black";
    this.shadowColor = "black";
    this.shadowBlur = 0;
    this.shadowOffsetX = 0;
    this.shadowOffsetY = 0;
    this.textBaseline = "alphabetic";
  }
  CanvasContext2D.prototype.measureText = function (text) {
    var s = String(text == null ? "" : text);
    var m = /(\d+(?:\.\d+)?)px/.exec(String(this.font || ""));
    var px = m ? Number(m[1]) : 16;
    var width = s.length * px * 0.58;
    return {
      width: width,
      actualBoundingBoxLeft: 0,
      actualBoundingBoxRight: width,
      actualBoundingBoxAscent: px * 0.8,
      actualBoundingBoxDescent: px * 0.2
    };
  };
  CanvasContext2D.prototype.createImageData = function (w, h) {
    return { width: w, height: h, data: new Uint8ClampedArray(Math.max(0, w * h * 4)) };
  };
  CanvasContext2D.prototype.putImageData = function () {};
  CanvasContext2D.prototype.clearRect = function () {};
  CanvasContext2D.prototype.fillRect = function () {};
  CanvasContext2D.prototype.drawImage = function () {};
  CanvasContext2D.prototype.fillText = function () {};
  CanvasContext2D.prototype.strokeText = function () {};
  CanvasContext2D.prototype.resetTransform = function () {};
  CanvasContext2D.prototype.scale = function () {};
  CanvasContext2D.prototype.getImageData = function (x, y, w, h) {
    return { width: w, height: h, data: new Uint8ClampedArray(Math.max(0, w * h * 4)) };
  };
  CanvasContext2D.prototype.createPattern = function () { return { setTransform: function () {} }; };
  CanvasContext2D.prototype.createLinearGradient = function () { return { addColorStop: function () {} }; };
  CanvasContext2D.prototype.createRadialGradient = function () { return { addColorStop: function () {} }; };
  G.CanvasRenderingContext2D = G.CanvasRenderingContext2D || CanvasContext2D;

  function Element(tag) {
    this.tagName = String(tag || "div").toUpperCase();
    this.nodeName = this.tagName;
    this.children = [];
    this.childNodes = this.children;
    this.style = {};
    this.relList = { supports: function () { return true; } };
    this.listeners = Object.create(null);
    this.parentNode = null;
    this.textContent = "";
    this.href = "";
    this.rel = "";
    this.as = "";
    this.crossOrigin = "";
    this.width = 0;
    this.height = 0;
  }
  Element.prototype.appendChild = function (child) {
    if (child) {
      child.parentNode = this;
      this.children.push(child);
    }
    return child;
  };
  Element.prototype.removeChild = function (child) {
    var idx = this.children.indexOf(child);
    if (idx >= 0) this.children.splice(idx, 1);
    if (child) child.parentNode = null;
    return child;
  };
  Element.prototype.addEventListener = function (type, fn) {
    this.listeners[String(type || "")] = fn;
  };
  Element.prototype.removeEventListener = function () {};
  Element.prototype.dispatchEvent = function (ev) {
    var fn = this.listeners[String(ev && ev.type || "")];
    if (typeof fn === "function") fn(ev);
    return !(ev && ev.defaultPrevented);
  };
  Element.prototype.setAttribute = function (name, value) {
    this[String(name || "")] = String(value == null ? "" : value);
  };
  Element.prototype.getAttribute = function (name) {
    var v = this[String(name || "")];
    return v == null ? null : String(v);
  };
  Element.prototype.getContext = function (kind) {
    if (String(kind || "").toLowerCase() !== "2d") return null;
    return new G.CanvasRenderingContext2D();
  };

  if (typeof G.HTMLCanvasElement !== "function") G.HTMLCanvasElement = Element;
  if (typeof G.MutationObserver !== "function") {
    G.MutationObserver = function () {};
    G.MutationObserver.prototype.observe = function () {};
    G.MutationObserver.prototype.disconnect = function () {};
  }

  var body = new Element("body");
  var head = new Element("head");
  var document = {
    body: body,
    head: head,
    createElement: function (tag) { return new Element(tag); },
    getElementById: function (id) {
      if (String(id || "") === "app") return body;
      return null;
    },
    querySelector: function () { return null; },
    querySelectorAll: function () { return []; },
    getElementsByTagName: function (name) {
      name = String(name || "").toLowerCase();
      if (name === "body") return [body];
      if (name === "head") return [head];
      if (name === "link") return [];
      return [];
    },
    contains: function () { return true; },
    addEventListener: function () {},
    removeEventListener: function () {}
  };
  G.document = document;
  G.window = G;
  G.self = G;
  G.innerWidth = Math.max(1, num(G.innerWidth, 2560) | 0);
  G.innerHeight = Math.max(1, num(G.innerHeight, 1440) | 0);
  G.__trueosWindowListeners = G.__trueosWindowListeners || Object.create(null);
  G.addEventListener = function (type, fn) {
    type = String(type || "");
    if (typeof fn !== "function") return;
    var list = G.__trueosWindowListeners[type];
    if (!list) list = G.__trueosWindowListeners[type] = [];
    list.push(fn);
  };
  G.removeEventListener = function (type, fn) {
    type = String(type || "");
    var list = G.__trueosWindowListeners[type];
    if (!list || typeof fn !== "function") return;
    for (var i = list.length - 1; i >= 0; i -= 1) {
      if (list[i] === fn) list.splice(i, 1);
    }
  };
  G.dispatchEvent = function (ev) {
    var type = String((ev && ev.type) || "");
    var list = G.__trueosWindowListeners[type] || [];
    for (var i = 0; i < list.length; i += 1) list[i].call(G, ev);
    return !(ev && ev.defaultPrevented);
  };
  G.__TRUEOS_DISPATCH_KEYDOWN__ = function (key, pointerId, modifiers, slotId) {
    modifiers = Number(modifiers) || 0;
    var ev = {
      type: "keydown",
      key: String(key || ""),
      pointerId: Number(pointerId) || 1,
      slotId: Number(slotId) || 0,
      shiftKey: !!(modifiers & 0x22),
      ctrlKey: !!(modifiers & 0x11),
      altKey: !!(modifiers & 0x44),
      metaKey: !!(modifiers & 0x88),
      defaultPrevented: false,
      preventDefault: function () { this.defaultPrevented = true; },
      stopPropagation: function () { this.propagationStopped = true; }
    };
    var before = (G.__pixiCapture && G.__pixiCapture.commands && G.__pixiCapture.commands.length) || 0;
    G.dispatchEvent(ev);
    var repainted = 0;
    if (G.__TRUEOS_PIXI_DIRTY__ && typeof G.__TRUEOS_REPAINT_NOW__ === "function") {
      G.__TRUEOS_REPAINT_NOW__();
      repainted = 1;
    }
    var after = (G.__pixiCapture && G.__pixiCapture.commands && G.__pixiCapture.commands.length) || before;
    var listeners = (G.__trueosWindowListeners.keydown || []).length;
    return { handled: listeners > 0 ? 1 : 0, listenerCount: listeners, painted: (after > before || repainted) ? 1 : 0, defaultPrevented: ev.defaultPrevented ? 1 : 0 };
  };
  G.requestAnimationFrame = G.requestAnimationFrame || function (fn) {
    if (typeof fn !== "function") return 0;
    if (typeof G.setTimeout === "function") {
      return G.setTimeout(function () {
        fn((G.performance && G.performance.now && G.performance.now()) || 0);
      }, 16);
    }
    fn((G.performance && G.performance.now && G.performance.now()) || 0);
    return 1;
  };
  G.cancelAnimationFrame = G.cancelAnimationFrame || function () {};
  if (!G.navigator) G.navigator = {};
  G.navigator.userAgent = G.navigator.userAgent || "TRUEOS Browser-OS";
  G.navigator.sendBeacon = function () { return true; };
  G["__pixiCapture"] = undefined;
  G["__TRUEOS_CAPTURE_ONLY__"] = true;
  if (typeof G["__TRUEOS_INPUT_HTML__"] !== "string") G["__TRUEOS_INPUT_HTML__"] = "";
  G["__TRUEOS_PIXI_APP"] = undefined;
  G["__TRUEOS_PIXI_APP_READY__"] = false;

  if (typeof G.Response !== "function") {
    G.Response = function Response(body, init) {
      this._body = String(body == null ? "" : body);
      this.status = init && init.status ? Number(init.status) | 0 : 200;
      this.ok = this.status >= 200 && this.status < 300;
    };
    G.Response.prototype.text = function () { return Promise.resolve(this._body); };
    G.Response.prototype.json = function () { return Promise.resolve(JSON.parse(this._body)); };
  }
  G.fetch = function (input, init) {
    var url = String(input && input.url ? input.url : input || "");
    if (url === "/input.html" || url.endsWith("/input.html")) {
      return Promise.resolve(new G.Response(String(G["__TRUEOS_INPUT_HTML__"] || ""), { status: 200 }));
    }
    if (url === "/__pixi_capture") {
      return Promise.resolve(new G.Response("", { status: 204 }));
    }
    return Promise.resolve(new G.Response("", { status: 200 }));
  };

  function pushNodeFromSnapshot(node, parent, ops, seen, snapshotSeen, textSlots) {
    if (!node || typeof node !== "object") return 0;
    var id = num(node.id, 0) | 0;
    if (id <= 0) return 0;
    var kind = 0;
    if (!seen[id]) {
      var type = String(node.type || "");
      kind = type.indexOf("Graphics") >= 0 ? 1 : type.indexOf("Text") >= 0 ? 2 : 0;
      ops.push({ code: 1, node: id, a: kind });
      seen[id] = true;
    }
    snapshotSeen[id] = true;
    if (kind === 2 && textSlots) textSlots.push(id);
    if (parent > 0) ops.push({ code: 2, node: parent, a: id });
    ops.push({ code: 3, node: id, a: num(node.x, 0), b: num(node.y, 0) });
    if (node.visible === false) ops.push({ code: 15, node: id, a: 0 });
    if (typeof node.alpha === "number" && node.alpha !== 1) ops.push({ code: 23, node: id, a: num(node.alpha, 1) });
    if (num(node.maskId, 0) > 0) ops.push({ code: 27, node: id, a: num(node.maskId, 0) });
    if (typeof node.text === "string" && textHasInk(node.text)) ops.push({ code: 8, node: id, text: node.text });
    var children = Array.isArray(node.children) ? node.children : [];
    if (node.sortableChildren === true && children.length > 1) {
      children = children.slice();
      children.sort(function (a, b) {
        return num(a && a.zIndex, 0) - num(b && b.zIndex, 0);
      });
    }
    for (var i = 0; i < children.length; i += 1) pushNodeFromSnapshot(children[i], id, ops, seen, snapshotSeen, textSlots);
    return id;
  }

  function colorArg(value, fallback) {
    if (typeof value === "number") return value >>> 0;
    if (value && typeof value.color === "number") return value.color >>> 0;
    if (typeof value === "string" && value.charAt(0) === "#") return parseInt(value.slice(1), 16) >>> 0;
    return fallback;
  }
  function alphaArg(value) {
    return value && typeof value.alpha === "number" ? num(value.alpha, 1) : 1;
  }
  function widthArg(value) {
    if (value && typeof value.width === "number") return num(value.width, 1);
    return value && typeof value.w === "number" ? num(value.w, 1) : 1;
  }
  function commandKind(target) {
    target = String(target || "");
    if (target.indexOf("Graphics") >= 0) return 1;
    if (target.indexOf("Text") >= 0) return 2;
    return 0;
  }
  function pointerEventCode(event) {
    event = String(event || "");
    if (event === "pointerdown") return 1;
    if (event === "pointerup") return 2;
    if (event === "pointermove") return 3;
    if (event === "pointerover") return 4;
    if (event === "pointerout") return 5;
    if (event === "pointerupoutside") return 6;
    if (event === "contextmenu") return 7;
    return 1;
  }
  function textHasInk(value) {
    var text = String(value == null ? "" : value);
    for (var i = 0; i < text.length; i += 1) {
      var code = text.charCodeAt(i);
      if (
        code > 32 &&
        !(code >= 127 && code <= 160) &&
        code !== 5760 &&
        !(code >= 8192 && code <= 8207) &&
        !(code >= 8232 && code <= 8238) &&
        code !== 8239 &&
        !(code >= 8287 && code <= 8298) &&
        code !== 12288 &&
        !(code >= 65024 && code <= 65039) &&
        code !== 65279
      ) return true;
    }
    return false;
  }

  G.__trueosParse5BuildSceneFromCapture = function () {
    var cap = G["__pixiCapture"];
    var commands = cap && Array.isArray(cap.commands) ? cap.commands : [];
    var ops = [];
    var seen = Object.create(null);
    var snapshotSeen = Object.create(null);
    var textSlots = [];
    var textMap = Object.create(null);
    var textHasFill = Object.create(null);
    var pendingTextFill = Object.create(null);
    var rootId = 0;
    var snapshot = null;
    for (var i = commands.length - 1; i >= 0; i -= 1) {
      if (commands[i] && commands[i].op === "snapshot" && commands[i].args && commands[i].args[0]) {
        snapshot = commands[i].args[0];
        rootId = num(commands[i].id, num(snapshot.id, 0)) | 0;
        break;
      }
    }
    var hasSnapshot = !!snapshot;
    if (snapshot) rootId = pushNodeFromSnapshot(snapshot, 0, ops, seen, snapshotSeen, textSlots) || rootId;

    function mappedTextNode(commandId, textValueHasInk, commandWasSnapshotSeen) {
      return commandId;
    }

    for (var j = 0; j < commands.length; j += 1) {
      var cmd = commands[j] || {};
      var id = num(cmd.id, 0) | 0;
      if (id <= 0) continue;
      var wasSnapshotSeen = !!snapshotSeen[id];
      if (!seen[id]) {
        ops.push({ code: 1, node: id, a: commandKind(cmd.target) });
        seen[id] = true;
      }
      var args = Array.isArray(cmd.args) ? cmd.args : [];
      switch (cmd.op) {
        case "clear": ops.push({ code: 4, node: id }); break;
        case "rect":
          ops.push({ code: 5, node: id, a: num(args[0], 0), b: num(args[1], 0), c: num(args[2], 0), d: num(args[3], 0) });
          break;
        case "roundRect":
          ops.push({ code: 24, node: id, a: num(args[0], 0), b: num(args[1], 0), c: num(args[2], 0), d: num(args[3], 0), text: String(num(args[4], 0)) });
          break;
        case "circle": ops.push({ code: 18, node: id, a: num(args[0], 0), b: num(args[1], 0), c: num(args[2], 0) }); break;
        case "ellipse": ops.push({ code: 26, node: id, a: num(args[0], 0), b: num(args[1], 0), c: num(args[2], 0), d: num(args[3], 0) }); break;
        case "moveTo": ops.push({ code: 19, node: id, a: num(args[0], 0), b: num(args[1], 0) }); break;
      case "lineTo": ops.push({ code: 20, node: id, a: num(args[0], 0), b: num(args[1], 0) }); break;
      case "closePath": ops.push({ code: 25, node: id }); break;
      case "image": ops.push({ code: 22, node: id, a: num(args[0], 0), b: num(args[1], 0), c: num(args[2], 0), d: num(args[3], 0), text: String(num(args[4], 0)) }); break;
      case "poly":
          var points = Array.isArray(args[0]) ? args[0] : [];
          if (points.length >= 2) {
            ops.push({ code: 19, node: id, a: num(points[0], 0), b: num(points[1], 0) });
            for (var pi = 2; pi + 1 < points.length; pi += 2) {
              ops.push({ code: 20, node: id, a: num(points[pi], 0), b: num(points[pi + 1], 0) });
            }
          }
          break;
        case "fill": ops.push({ code: 6, node: id, a: colorArg(args[0], 0xffffff), b: alphaArg(args[0]) }); break;
        case "stroke": ops.push({ code: 7, node: id, a: colorArg(args[0], 0xffffff), b: alphaArg(args[0]), c: widthArg(args[0]) }); break;
        case "addChild":
          if (num(args[0], 0) > 0) ops.push({ code: 2, node: id, a: num(args[0], 0) });
          break;
        case "addChildAt":
          if (num(args[0], 0) > 0) ops.push({ code: 10, node: id, a: num(args[0], 0), b: num(args[1], 0) });
          break;
        case "setChildIndex":
          if (num(args[0], 0) > 0) ops.push({ code: 11, node: id, a: num(args[0], 0), b: num(args[1], 0) });
          break;
        case "removeChild":
          if (!hasSnapshot && num(args[0], 0) > 0) ops.push({ code: 12, node: id, a: num(args[0], 0) });
          break;
        case "removeChildren": if (!hasSnapshot) ops.push({ code: 14, node: id }); break;
        case "removeAllListeners": if (!hasSnapshot) ops.push({ code: 17, node: id }); break;
        case "on": if (cmd.event) ops.push({ code: 16, node: id, a: pointerEventCode(cmd.event) }); break;
        case "alpha": ops.push({ code: 23, node: id, a: num(args[0], 1) }); break;
        case "mask": ops.push({ code: 27, node: id, a: num(args[0], 0) }); break;
        case "text.text.set":
          var textValue = String(args[0] == null ? "" : args[0]);
          var hasInk = textHasInk(textValue);
          if (hasSnapshot && !hasInk) break;
          var textNode = mappedTextNode(id, hasInk, wasSnapshotSeen);
          if (typeof pendingTextFill[id] === "number") {
            ops.push({ code: 9, node: textNode, a: pendingTextFill[id], b: 1 });
            textHasFill[textNode] = true;
            delete pendingTextFill[id];
          }
          if (!textHasFill[textNode]) ops.push({ code: 9, node: textNode, a: 0x111111, b: 1 });
          ops.push({ code: 8, node: textNode, text: textValue });
          break;
        case "text.style.set":
          if (args[0] && typeof args[0].fill !== "undefined") {
            var fill = colorArg(args[0].fill, 0xffffff);
            var mapped = textMap[id] || (wasSnapshotSeen ? id : 0);
            if (mapped) {
              ops.push({ code: 9, node: mapped, a: fill, b: 1 });
              textHasFill[mapped] = true;
            } else pendingTextFill[id] = fill;
          }
          break;
      }
    }
    return {
      ok: 1,
      ui3Scene: {
        version: 1,
        commandSource: "parse5-trueos-pixi",
        rootId: rootId,
        opCount: ops.length,
        ops: ops
      }
    };
  };
})(typeof globalThis !== "undefined" ? globalThis : this);
"##;
const TRUESURFER_PARSE5_VITE_HOST_CORE_SOURCE: &[u8] = br##"
var G = (typeof globalThis !== "undefined") ? globalThis : this;
function __trueosNum(value, fallback) {
  var out = Number(value);
  return Number.isFinite(out) ? out : fallback;
}
"##;
const TRUESURFER_PARSE5_VITE_HOST_EVENT_SOURCE: &[u8] = br##"
var G = (typeof globalThis !== "undefined") ? globalThis : this;
function Event(type, init) {
  this.type = String(type || "");
  this.cancelable = !!(init && init.cancelable);
  this.defaultPrevented = false;
}
Event.prototype.preventDefault = function () {
  if (this.cancelable) this.defaultPrevented = true;
};
if (typeof G.Event !== "function") G.Event = Event;
"##;
const TRUESURFER_PARSE5_VITE_HOST_CANVAS_SOURCE: &[u8] = br##"
var G = (typeof globalThis !== "undefined") ? globalThis : this;
function CanvasRenderingContext2D() {
  this.font = "16px sans-serif";
  this.fillStyle = "black";
  this.strokeStyle = "black";
  this.textBaseline = "alphabetic";
}
CanvasRenderingContext2D.prototype.measureText = function (text) {
  var s = String(text == null ? "" : text);
  var font = String(this.font || "");
  var px_at = font.indexOf("px");
  var start = px_at;
  while (start > 0) {
    var ch = font.charCodeAt(start - 1);
    if (ch < 48 || ch > 57) break;
    start -= 1;
  }
  var px = px_at > start ? Number(font.slice(start, px_at)) : 16;
  var width = s.length * px * 0.58;
  var out = {};
  out.width = width;
  out.actualBoundingBoxLeft = 0;
  out.actualBoundingBoxRight = width;
  out.actualBoundingBoxAscent = px * 0.8;
  out.actualBoundingBoxDescent = px * 0.2;
  return out;
};
CanvasRenderingContext2D.prototype.createImageData = function (w, h) {
  return { width: w, height: h, data: new Uint8ClampedArray(Math.max(0, w * h * 4)) };
};
CanvasRenderingContext2D.prototype.getImageData = function (x, y, w, h) {
  return { width: w, height: h, data: new Uint8ClampedArray(Math.max(0, w * h * 4)) };
};
CanvasRenderingContext2D.prototype.putImageData = function () {};
CanvasRenderingContext2D.prototype.clearRect = function () {};
CanvasRenderingContext2D.prototype.fillRect = function () {};
CanvasRenderingContext2D.prototype.drawImage = function () {};
CanvasRenderingContext2D.prototype.fillText = function () {};
CanvasRenderingContext2D.prototype.strokeText = function () {};
CanvasRenderingContext2D.prototype.resetTransform = function () {};
CanvasRenderingContext2D.prototype.scale = function () {};
CanvasRenderingContext2D.prototype.createPattern = function () { return { setTransform: function () {} }; };
CanvasRenderingContext2D.prototype.createLinearGradient = function () { return { addColorStop: function () {} }; };
CanvasRenderingContext2D.prototype.createRadialGradient = function () { return { addColorStop: function () {} }; };
G.CanvasRenderingContext2D = G.CanvasRenderingContext2D || CanvasRenderingContext2D;
"##;
const TRUESURFER_PARSE5_VITE_HOST_DOM_SOURCE: &[u8] = br##"
var G = (typeof globalThis !== "undefined") ? globalThis : this;
function Element(tag) {
  this.tagName = String(tag || "div").toUpperCase();
  this.nodeName = this.tagName;
  this.children = [];
  this.childNodes = this.children;
  this.style = {};
  this.relList = { supports: function () { return true; } };
  this.listeners = {};
  this.parentNode = null;
  this.textContent = "";
  this.href = "";
  this.rel = "";
  this.as = "";
  this.crossOrigin = "";
  this.width = 0;
  this.height = 0;
}
Element.prototype.appendChild = function (child) {
  if (child) {
    child.parentNode = this;
    this.children.push(child);
  }
  return child;
};
Element.prototype.removeChild = function (child) {
  var idx = this.children.indexOf(child);
  if (idx >= 0) this.children.splice(idx, 1);
  if (child) child.parentNode = null;
  return child;
};
Element.prototype.addEventListener = function (type, fn) {
  this.listeners[String(type || "")] = fn;
};
Element.prototype.removeEventListener = function () {};
Element.prototype.dispatchEvent = function (ev) {
  var fn = this.listeners[String((ev && ev.type) || "")];
  if (typeof fn === "function") fn(ev);
  return !(ev && ev.defaultPrevented);
};
Element.prototype.setAttribute = function (name, value) {
  this[String(name || "")] = String(value == null ? "" : value);
};
Element.prototype.getAttribute = function (name) {
  var v = this[String(name || "")];
  return v == null ? null : String(v);
};
Element.prototype.getContext = function (kind) {
  if (String(kind || "").toLowerCase() !== "2d") return null;
  return new G.CanvasRenderingContext2D();
};
G.HTMLCanvasElement = G.HTMLCanvasElement || Element;
G.HTMLElement = G.HTMLElement || Element;
G.Element = G.Element || Element;
if (typeof G.MutationObserver !== "function") {
  G.MutationObserver = function () {};
  G.MutationObserver.prototype.observe = function () {};
  G.MutationObserver.prototype.disconnect = function () {};
}
var __trueosBody = new Element("body");
var __trueosHead = new Element("head");
G.document = {
  body: __trueosBody,
  head: __trueosHead,
  createElement: function (tag) { return new Element(tag); },
  getElementById: function (id) { return String(id || "") === "app" ? __trueosBody : null; },
  querySelector: function () { return null; },
  querySelectorAll: function () { return []; },
  getElementsByTagName: function (name) {
    name = String(name || "").toLowerCase();
    if (name === "body") return [__trueosBody];
    if (name === "head") return [__trueosHead];
    if (name === "link") return [];
    return [];
  },
  contains: function () { return true; },
  addEventListener: function () {},
  removeEventListener: function () {}
};
G.window = G;
G.self = G;
G.innerWidth = Math.max(1, __trueosNum(G.innerWidth, 2560) | 0);
G.innerHeight = Math.max(1, __trueosNum(G.innerHeight, 1440) | 0);
G.__trueosWindowListeners = G.__trueosWindowListeners || Object.create(null);
G.addEventListener = function (type, fn) {
  type = String(type || "");
  if (typeof fn !== "function") return;
  var list = G.__trueosWindowListeners[type];
  if (!list) list = G.__trueosWindowListeners[type] = [];
  list.push(fn);
};
G.removeEventListener = function (type, fn) {
  type = String(type || "");
  var list = G.__trueosWindowListeners[type];
  if (!list || typeof fn !== "function") return;
  for (var i = list.length - 1; i >= 0; i -= 1) {
    if (list[i] === fn) list.splice(i, 1);
  }
};
G.dispatchEvent = function (ev) {
  var type = String((ev && ev.type) || "");
  var list = G.__trueosWindowListeners[type] || [];
  for (var i = 0; i < list.length; i += 1) list[i].call(G, ev);
  return !(ev && ev.defaultPrevented);
};
G.__TRUEOS_DISPATCH_KEYDOWN__ = function (key, pointerId, modifiers, slotId) {
  modifiers = Number(modifiers) || 0;
  var ev = {
    type: "keydown",
    key: String(key || ""),
    pointerId: Number(pointerId) || 1,
    slotId: Number(slotId) || 0,
    shiftKey: !!(modifiers & 0x22),
    ctrlKey: !!(modifiers & 0x11),
    altKey: !!(modifiers & 0x44),
    metaKey: !!(modifiers & 0x88),
    defaultPrevented: false,
    preventDefault: function () { this.defaultPrevented = true; },
    stopPropagation: function () { this.propagationStopped = true; }
  };
  var before = (G.__pixiCapture && G.__pixiCapture.commands && G.__pixiCapture.commands.length) || 0;
  G.dispatchEvent(ev);
  var repainted = 0;
  if (G.__TRUEOS_PIXI_DIRTY__ && typeof G.__TRUEOS_REPAINT_NOW__ === "function") {
    G.__TRUEOS_REPAINT_NOW__();
    repainted = 1;
  }
  var after = (G.__pixiCapture && G.__pixiCapture.commands && G.__pixiCapture.commands.length) || before;
  var listeners = (G.__trueosWindowListeners.keydown || []).length;
  return { handled: listeners > 0 ? 1 : 0, listenerCount: listeners, painted: (after > before || repainted) ? 1 : 0, defaultPrevented: ev.defaultPrevented ? 1 : 0 };
};
G.performance = G.performance || { now: function () { return 0; } };
G.requestAnimationFrame = G.requestAnimationFrame || function (fn) {
  if (typeof fn !== "function") return 0;
  if (typeof G.setTimeout === "function") {
    return G.setTimeout(function () { fn(G.performance.now()); }, 16);
  }
  fn(G.performance.now());
  return 1;
};
G.cancelAnimationFrame = G.cancelAnimationFrame || function () {};
G.setTimeout = G.setTimeout || function (fn) { if (typeof fn === "function") fn(); return 1; };
G.clearTimeout = G.clearTimeout || function () {};
G.setInterval = G.setInterval || function () { return 1; };
G.clearInterval = G.clearInterval || function () {};
G.navigator = G.navigator || {};
G.navigator.userAgent = G.navigator.userAgent || "TRUEOS Browser-OS";
G.navigator.sendBeacon = function () { return true; };
G.Blob = G.Blob || function Blob(parts, init) { this.parts = parts || []; this.type = init && init.type || ""; };
"##;
const TRUESURFER_PARSE5_VITE_HOST_FETCH_SOURCE: &[u8] = br##"
var G = (typeof globalThis !== "undefined") ? globalThis : this;
G.__pixiCapture = undefined;
G.__TRUEOS_CAPTURE_ONLY__ = true;
if (typeof G.__TRUEOS_INPUT_HTML__ !== "string") G.__TRUEOS_INPUT_HTML__ = "";
G.__TRUEOS_PIXI_APP = undefined;
G.__TRUEOS_PIXI_APP_READY__ = false;
G.__TRUEOS_PIXI_APP_ERROR__ = "";
G.__TRUEOS_PIXI_APP_PHASE__ = "host:fetch-ready";
if (typeof G.Response !== "function") {
  G.Response = function Response(body, init) {
    this._body = String(body == null ? "" : body);
    this.status = init && init.status ? Number(init.status) | 0 : 200;
    this.ok = this.status >= 200 && this.status < 300;
  };
  G.Response.prototype.text = function () { return Promise.resolve(this._body); };
  G.Response.prototype.json = function () { return Promise.resolve(JSON.parse(this._body)); };
}
G.fetch = function (input, init) {
  var url = String(input && input.url ? input.url : input || "");
  if (url === "/input.html" || url.slice(-11) === "/input.html") {
    return Promise.resolve(new G.Response(String(G.__TRUEOS_INPUT_HTML__ || ""), { status: 200 }));
  }
  return Promise.resolve(new G.Response("", { status: url === "/__pixi_capture" ? 204 : 200 }));
};
"##;
const TRUESURFER_PARSE5_VITE_HOST_CAPTURE_SOURCE: &[u8] = br##"
var G = (typeof globalThis !== "undefined") ? globalThis : this;
function __trueosColorArg(value, fallback) {
  if (typeof value === "number") return value >>> 0;
  if (value && typeof value.color === "number") return value.color >>> 0;
  if (typeof value === "string" && value.charAt(0) === "#") return parseInt(value.slice(1), 16) >>> 0;
  return fallback;
}
function __trueosAlphaArg(value) {
  return value && typeof value.alpha === "number" ? __trueosNum(value.alpha, 1) : 1;
}
function __trueosWidthArg(value) {
  if (value && typeof value.width === "number") return __trueosNum(value.width, 1);
  return value && typeof value.w === "number" ? __trueosNum(value.w, 1) : 1;
}
function __trueosCommandKind(target) {
  target = String(target || "");
  if (target.indexOf("Graphics") >= 0) return 1;
  if (target.indexOf("Text") >= 0) return 2;
  return 0;
}
function __trueosPointerEventCode(event) {
  event = String(event || "");
  if (event === "pointerdown") return 1;
  if (event === "pointerup") return 2;
  if (event === "pointermove") return 3;
  if (event === "pointerover") return 4;
  if (event === "pointerout") return 5;
  if (event === "pointerupoutside") return 6;
  if (event === "contextmenu") return 7;
  return 1;
}
function __trueosTextHasInk(value) {
  var text = String(value == null ? "" : value);
  for (var i = 0; i < text.length; i += 1) {
    var code = text.charCodeAt(i);
    if (
      code > 32 &&
      !(code >= 127 && code <= 160) &&
      code !== 5760 &&
      !(code >= 8192 && code <= 8207) &&
      !(code >= 8232 && code <= 8238) &&
      code !== 8239 &&
      !(code >= 8287 && code <= 8298) &&
      code !== 12288 &&
      !(code >= 65024 && code <= 65039) &&
      code !== 65279
    ) return true;
  }
  return false;
}
function __trueosStripHostMarkers(value) {
  var text = String(value == null ? "" : value);
  var markers = [
    "<truesurfer-parse5-trueos-host-core>",
    "<truesurfer-parse5-trueos-host-event>",
    "<truesurfer-parse5-trueos-host-canvas>",
    "<truesurfer-parse5-trueos-host-dom>",
    "<truesurfer-parse5-trueos-host-fetch>",
    "<truesurfer-parse5-trueos-host-capture>",
    "<truesurfer-parse5-trueos-app.js>",
    "<truesurfer-init>",
    "<node-fetch-shim>"
  ];
  var changed = true;
  while (changed) {
    changed = false;
    for (var i = 0; i < markers.length; i += 1) {
      var marker = markers[i];
      if (text.indexOf(marker) >= 0) {
        text = text.split(marker).join("");
        changed = true;
      }
    }
  }
  if (text.indexOf("<truesurfer-") === 0) return "";
  var strippedPrefix = true;
  while (strippedPrefix) {
    strippedPrefix = false;
    while (text.indexOf("__trueosNum") === 0) {
      text = text.slice("__trueosNum".length);
      strippedPrefix = true;
    }
    while (text.indexOf("__trueosNu") === 0) {
      text = text.slice("__trueosNu".length);
      strippedPrefix = true;
    }
    while (text.indexOf("__trueosN") === 0) {
      text = text.slice("__trueosN".length);
      strippedPrefix = true;
    }
    while (text.indexOf("__trueos") === 0) {
      text = text.slice("__trueos".length);
      strippedPrefix = true;
    }
    while (text.indexOf("Num") === 0) {
      text = text.slice("Num".length);
      strippedPrefix = true;
    }
    while (text.indexOf("Nu") === 0) {
      text = text.slice("Nu".length);
      strippedPrefix = true;
    }
    while (text.indexOf("N") === 0 && (text.length === 1 || text.indexOf("__trueos") === 1)) {
      text = text.slice("N".length);
      strippedPrefix = true;
    }
  }
  if (text.indexOf("__trueo") === 0) return "";
  if (text === "N" || text === "Nu" || text === "Num") return "";
  return text;
}
function __trueosNormalizeCommandOp(value) {
  var op = String(value == null ? "" : value);
  var strippedPrefix = true;
  while (strippedPrefix) {
    strippedPrefix = false;
    while (op.indexOf("__trueosNum") === 0) {
      op = op.slice("__trueosNum".length);
      strippedPrefix = true;
    }
    while (op.indexOf("__trueosNu") === 0) {
      op = op.slice("__trueosNu".length);
      strippedPrefix = true;
    }
    while (op.indexOf("__trueosN") === 0) {
      op = op.slice("__trueosN".length);
      strippedPrefix = true;
    }
    while (op.indexOf("__trueos") === 0) {
      op = op.slice("__trueos".length);
      strippedPrefix = true;
    }
    while (op.indexOf("Num") === 0) {
      op = op.slice("Num".length);
      strippedPrefix = true;
    }
    while (op.indexOf("Nu") === 0) {
      op = op.slice("Nu".length);
      strippedPrefix = true;
    }
    while (op.indexOf("N") === 0) {
      op = op.slice("N".length);
      strippedPrefix = true;
    }
  }
  return op;
}
function __trueosTextLooksInternal(value) {
  var text = String(value == null ? "" : value);
  if (text.indexOf("<truesurfer-") >= 0) return true;
  if (text.indexOf("__trueo") >= 0) return true;
  if (text.indexOf("function __trueos") >= 0) return true;
  if (text.indexOf("G.__trueos") >= 0) return true;
  return false;
}
function __trueosTextIsRenderable(value) {
  var text = String(value == null ? "" : value);
  if (!__trueosTextHasInk(text)) return false;
  if (text === "true" || text === "false") return false;
  if (text === "N" || text === "Nu" || text === "Num") return false;
  if (__trueosTextLooksInternal(text)) return false;
  return true;
}
function __trueosCleanRenderableText(value) {
  var raw = String(value == null ? "" : value);
  var stripped = __trueosStripHostMarkers(raw);
  if (__trueosTextIsRenderable(stripped)) return stripped;
  if (__trueosTextIsRenderable(raw) && !__trueosTextLooksInternal(raw)) return raw;
  return "";
}
function __trueosOverlayTextIsRenderable(value) {
  var text = __trueosStripHostMarkers(value);
  if (!__trueosTextHasInk(text)) return false;
  if (text === "true" || text === "false") return false;
  if (text === "N" || text === "Nu" || text === "Num") return false;
  if (__trueosTextLooksInternal(text)) return false;
  return true;
}
function __trueosTextWithinVisibleSurface(x, y) {
  return x >= 0 && y >= 0 && x < 2560 && y < 1440;
}
function __trueosLogTextSample(value) {
  var s = String(value == null ? "" : value);
  var out = "";
  for (var i = 0; i < s.length && out.length < 96; i += 1) {
    var ch = s.charAt(i);
    out += (ch === "\r" || ch === "\n" || ch === "\t" || ch === "|" || ch === "\"" || ch === "\\") ? "_" : ch;
  }
  return out;
}
function __trueosSnapshotDiag(node, depth, out) {
  if (!node || typeof node !== "object") return out;
  if (depth > out.maxDepth) out.maxDepth = depth;
  out.nodes += 1;
  var text = typeof node.text === "string" ? node.text : "";
  if (text) out.text += 1;
  if (__trueosTextIsRenderable(text)) {
    out.renderableText += 1;
    if (out.samples.length < 12) out.samples.push("#" + out.samples.length + "@node=" + (__trueosNum(node.id, 0) | 0) + " sample=\"" + __trueosLogTextSample(text) + "\"");
  }
  var children = node.children && node.children.length ? node.children : [];
  if (!children.length && node.children) out.nonArrayChildren += 1;
  for (var i = 0; i < children.length; i += 1) __trueosSnapshotDiag(children[i], depth + 1, out);
  return out;
}
function __trueosCollectSnapshotVisibility(node, ox, oy, inheritedVisible, out) {
  if (!node || typeof node !== "object") return;
  var id = __trueosNum(node.id, 0) | 0;
  if (id > 0) {
    var seenKey = "__seen_" + id;
    if (out[seenKey]) return;
    out[seenKey] = true;
  }
  var localX = __trueosNum(node.x, 0);
  var localY = __trueosNum(node.y, 0);
  var worldX = __trueosNum(ox, 0) + localX;
  var worldY = __trueosNum(oy, 0) + localY;
  if (typeof node.globalX === "number" && typeof node.globalY === "number") {
    worldX = __trueosNum(node.globalX, worldX);
    worldY = __trueosNum(node.globalY, worldY);
  }
  var visible = inheritedVisible && node.visible !== false && __trueosNum(node.alpha, 1) > 0;
  if (id > 0 && visible && __trueosTextWithinVisibleSurface(worldX, worldY)) out[id] = true;
  var children = node.children && node.children.length ? node.children : [];
  for (var i = 0; i < children.length; i += 1) __trueosCollectSnapshotVisibility(children[i], worldX, worldY, visible, out);
}
function __trueosPushSnapshotNode(node, parent, ops, seen, snapshotSeen, snapshotTextVisible, snapshotTextWorld, textSlots, ox, oy) {
  if (!node || typeof node !== "object") return 0;
  var id = __trueosNum(node.id, 0) | 0;
  if (id <= 0) return 0;
  if (snapshotSeen[id]) {
    if (parent > 0) ops.push({ code: 2, node: parent, a: id });
    return id;
  }
  var kind = 0;
  var localX = __trueosNum(node.x, 0);
  var localY = __trueosNum(node.y, 0);
  var worldX = __trueosNum(ox, 0) + localX;
  var worldY = __trueosNum(oy, 0) + localY;
  if (typeof node.globalX === "number" && typeof node.globalY === "number") {
    worldX = __trueosNum(node.globalX, worldX);
    worldY = __trueosNum(node.globalY, worldY);
  }
  var type = String(node.type || "");
  var isTextNode = type.indexOf("Text") >= 0;
  var cleanedText = typeof node.text === "string" ? node.text : "";
  var hasRenderableText = cleanedText && __trueosTextIsRenderable(cleanedText);
  var hasVisibleText = hasRenderableText && __trueosTextWithinVisibleSurface(worldX, worldY);
  if (isTextNode && __trueosTextWithinVisibleSurface(worldX, worldY)) {
    snapshotTextWorld[id] = { x: worldX, y: worldY };
  }
  if (!seen[id]) {
    kind = hasVisibleText ? 2 : (type.indexOf("Graphics") >= 0 ? 1 : (isTextNode ? 2 : 0));
    ops.push({ code: 1, node: id, a: kind });
    seen[id] = true;
  }
  snapshotSeen[id] = true;
  if (hasVisibleText) {
    snapshotTextVisible[id] = true;
    snapshotTextWorld[id] = { x: worldX, y: worldY };
  }
  if (kind === 2 && textSlots) textSlots.push(id);
  if (parent > 0) ops.push({ code: 2, node: parent, a: id });
  ops.push({ code: 3, node: id, a: localX, b: localY });
  if (typeof node.scaleX === "number" || typeof node.scaleY === "number") {
    var sx = __trueosNum(node.scaleX, 1);
    var sy = __trueosNum(node.scaleY, sx);
    if (sx !== 1 || sy !== 1) ops.push({ code: 28, node: id, a: sx, b: sy });
  }
  if (node.visible === false) ops.push({ code: 15, node: id, a: 0 });
  if (typeof node.alpha === "number" && node.alpha !== 1) ops.push({ code: 23, node: id, a: __trueosNum(node.alpha, 1) });
  if (__trueosNum(node.maskId, 0) > 0) ops.push({ code: 27, node: id, a: __trueosNum(node.maskId, 0) });
  var children = node.children && node.children.length ? node.children : [];
  if (node.sortableChildren === true && children.length > 1) {
    children = children.slice();
    children.sort(function (a, b) {
      return __trueosNum(a && a.zIndex, 0) - __trueosNum(b && b.zIndex, 0);
    });
  }
  for (var i = 0; i < children.length; i += 1) __trueosPushSnapshotNode(children[i], id, ops, seen, snapshotSeen, snapshotTextVisible, snapshotTextWorld, textSlots, worldX, worldY);
  return id;
}
function __trueosPushLayoutTextOverlays(node, parentId, ox, oy, ops, state) {
  if (!node || typeof node !== "object") return;
  var x = ox + __trueosNum(node.x, 0);
  var y = oy + __trueosNum(node.y, 0);
  var cleanedText = __trueosStripHostMarkers(node.text);
  if (node.kind === "text" && __trueosOverlayTextIsRenderable(cleanedText)) {
    var id = state.nextId++;
    var text = cleanedText;
    ops.push({ code: 1, node: id, a: 2 });
    if (parentId > 0) ops.push({ code: 2, node: parentId, a: id });
    ops.push({ code: 3, node: id, a: x, b: y });
    ops.push({ code: 9, node: id, a: 0x111111, b: 1 });
    ops.push({ code: 8, node: id, text: text });
    state.text += 1;
    if (state.samples.length < 12) state.samples.push("#" + state.samples.length + "@node=" + id + " sample=\"" + __trueosLogTextSample(text) + "\"");
  }
  var children = node.children && node.children.length ? node.children : [];
  for (var i = 0; i < children.length; i += 1) __trueosPushLayoutTextOverlays(children[i], parentId, x, y, ops, state);
}
function __trueosPushFlatTextOverlay(item, parentId, ops, state) {
  if (!item || typeof item !== "object") return;
  var text = __trueosStripHostMarkers(item.text);
  var visibleSample = __trueosLogTextSample(text);
  if (!visibleSample) return;
  if (visibleSample === "true" || visibleSample === "false") return;
  if (visibleSample.indexOf("<truesurfer-") === 0) return;
  if (visibleSample.indexOf("__trueo") === 0) return;
  if (!__trueosOverlayTextIsRenderable(text)) return;
  var id = state.nextId++;
  ops.push({ code: 1, node: id, a: 2 });
  if (parentId > 0) ops.push({ code: 2, node: parentId, a: id });
  ops.push({ code: 3, node: id, a: __trueosNum(item.x, 0), b: __trueosNum(item.y, 0) });
  ops.push({ code: 9, node: id, a: 0x111111, b: 1 });
  ops.push({ code: 8, node: id, text: text });
  state.text += 1;
  if (state.samples.length < 12) state.samples.push("#" + state.samples.length + "@node=" + id + " sample=\"" + __trueosLogTextSample(text) + "\"");
}
G.__trueosParse5BuildSceneFromCapture = function () {
  var cap = G.__pixiCapture;
  var commands = cap && cap.commands && cap.commands.length ? cap.commands : [];
  var ops = [];
  var seen = {};
  var snapshotSeen = {};
  var snapshotTextVisible = {};
  var snapshotTextWorld = {};
  var snapshotNodeVisible = {};
  var textSlots = [];
  var textMap = {};
  var textHasFill = {};
  var pendingTextFill = {};
  var declaredText = {};
  var finalTextById = {};
  var finalTextQueued = {};
  var finalTextOrder = [];
  var replayTextEmitted = 0;
  var rootId = 0;
  var snapshot = null;
  var replayStart = 0;
  var i;
  for (i = commands.length - 1; i >= 0; i -= 1) {
    if (commands[i] && __trueosNormalizeCommandOp(commands[i].op) === "snapshot" && commands[i].args && commands[i].args[0]) {
      snapshot = commands[i].args[0];
      rootId = __trueosNum(commands[i].id, __trueosNum(snapshot.id, 0)) | 0;
      for (var si = i - 1; si >= 0; si -= 1) {
        if (commands[si] && __trueosNormalizeCommandOp(commands[si].op) === "snapshot") {
          replayStart = si + 1;
          break;
        }
      }
      break;
    }
  }
  var hasSnapshot = !!snapshot;
  if (G.console && typeof G.console.log === "function") {
    var diag = __trueosSnapshotDiag(snapshot, 0, { nodes: 0, text: 0, renderableText: 0, nonArrayChildren: 0, maxDepth: 0, samples: [] });
    var textSetAll = 0;
    var textSetReplay = 0;
    var textSetRenderable = 0;
    var textSetSamples = [];
    for (var ci = 0; ci < commands.length; ci += 1) {
      if (commands[ci] && __trueosNormalizeCommandOp(commands[ci].op) === "text.text.set") {
        textSetAll += 1;
        var textSetValue = String(commands[ci].args && commands[ci].args.length ? commands[ci].args[0] : "");
        if (__trueosTextIsRenderable(textSetValue)) textSetRenderable += 1;
        if (textSetSamples.length < 18) textSetSamples.push("#" + textSetSamples.length + "@id=" + (__trueosNum(commands[ci].id, 0) | 0) + " target=\"" + __trueosLogTextSample(commands[ci].target) + "\" value=\"" + __trueosLogTextSample(textSetValue) + "\"");
        if (ci >= replayStart) textSetReplay += 1;
      }
    }
    G.console.log("[parse5 trueos host] snapshot-diag has=" + (hasSnapshot ? 1 : 0) + " nodes=" + diag.nodes + " text=" + diag.text + " renderable_text=" + diag.renderableText + " max_depth=" + diag.maxDepth + " non_array_children=" + diag.nonArrayChildren + " commands=" + commands.length + " replay_start=" + replayStart + " text_set_all=" + textSetAll + " text_set_replay=" + textSetReplay + " text_set_renderable=" + textSetRenderable + " samples=" + diag.samples.join("|") + " text_sets=" + textSetSamples.join("|"));
  }
  if (snapshot) {
    if (G.console && typeof G.console.log === "function") G.console.log("[parse5 trueos host] scene-build phase=snapshot-visibility begin");
    __trueosCollectSnapshotVisibility(snapshot, 0, 0, true, snapshotNodeVisible);
    if (G.console && typeof G.console.log === "function") G.console.log("[parse5 trueos host] scene-build phase=snapshot-push begin");
    rootId = __trueosPushSnapshotNode(snapshot, 0, ops, seen, snapshotSeen, snapshotTextVisible, snapshotTextWorld, textSlots, 0, 0) || rootId;
    if (G.console && typeof G.console.log === "function") G.console.log("[parse5 trueos host] scene-build phase=snapshot-push done ops=" + ops.length);
  }
  function __trueosMappedTextNode(commandId, textValueHasInk, commandWasSnapshotSeen) {
    return commandId;
  }
  function __trueosDeclareTextNode(nodeId) {
    if (!declaredText[nodeId]) {
      ops.push({ code: 1, node: nodeId, a: 2 });
      declaredText[nodeId] = true;
    }
    seen[nodeId] = true;
  }
  if (G.console && typeof G.console.log === "function") G.console.log("[parse5 trueos host] scene-build phase=text-prepass begin commands=" + commands.length);
  var preRawRenderable = 0;
  var preCleanRenderable = 0;
  var preTextSamples = [];
  for (var preTextIndex = 0; preTextIndex < commands.length; preTextIndex += 1) {
    var preTextCmd = commands[preTextIndex] || {};
    if (__trueosNormalizeCommandOp(preTextCmd.op) !== "text.text.set") continue;
    var preTextId = __trueosNum(preTextCmd.id, 0) | 0;
    if (preTextId <= 0) continue;
    var preTextArgs = preTextCmd.args && preTextCmd.args.length ? preTextCmd.args : [];
    var preTextRaw = String(preTextArgs[0] == null ? "" : preTextArgs[0]);
    var preTextValue = __trueosCleanRenderableText(preTextRaw);
    if (__trueosTextIsRenderable(preTextRaw)) preRawRenderable += 1;
    if (__trueosTextIsRenderable(preTextValue)) preCleanRenderable += 1;
    if (preTextSamples.length < 24 && (__trueosTextIsRenderable(preTextRaw) || __trueosTextIsRenderable(preTextValue))) {
      preTextSamples.push("#" + preTextSamples.length + "@i=" + preTextIndex + " id=" + preTextId + " raw=\"" + __trueosLogTextSample(preTextRaw) + "\" clean=\"" + __trueosLogTextSample(preTextValue) + "\"");
    }
    if (!__trueosTextIsRenderable(preTextValue)) continue;
    if (!finalTextQueued[preTextId]) {
      finalTextQueued[preTextId] = true;
      finalTextOrder.push(preTextId);
    }
    finalTextById[preTextId] = preTextValue;
  }
  if (G.console && typeof G.console.log === "function") G.console.log("[parse5 trueos host] scene-build phase=text-prepass done final_text=" + finalTextOrder.length + " raw_renderable=" + preRawRenderable + " clean_renderable=" + preCleanRenderable + " samples=" + preTextSamples.join("|"));
  if (G.console && typeof G.console.log === "function") G.console.log("[parse5 trueos host] scene-build phase=replay begin start=" + replayStart + " commands=" + commands.length);
  for (i = replayStart; i < commands.length; i += 1) {
    var cmd = commands[i] || {};
    var id = __trueosNum(cmd.id, 0) | 0;
    if (id <= 0) continue;
    var wasSnapshotSeen = !!snapshotSeen[id];
    var wasSnapshotTextVisible = !!snapshotTextVisible[id];
    if (!seen[id]) {
      ops.push({ code: 1, node: id, a: __trueosCommandKind(cmd.target) });
      seen[id] = true;
    }
    var args = cmd.args && cmd.args.length ? cmd.args : [];
    switch (__trueosNormalizeCommandOp(cmd.op)) {
      case "clear": ops.push({ code: 4, node: id }); break;
      case "rect":
        ops.push({ code: 5, node: id, a: __trueosNum(args[0], 0), b: __trueosNum(args[1], 0), c: __trueosNum(args[2], 0), d: __trueosNum(args[3], 0) });
        break;
      case "roundRect":
        ops.push({ code: 24, node: id, a: __trueosNum(args[0], 0), b: __trueosNum(args[1], 0), c: __trueosNum(args[2], 0), d: __trueosNum(args[3], 0), text: String(__trueosNum(args[4], 0)) });
        break;
      case "circle": ops.push({ code: 18, node: id, a: __trueosNum(args[0], 0), b: __trueosNum(args[1], 0), c: __trueosNum(args[2], 0) }); break;
      case "ellipse": ops.push({ code: 26, node: id, a: __trueosNum(args[0], 0), b: __trueosNum(args[1], 0), c: __trueosNum(args[2], 0), d: __trueosNum(args[3], 0) }); break;
      case "moveTo": ops.push({ code: 19, node: id, a: __trueosNum(args[0], 0), b: __trueosNum(args[1], 0) }); break;
      case "lineTo": ops.push({ code: 20, node: id, a: __trueosNum(args[0], 0), b: __trueosNum(args[1], 0) }); break;
      case "closePath": ops.push({ code: 25, node: id }); break;
      case "image": ops.push({ code: 22, node: id, a: __trueosNum(args[0], 0), b: __trueosNum(args[1], 0), c: __trueosNum(args[2], 0), d: __trueosNum(args[3], 0), text: String(__trueosNum(args[4], 0)) }); break;
      case "poly":
        var points = args[0] && args[0].length ? args[0] : [];
        if (points.length >= 2) {
          ops.push({ code: 19, node: id, a: __trueosNum(points[0], 0), b: __trueosNum(points[1], 0) });
          for (var pi = 2; pi + 1 < points.length; pi += 2) {
            ops.push({ code: 20, node: id, a: __trueosNum(points[pi], 0), b: __trueosNum(points[pi + 1], 0) });
          }
        }
        break;
      case "fill": ops.push({ code: 6, node: id, a: __trueosColorArg(args[0], 0xffffff), b: __trueosAlphaArg(args[0]) }); break;
      case "stroke": ops.push({ code: 7, node: id, a: __trueosColorArg(args[0], 0xffffff), b: __trueosAlphaArg(args[0]), c: __trueosWidthArg(args[0]) }); break;
      case "addChild":
        if (__trueosNum(args[0], 0) > 0) ops.push({ code: 2, node: id, a: __trueosNum(args[0], 0) });
        break;
      case "addChildAt":
        if (__trueosNum(args[0], 0) > 0) ops.push({ code: 10, node: id, a: __trueosNum(args[0], 0), b: __trueosNum(args[1], 0) });
        break;
      case "setChildIndex":
        if (__trueosNum(args[0], 0) > 0) ops.push({ code: 11, node: id, a: __trueosNum(args[0], 0), b: __trueosNum(args[1], 0) });
        break;
      case "position":
        ops.push({ code: 3, node: id, a: __trueosNum(args[0], 0), b: __trueosNum(args[1], 0) });
        break;
      case "removeChild":
        if (!hasSnapshot && __trueosNum(args[0], 0) > 0) ops.push({ code: 12, node: id, a: __trueosNum(args[0], 0) });
        break;
      case "removeChildren": if (!hasSnapshot) ops.push({ code: 14, node: id }); break;
      case "removeAllListeners": if (!hasSnapshot) ops.push({ code: 17, node: id }); break;
      case "on": if (cmd.event) ops.push({ code: 16, node: id, a: __trueosPointerEventCode(cmd.event) }); break;
      case "alpha": ops.push({ code: 23, node: id, a: __trueosNum(args[0], 1) }); break;
      case "scale": ops.push({ code: 28, node: id, a: __trueosNum(args[0], 1), b: __trueosNum(args[1], __trueosNum(args[0], 1)) }); break;
      case "mask": ops.push({ code: 27, node: id, a: __trueosNum(args[0], 0) }); break;
      case "text.text.set":
        var textValue = __trueosCleanRenderableText(args[0]);
        var hasInk = __trueosTextIsRenderable(textValue);
        if (hasSnapshot && (!wasSnapshotSeen || !snapshotNodeVisible[id])) break;
        if (!finalTextQueued[id]) {
          finalTextQueued[id] = true;
          finalTextOrder.push(id);
        }
        if (hasInk || !__trueosTextIsRenderable(finalTextById[id])) finalTextById[id] = hasInk ? textValue : "";
        break;
      case "text.style.set":
        if (args[0] && typeof args[0].fill !== "undefined") {
          var fill = __trueosColorArg(args[0].fill, 0xffffff);
          pendingTextFill[id] = fill;
        }
        break;
      }
  }
  if (G.console && typeof G.console.log === "function") G.console.log("[parse5 trueos host] scene-build phase=replay done final_text=" + finalTextOrder.length + " ops=" + ops.length);
  if (G.console && typeof G.console.log === "function") G.console.log("[parse5 trueos host] scene-build phase=final-text begin count=" + finalTextOrder.length);
  for (var ti = 0; ti < finalTextOrder.length; ti += 1) {
    var finalId = finalTextOrder[ti] | 0;
    var finalText = finalTextById[finalId];
    if (!__trueosTextIsRenderable(finalText)) continue;
    var finalTextNode = __trueosMappedTextNode(finalId, true, !!snapshotSeen[finalId]);
    var worldText = hasSnapshot ? snapshotTextWorld[finalId] : null;
    if (worldText) {
      finalTextNode = 800000 + finalId;
      ops.push({ code: 1, node: finalTextNode, a: 2 });
      if (rootId > 0) ops.push({ code: 2, node: rootId, a: finalTextNode });
      ops.push({ code: 3, node: finalTextNode, a: __trueosNum(worldText.x, 0), b: __trueosNum(worldText.y, 0) });
    } else {
      __trueosDeclareTextNode(finalTextNode);
    }
    if (typeof pendingTextFill[finalId] === "number") {
      ops.push({ code: 9, node: finalTextNode, a: pendingTextFill[finalId], b: 1 });
      textHasFill[finalTextNode] = true;
    }
    if (!textHasFill[finalTextNode]) ops.push({ code: 9, node: finalTextNode, a: 0x111111, b: 1 });
    ops.push({ code: 8, node: finalTextNode, text: finalText });
    replayTextEmitted += 1;
  }
  if (G.console && typeof G.console.log === "function") G.console.log("[parse5 trueos host] scene-build phase=final-text done emitted=" + replayTextEmitted + " ops=" + ops.length);
  var flatTextOverlays = G.__TRUEOS_PIXI_LAYOUT_TEXT_OVERLAYS__;
  var layoutOverlay = G.__TRUEOS_PIXI_LAST_LAYOUT__;
  var layoutOverlayState = { nextId: 900000, text: 0, samples: [] };
  var enableJsLayoutTextOverlay = false;
  if (enableJsLayoutTextOverlay && flatTextOverlays && flatTextOverlays.length) {
    for (var oi = 0; oi < flatTextOverlays.length; oi += 1) {
      __trueosPushFlatTextOverlay(flatTextOverlays[oi], rootId || 1, ops, layoutOverlayState);
    }
  } else if (enableJsLayoutTextOverlay && layoutOverlay) {
    __trueosPushLayoutTextOverlays(layoutOverlay, rootId || 1, 0, 0, ops, layoutOverlayState);
  }
  if (G.console && typeof G.console.log === "function") {
    if ((flatTextOverlays && flatTextOverlays.length) || layoutOverlayState.text > 0) {
      var flatSamples = [];
      if (flatTextOverlays && flatTextOverlays.length) {
        for (var fi = 0; fi < flatTextOverlays.length && flatSamples.length < 8; fi += 1) {
          var flatItem = flatTextOverlays[fi] || {};
          flatSamples.push("#" + flatSamples.length + " x=" + __trueosNum(flatItem.x, 0) + " y=" + __trueosNum(flatItem.y, 0) + " text=\"" + __trueosLogTextSample(flatItem.text) + "\"");
        }
      }
      G.console.log("[parse5 trueos host] layout-text-overlay flat_len=" + (flatTextOverlays && flatTextOverlays.length ? flatTextOverlays.length : 0) + " count=" + layoutOverlayState.text + " samples=" + layoutOverlayState.samples.join("|") + " flat_samples=" + flatSamples.join("|"));
    }
    var textLog = [];
    for (i = 0; i < ops.length && textLog.length < 12; i += 1) {
      if (ops[i] && ops[i].code === 8) {
        textLog.push("#" + textLog.length + "@node=" + ops[i].node + " chars=" + String(ops[i].text || "").length + " sample=\"" + __trueosLogTextSample(ops[i].text) + "\"");
      }
    }
    G.console.log("[parse5 trueos host] ui3-scene-text emitted=" + replayTextEmitted + " " + textLog.join("|"));
  }
  return { ok: 1, ui3Scene: { version: 1, commandSource: "parse5-trueos-pixi", rootId: rootId, opCount: ops.length, layoutTextOps: layoutOverlayState.text, ops: ops } };
};
"##;
const TRUESURFER_IMPORT_SOURCE: &[u8] = br#"
globalThis.__trueosTruesurferReady = 0;
globalThis.__trueosTruesurferWarmup = {
  status: 'loading-entry',
  baseUrl: '/qjs/truesurfer/truesurfer.mjs',
};
if (typeof globalThis.importModule !== 'function') {
  globalThis.__trueosTruesurferReady = -1;
  globalThis.__trueosTruesurferWarmup = {
    status: 'error',
    baseUrl: '/qjs/truesurfer/truesurfer.mjs',
    error: 'importModule is not available',
  };
  throw new Error('importModule is not available');
}
globalThis.__trueosTruesurferEntryPromise = Promise.resolve(
  globalThis.importModule('/qjs/truesurfer/truesurfer.mjs'),
).catch((error) => {
  const message = error && error.stack ? String(error.stack) : String(error || 'unknown truesurfer import error');
  globalThis.__trueosTruesurferReady = -1;
  globalThis.__trueosTruesurferWarmup = {
    status: 'error',
    baseUrl: '/qjs/truesurfer/truesurfer.mjs',
    error: message,
  };
  throw error;
});
"#;
const TRUESURFER_READY_PROP: &[u8] = b"__trueosTruesurferReady\0";
const TRUESURFER_ID_PROP: &[u8] = b"__trueosTruesurferBrowserId\0";
const TRUESURFER_OBJ_PROP: &[u8] = b"__trueosTruesurfer\0";
const TRUESURFER_SET_HTML_PROP: &[u8] = b"setHtml\0";
const TRUESURFER_META_URL_PROP: &[u8] = b"url\0";
const TRUESURFER_RESULT_OK_PROP: &[u8] = b"ok\0";
const TRUESURFER_RESULT_BYTES_PROP: &[u8] = b"bytes\0";
const TRUESURFER_RESULT_LINES_PROP: &[u8] = b"lines\0";
const TRUESURFER_RESULT_PARSE_MS_PROP: &[u8] = b"parseMs\0";
const TRUESURFER_RESULT_TITLE_PROP: &[u8] = b"title\0";
const TRUESURFER_RESULT_FAVICON_URL_PROP: &[u8] = b"faviconUrl\0";
const TRUESURFER_RESULT_SHELL_BYTES_PROP: &[u8] = b"shellBytes\0";
const TRUESURFER_RESULT_BODY_BYTES_PROP: &[u8] = b"bodyBytes\0";
const TRUESURFER_RESULT_UI3_SCENE_PROP: &[u8] = b"ui3Scene\0";
const TRUESURFER_RESULT_STYLE_COUNT_PROP: &[u8] = b"styleCount\0";
const TRUESURFER_RESULT_STYLE_BYTES_PROP: &[u8] = b"styleBytes\0";
const TRUESURFER_RESULT_SCRIPT_COUNT_PROP: &[u8] = b"scriptCount\0";
const TRUESURFER_RESULT_SCRIPT_BYTES_PROP: &[u8] = b"scriptBytes\0";
const TRUESURFER_RESULT_ERROR_PROP: &[u8] = b"error\0";
const TRUESURFER_TRUEOS_INPUT_HTML_PROP: &[u8] = b"__TRUEOS_INPUT_HTML__\0";
const TRUESURFER_TRUEOS_PIXI_APP_READY_PROP: &[u8] = b"__TRUEOS_PIXI_APP_READY__\0";
const TRUESURFER_TRUEOS_PIXI_APP_ERROR_PROP: &[u8] = b"__TRUEOS_PIXI_APP_ERROR__\0";
const TRUESURFER_TRUEOS_PIXI_APP_PHASE_PROP: &[u8] = b"__TRUEOS_PIXI_APP_PHASE__\0";
const TRUESURFER_TRUEOS_PIXI_CAPTURE_ERROR_PROP: &[u8] = b"__TRUEOS_PIXI_CAPTURE_ERROR__\0";
const TRUESURFER_TRUEOS_PIXI_CAPTURE_STEP_PROP: &[u8] = b"__TRUEOS_PIXI_CAPTURE_STEP__\0";
const TRUESURFER_TRUEOS_PIXI_LAYOUT_STEP_PROP: &[u8] = b"__TRUEOS_PIXI_LAYOUT_STEP__\0";
const TRUESURFER_TRUEOS_PIXI_BRIDGE_STATS_PROP: &[u8] = b"__TRUEOS_PIXI_BRIDGE_STATS__\0";
const TRUESURFER_TRUEOS_PIXI_LAYOUT_TEXT_OVERLAYS_PROP: &[u8] =
    b"__TRUEOS_PIXI_LAYOUT_TEXT_OVERLAYS__\0";
const TRUESURFER_TRUEOS_PIXI_POINTER_DISPATCH_PROP: &[u8] = b"__TRUEOS_DISPATCH_PIXI_POINTER__\0";
const TRUESURFER_TRUEOS_PIXI_KEYDOWN_DISPATCH_PROP: &[u8] = b"__TRUEOS_DISPATCH_KEYDOWN__\0";
const TRUESURFER_TRUEOS_PIXI_REPAINT_NOW_PROP: &[u8] = b"__TRUEOS_REPAINT_NOW__\0";
const TRUESURFER_PARSE5_BUILD_SCENE_PROP: &[u8] = b"__trueosParse5BuildSceneFromCapture\0";
const TRUESURFER_BUILD_TEXT_WIDGET_SCENE_PROP: &[u8] = b"__trueosBuildTextWidgetScene\0";
const TRUESURFER_BUILD_DEMO_TEXT_WIDGET_SCENE_PROP: &[u8] = b"__trueosBuildDemoTextWidgetScene\0";
const TRUESURFER_UI3_SCENE_COMMAND_SOURCE_PROP: &[u8] = b"commandSource\0";
const TRUESURFER_UI3_SCENE_ROOT_ID_PROP: &[u8] = b"rootId\0";
const TRUESURFER_UI3_SCENE_LAYOUT_TEXT_OPS_PROP: &[u8] = b"layoutTextOps\0";
const TRUESURFER_UI3_SCENE_OPS_PROP: &[u8] = b"ops\0";
const TRUESURFER_WIDGET_PROP: &[u8] = b"widget\0";
const TRUESURFER_WIDGET_RENDERER_PROP: &[u8] = b"renderer\0";
const TRUESURFER_WIDGET_TAGS_PROP: &[u8] = b"tags\0";
const TRUESURFER_WIDGET_TAG_COUNTS_PROP: &[u8] = b"tagCounts\0";
const TRUESURFER_WIDGET_COUNT_PROP: &[u8] = b"widgetCount\0";
const TRUESURFER_WIDGET_BUTTON_COUNT_PROP: &[u8] = b"buttonCount\0";
const TRUESURFER_WIDGET_IFRAME_COUNT_PROP: &[u8] = b"iframeCount\0";
const TRUESURFER_WIDGET_IFRAME_SRCDOC_COUNT_PROP: &[u8] = b"iframeSrcdocCount\0";
const TRUESURFER_WIDGET_TEXT_COUNT_PROP: &[u8] = b"textCount\0";
const TRUESURFER_WIDGET_TEXT_BYTES_PROP: &[u8] = b"textBytes\0";
const TRUESURFER_UI3_OP_CODE_PROP: &[u8] = b"code\0";
const TRUESURFER_UI3_OP_NODE_PROP: &[u8] = b"node\0";
const TRUESURFER_UI3_OP_A_PROP: &[u8] = b"a\0";
const TRUESURFER_UI3_OP_B_PROP: &[u8] = b"b\0";
const TRUESURFER_UI3_OP_C_PROP: &[u8] = b"c\0";
const TRUESURFER_UI3_OP_D_PROP: &[u8] = b"d\0";
const TRUESURFER_UI3_OP_TEXT_PROP: &[u8] = b"text\0";
const TRUESURFER_LAYOUT_TEXT_X_PROP: &[u8] = b"x\0";
const TRUESURFER_LAYOUT_TEXT_Y_PROP: &[u8] = b"y\0";
const TRUESURFER_BRIDGE_RENDER_NODES_PROP: &[u8] = b"renderNodes\0";
const TRUESURFER_BRIDGE_RENDER_BLOCKS_PROP: &[u8] = b"renderBlocks\0";
const TRUESURFER_BRIDGE_RENDER_TEXT_PROP: &[u8] = b"renderText\0";
const TRUESURFER_BRIDGE_RENDER_TAGS_PROP: &[u8] = b"renderTags\0";
const TRUESURFER_BRIDGE_RENDER_TEXT_SAMPLES_PROP: &[u8] = b"renderTextSamples\0";
const TRUESURFER_BRIDGE_LAYOUT_BOXES_PROP: &[u8] = b"layoutBoxes\0";
const TRUESURFER_BRIDGE_LAYOUT_BLOCKS_PROP: &[u8] = b"layoutBlocks\0";
const TRUESURFER_BRIDGE_LAYOUT_TEXT_PROP: &[u8] = b"layoutText\0";
const TRUESURFER_BRIDGE_LAYOUT_MAX_DEPTH_PROP: &[u8] = b"layoutMaxDepth\0";
const TRUESURFER_BRIDGE_LAYOUT_TEXT_SAMPLES_PROP: &[u8] = b"layoutTextSamples\0";
const TRUESURFER_BRIDGE_MEASURE_TEXT_CALLS_PROP: &[u8] = b"measureTextCalls\0";
const TRUESURFER_BRIDGE_PIXI_COMMANDS_PROP: &[u8] = b"pixiCommands\0";
const TRUESURFER_BRIDGE_PIXI_OPS_PROP: &[u8] = b"pixiOps\0";
const TRUESURFER_BRIDGE_PIXI_UNSUPPORTED_PROP: &[u8] = b"pixiUnsupported\0";
const TRUESURFER_HTML_QUEUE_DEPTH: usize = 2;
const TRUESURFER_HTML_QUEUE_WAIT_MS: u64 = 2;
const TRUESURFER_BUSY_PUMP_BUDGET: usize = 512;
const TRUESURFER_BUSY_SLEEP_MS: u64 = 1;
const TRUESURFER_PARSE5_ASSET_PUMP_BUDGET: usize = 1024;
const TRUESURFER_PARSE5_ASSET_WAIT_MS: u64 = 850;
const TRUESURFER_UI3_SCENE_OP_LIMIT: u32 = 8192;
const TRUESURFER_PARSE5_VISIBLE_TEXT_WIDTH: f32 = 2560.0;
const TRUESURFER_PARSE5_VISIBLE_TEXT_HEIGHT: f32 = 1440.0;
const UI2_HOSTED_BROWSER_DIRTY_CONTENT: u32 = 1 << 0;
const UI2_HOSTED_BROWSER_DIRTY_INTERACTIVE: u32 = 1 << 1;

struct SpinRawMutex(Mutex<()>);

unsafe impl RawMutex for SpinRawMutex {
    const INIT: Self = Self(Mutex::new(()));

    fn lock<R>(&self, f: impl FnOnce() -> R) -> R {
        let _guard = self.0.lock();
        f()
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HostedBrowserSurfaceState {
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub content_width: u32,
    pub content_height: u32,
    pub scroll_x: u32,
    pub scroll_y: u32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HostedBrowserInteractiveItem {
    pub item_id: u32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HostedBrowserInteractiveState {
    pub interactives: alloc::vec::Vec<HostedBrowserInteractiveItem>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HostedBrowserGadget {
    pub node_id: u32,
    pub tag: String,
    pub text: String,
    pub x_px: u32,
    pub y_px: u32,
    pub width_px: u32,
    pub height_px: u32,
    pub font_size_px: u32,
    pub line_height_px: u32,
    pub text_color_rgb: u32,
    pub button_like: bool,
    pub tex_id: u32,
    pub changed: bool,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HostedBrowserGadgetSnapshot {
    pub version: u32,
    pub background_color_rgb: u32,
    pub gadgets: Vec<HostedBrowserGadget>,
}

#[derive(Clone, Debug)]
pub enum HostedKeyboardEvent {
    Text { text: String },
    Key { key: String, modifiers: u8 },
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ParseResult {
    pub ok: bool,
    pub url: String,
    pub bytes: u32,
    pub lines: u32,
    pub parse_ms: u32,
    pub title: String,
    pub favicon_url: String,
    pub shell_bytes: u32,
    pub body_bytes: u32,
    pub style_count: u32,
    pub style_bytes: u32,
    pub script_count: u32,
    pub script_bytes: u32,
    pub error: String,
}

#[derive(Clone, Debug)]
struct PendingHtml {
    html: String,
    url: String,
}

#[derive(Clone, Debug)]
struct QueuedUi3PointerEvent {
    target_node: u32,
    kind: String,
    x: i32,
    y: i32,
    pointer_id: u32,
    buttons: u32,
}

#[derive(Clone, Debug)]
struct QueuedUi3KeyboardEvent {
    key: String,
    slot_id: u32,
    pointer_id: u32,
    modifiers: u32,
}

#[derive(Clone, Default)]
struct HtmlHandoffSlot {
    html: String,
    url: String,
}

struct BrowserHtmlQueue {
    sender: Mutex<Sender<'static, SpinRawMutex, HtmlHandoffSlot>>,
    receiver: Mutex<Receiver<'static, SpinRawMutex, HtmlHandoffSlot>>,
}

struct BrowserUi3PointerQueue {
    queue: Mutex<VecDeque<QueuedUi3PointerEvent>>,
}

struct BrowserUi3KeyboardQueue {
    queue: Mutex<VecDeque<QueuedUi3KeyboardEvent>>,
}

#[derive(Default)]
struct BrowserInstanceState {
    started: bool,
    api_ready: bool,
    last_parse_result: Option<ParseResult>,
    gadget_snapshot: HostedBrowserGadgetSnapshot,
    window_id: u32,
    render_tex_id: u32,
    surface_seq: u32,
    interactive_seq: u32,
    gadget_seq: u32,
    surface_state: HostedBrowserSurfaceState,
}

static TRUESURFER_STATE: Mutex<BTreeMap<u32, BrowserInstanceState>> = Mutex::new(BTreeMap::new());
static BROWSER_RPC_SEQ: AtomicU32 = AtomicU32::new(1);
static TRUESURFER_HTML_QUEUES: Once<Vec<BrowserHtmlQueue>> = Once::new();
static TRUESURFER_UI3_POINTER_QUEUES: Once<Vec<BrowserUi3PointerQueue>> = Once::new();
static TRUESURFER_UI3_KEYBOARD_QUEUES: Once<Vec<BrowserUi3KeyboardQueue>> = Once::new();
static TRUESURFER_HTML_READY: [Signal<SpinRawMutex, ()>; MAX_BROWSER_INSTANCE_ID as usize] =
    [const { Signal::new() }; MAX_BROWSER_INSTANCE_ID as usize];
static TRUESURFER_UI3_POINTER_QUEUE_LOGS: AtomicU32 = AtomicU32::new(0);
static TRUESURFER_UI3_POINTER_LOOP_LOGS: AtomicU32 = AtomicU32::new(0);
static TRUESURFER_UI3_KEYBOARD_QUEUE_LOGS: AtomicU32 = AtomicU32::new(0);

const TRUESURFER_UI3_POINTER_QUEUE_DEPTH: usize = 256;
const TRUESURFER_UI3_KEYBOARD_QUEUE_DEPTH: usize = 256;

fn html_handoff_queues() -> &'static Vec<BrowserHtmlQueue> {
    TRUESURFER_HTML_QUEUES.call_once(|| {
        let mut queues = Vec::with_capacity(MAX_BROWSER_INSTANCE_ID as usize);
        for _ in 0..MAX_BROWSER_INSTANCE_ID {
            let slots: &'static mut [HtmlHandoffSlot] = Box::leak(
                vec![HtmlHandoffSlot::default(); TRUESURFER_HTML_QUEUE_DEPTH].into_boxed_slice(),
            );
            let channel: &'static mut Channel<'static, SpinRawMutex, HtmlHandoffSlot> =
                Box::leak(Box::new(Channel::new(slots)));
            let (sender, receiver) = channel.split();
            queues.push(BrowserHtmlQueue {
                sender: Mutex::new(sender),
                receiver: Mutex::new(receiver),
            });
        }
        queues
    })
}

fn html_handoff_queue(browser_instance_id: u32) -> Option<&'static BrowserHtmlQueue> {
    if !browser_valid(browser_instance_id) {
        return None;
    }
    html_handoff_queues().get(browser_instance_id.saturating_sub(1) as usize)
}

fn ui3_pointer_queues() -> &'static Vec<BrowserUi3PointerQueue> {
    TRUESURFER_UI3_POINTER_QUEUES.call_once(|| {
        let mut queues = Vec::with_capacity(MAX_BROWSER_INSTANCE_ID as usize);
        for _ in 0..MAX_BROWSER_INSTANCE_ID {
            queues.push(BrowserUi3PointerQueue {
                queue: Mutex::new(VecDeque::with_capacity(TRUESURFER_UI3_POINTER_QUEUE_DEPTH)),
            });
        }
        queues
    })
}

fn ui3_keyboard_queues() -> &'static Vec<BrowserUi3KeyboardQueue> {
    TRUESURFER_UI3_KEYBOARD_QUEUES.call_once(|| {
        let mut queues = Vec::with_capacity(MAX_BROWSER_INSTANCE_ID as usize);
        for _ in 0..MAX_BROWSER_INSTANCE_ID {
            queues.push(BrowserUi3KeyboardQueue {
                queue: Mutex::new(VecDeque::with_capacity(TRUESURFER_UI3_KEYBOARD_QUEUE_DEPTH)),
            });
        }
        queues
    })
}

fn ui3_pointer_queue(browser_instance_id: u32) -> Option<&'static BrowserUi3PointerQueue> {
    if !browser_valid(browser_instance_id) {
        return None;
    }
    ui3_pointer_queues().get(browser_instance_id.saturating_sub(1) as usize)
}

fn ui3_keyboard_queue(browser_instance_id: u32) -> Option<&'static BrowserUi3KeyboardQueue> {
    if !browser_valid(browser_instance_id) {
        return None;
    }
    ui3_keyboard_queues().get(browser_instance_id.saturating_sub(1) as usize)
}

fn html_ready_signal(browser_instance_id: u32) -> Option<&'static Signal<SpinRawMutex, ()>> {
    if !browser_valid(browser_instance_id) {
        return None;
    }
    TRUESURFER_HTML_READY.get(browser_instance_id.saturating_sub(1) as usize)
}

#[inline]
fn browser_valid(browser_instance_id: u32) -> bool {
    (1..=MAX_BROWSER_INSTANCE_ID).contains(&browser_instance_id)
}

#[inline]
fn default_render_tex_id(browser_instance_id: u32) -> u32 {
    9_000u32.saturating_add(browser_instance_id.saturating_sub(1))
}

fn with_browser_state_mut<R>(
    browser_instance_id: u32,
    f: impl FnOnce(&mut BrowserInstanceState) -> R,
) -> Option<R> {
    if !browser_valid(browser_instance_id) {
        return None;
    }
    let mut guard = TRUESURFER_STATE.lock();
    let state = guard
        .entry(browser_instance_id)
        .or_insert_with(|| BrowserInstanceState {
            gadget_snapshot: HostedBrowserGadgetSnapshot::default(),
            render_tex_id: default_render_tex_id(browser_instance_id),
            surface_state: HostedBrowserSurfaceState {
                viewport_width: 512,
                viewport_height: 512,
                content_width: 512,
                content_height: 1,
                scroll_x: 0,
                scroll_y: 0,
            },
            ..BrowserInstanceState::default()
        });
    Some(f(state))
}

fn with_browser_state<R>(
    browser_instance_id: u32,
    f: impl FnOnce(&BrowserInstanceState) -> R,
) -> Option<R> {
    if !browser_valid(browser_instance_id) {
        return None;
    }
    let guard = TRUESURFER_STATE.lock();
    guard.get(&browser_instance_id).map(f)
}

#[inline]
fn signal_ui2_hosted_browser_dirty(browser_instance_id: u32, flags: u32) {
    if browser_valid(browser_instance_id) && flags != 0 {
        qjs::platform::ui::signal_hosted_browser_dirty(browser_instance_id, flags);
    }
}

#[inline]
fn log_line(line: String) {
    qjs::trueos_shims::log_info(line.as_str());
}

#[inline]
fn log_error(line: String) {
    qjs::trueos_shims::log_error(line.as_str());
}

pub fn default_browser_started() -> bool {
    with_browser_state(1, |state| state.started).unwrap_or(false)
}

pub fn latest_parse_result_for_browser(browser_instance_id: u32) -> Option<ParseResult> {
    with_browser_state(browser_instance_id, |state| state.last_parse_result.clone()).flatten()
}

pub async fn queue_set_html_with_url_for_browser(
    browser_instance_id: u32,
    html: String,
    url: Option<String>,
) -> bool {
    let Some(queue) = html_handoff_queue(browser_instance_id) else {
        return false;
    };
    let Some(ready_signal) = html_ready_signal(browser_instance_id) else {
        return false;
    };

    let html_len = html.len();
    let mut next_html = Some(html);
    let mut next_url = Some(url.unwrap_or_default());

    loop {
        {
            let mut sender = queue.sender.lock();
            if let Some(slot) = sender.try_send() {
                slot.html = next_html.take().unwrap_or_default();
                slot.url = next_url.take().unwrap_or_default();
                sender.send_done();
                ready_signal.signal(());
                log_line(format!(
                    "qjs-truesurfer[{}]: queued html bytes={} depth={} signal=1\n",
                    browser_instance_id,
                    html_len,
                    sender.len()
                ));
                return true;
            }
        }

        Timer::after(EmbassyDuration::from_millis(TRUESURFER_HTML_QUEUE_WAIT_MS)).await;
    }
}

pub fn queue_ui3_pointer_event_for_browser(
    browser_instance_id: u32,
    target_node: u32,
    kind: &str,
    x: i32,
    y: i32,
    pointer_id: u32,
    buttons: u32,
) -> bool {
    if target_node == 0 || !browser_valid(browser_instance_id) {
        return false;
    }
    let Some(queue) = ui3_pointer_queue(browser_instance_id) else {
        return false;
    };
    let Some(ready_signal) = html_ready_signal(browser_instance_id) else {
        return false;
    };

    let depth = {
        let mut guard = queue.queue.lock();
        while guard.len() >= TRUESURFER_UI3_POINTER_QUEUE_DEPTH {
            guard.pop_front();
        }
        guard.push_back(QueuedUi3PointerEvent {
            target_node,
            kind: String::from(kind),
            x,
            y,
            pointer_id,
            buttons,
        });
        guard.len()
    };
    ready_signal.signal(());

    let log_idx = TRUESURFER_UI3_POINTER_QUEUE_LOGS.fetch_add(1, Ordering::Relaxed);
    if kind != "pointermove" || buttons != 0 || log_idx < 96 {
        log_line(format!(
            "qjs-truesurfer[{}]: ui3 pointer queued target={} kind={} x={} y={} pointer={} buttons=0x{:X} depth={}\n",
            browser_instance_id, target_node, kind, x, y, pointer_id, buttons, depth
        ));
    }
    true
}

pub fn queue_ui3_keyboard_event_for_browser(
    browser_instance_id: u32,
    key: String,
    slot_id: u32,
    pointer_id: u32,
    modifiers: u32,
) -> bool {
    if key.is_empty() || !browser_valid(browser_instance_id) {
        return false;
    }
    let Some(queue) = ui3_keyboard_queue(browser_instance_id) else {
        return false;
    };
    let Some(ready_signal) = html_ready_signal(browser_instance_id) else {
        return false;
    };

    let depth = {
        let mut guard = queue.queue.lock();
        while guard.len() >= TRUESURFER_UI3_KEYBOARD_QUEUE_DEPTH {
            guard.pop_front();
        }
        guard.push_back(QueuedUi3KeyboardEvent {
            key: key.clone(),
            slot_id,
            pointer_id,
            modifiers,
        });
        guard.len()
    };
    ready_signal.signal(());

    let log_idx = TRUESURFER_UI3_KEYBOARD_QUEUE_LOGS.fetch_add(1, Ordering::Relaxed);
    if log_idx < 96 || key != "ArrowLeft" && key != "ArrowRight" {
        log_line(format!(
            "qjs-truesurfer[{}]: ui3 keyboard queued key={} slot={} pointer={} modifiers=0x{:X} depth={}\n",
            browser_instance_id, key, slot_id, pointer_id, modifiers, depth
        ));
    }
    true
}

pub fn queue_browser_rpc(_method: String, _args_json: String, _browser_window_id: u32) -> u32 {
    BROWSER_RPC_SEQ.fetch_add(1, Ordering::Relaxed)
}

pub fn take_browser_rpc_result(_id: u32) -> Option<String> {
    None
}

pub fn hosted_surface_seq_for_browser(browser_instance_id: u32) -> u32 {
    with_browser_state(browser_instance_id, |state| state.surface_seq).unwrap_or(0)
}

pub fn hosted_interactive_seq_for_browser(browser_instance_id: u32) -> u32 {
    with_browser_state(browser_instance_id, |state| state.interactive_seq).unwrap_or(0)
}

pub fn hosted_gadget_seq_for_browser(browser_instance_id: u32) -> u32 {
    with_browser_state(browser_instance_id, |state| state.gadget_seq).unwrap_or(0)
}

pub fn hosted_surface_state_for_browser(browser_instance_id: u32) -> HostedBrowserSurfaceState {
    with_browser_state(browser_instance_id, |state| state.surface_state).unwrap_or_default()
}

pub fn hosted_interactive_state_for_browser(
    _browser_instance_id: u32,
) -> HostedBrowserInteractiveState {
    HostedBrowserInteractiveState::default()
}

pub fn hosted_gadget_snapshot_for_browser(browser_instance_id: u32) -> HostedBrowserGadgetSnapshot {
    with_browser_state(browser_instance_id, |state| state.gadget_snapshot.clone())
        .unwrap_or_default()
}

pub fn set_hosted_viewport_for_browser(
    browser_instance_id: u32,
    viewport_width: u32,
    viewport_height: u32,
    _content_x: i32,
    _content_y: i32,
    content_width: u32,
    content_height: u32,
) -> bool {
    let mut dirty = false;
    let ok = with_browser_state_mut(browser_instance_id, |state| {
        let next = HostedBrowserSurfaceState {
            viewport_width: viewport_width.max(1),
            viewport_height: viewport_height.max(1),
            content_width: content_width.max(viewport_width.max(1)),
            content_height: content_height.max(1),
            scroll_x: state.surface_state.scroll_x,
            scroll_y: state.surface_state.scroll_y,
        };
        if state.surface_state == next {
            return true;
        }
        state.surface_state = next;
        state.surface_seq = state.surface_seq.wrapping_add(1);
        dirty = true;
        true
    })
    .unwrap_or(false);
    if dirty {
        signal_ui2_hosted_browser_dirty(browser_instance_id, UI2_HOSTED_BROWSER_DIRTY_CONTENT);
    }
    ok
}

pub fn set_hosted_scroll_for_browser(
    browser_instance_id: u32,
    scroll_x: u32,
    scroll_y: u32,
) -> bool {
    let mut dirty = false;
    let ok = with_browser_state_mut(browser_instance_id, |state| {
        if state.surface_state.scroll_x == scroll_x && state.surface_state.scroll_y == scroll_y {
            return true;
        }
        state.surface_state.scroll_x = scroll_x;
        state.surface_state.scroll_y = scroll_y;
        state.surface_seq = state.surface_seq.wrapping_add(1);
        dirty = true;
        true
    })
    .unwrap_or(false);
    if dirty {
        signal_ui2_hosted_browser_dirty(browser_instance_id, UI2_HOSTED_BROWSER_DIRTY_CONTENT);
    }
    ok
}

pub fn bind_browser_window_to_instance(browser_instance_id: u32, window_id: u32) -> bool {
    with_browser_state_mut(browser_instance_id, |state| {
        state.window_id = window_id;
        true
    })
    .unwrap_or(false)
}

pub fn browser_window_id_for_instance(browser_instance_id: u32) -> u32 {
    with_browser_state(browser_instance_id, |state| state.window_id).unwrap_or(0)
}

pub fn set_browser_render_target_tex_id_for_browser(browser_instance_id: u32, tex_id: u32) -> bool {
    with_browser_state_mut(browser_instance_id, |state| {
        state.render_tex_id = tex_id;
        true
    })
    .unwrap_or(false)
}

pub fn render_tex_id_for_browser_instance(browser_instance_id: u32) -> u32 {
    with_browser_state(browser_instance_id, |state| state.render_tex_id)
        .unwrap_or_else(|| default_render_tex_id(browser_instance_id))
}

pub fn queue_hosted_keyboard_events(
    browser_window_id: u32,
    events: &[HostedKeyboardEvent],
) -> bool {
    if events.is_empty() {
        return true;
    }
    let Some(browser_instance_id) = (1..=MAX_BROWSER_INSTANCE_ID)
        .find(|candidate| browser_window_id_for_instance(*candidate) == browser_window_id)
    else {
        return false;
    };
    let queued = with_browser_state_mut(browser_instance_id, |state| {
        state.interactive_seq = state.interactive_seq.wrapping_add(events.len() as u32);
        true
    })
    .unwrap_or(false);
    if queued {
        signal_ui2_hosted_browser_dirty(browser_instance_id, UI2_HOSTED_BROWSER_DIRTY_INTERACTIVE);
    }
    queued
}

unsafe fn set_global_i32(ctx: *mut qjs::JSContext, key: &[u8], value: i32) {
    let global = qjs::JS_GetGlobalObject(ctx);
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        global,
        key.as_ptr() as *const c_char,
        qjs::JS_NewFloat64(ctx, value as f64),
    );
    qjs::js_free_value(ctx, global);
}

unsafe fn set_global_string(ctx: *mut qjs::JSContext, key: &[u8], value: &str) {
    let global = qjs::JS_GetGlobalObject(ctx);
    let value_js = qjs::JS_NewStringLen(ctx, value.as_ptr() as *const c_char, value.len());
    let _ = qjs::JS_SetPropertyStr(ctx, global, key.as_ptr() as *const c_char, value_js);
    qjs::js_free_value(ctx, global);
}

unsafe fn read_global_bool(ctx: *mut qjs::JSContext, key: &[u8]) -> bool {
    let global = qjs::JS_GetGlobalObject(ctx);
    let value = qjs::JS_GetPropertyStr(ctx, global, key.as_ptr() as *const c_char);
    let mut out = 0.0f64;
    let ok = qjs::JS_ToFloat64(ctx, &mut out as *mut f64, value) == 0 && out.is_finite();
    qjs::js_free_value(ctx, value);
    qjs::js_free_value(ctx, global);
    ok && out != 0.0
}

unsafe fn read_global_string(ctx: *mut qjs::JSContext, key: &[u8]) -> String {
    let global = qjs::JS_GetGlobalObject(ctx);
    let value = qjs::JS_GetPropertyStr(ctx, global, key.as_ptr() as *const c_char);
    if value.is_exception() || value.tag == qjs::JS_TAG_UNDEFINED || value.tag == qjs::JS_TAG_NULL {
        qjs::js_free_value(ctx, value);
        qjs::js_free_value(ctx, global);
        return String::new();
    }
    let out = js_value_to_string(ctx, value);
    qjs::js_free_value(ctx, value);
    qjs::js_free_value(ctx, global);
    strip_trueos_host_markers(out.as_str())
}

unsafe fn truesurfer_ready(ctx: *mut qjs::JSContext) -> bool {
    let global = qjs::JS_GetGlobalObject(ctx);
    let ready =
        qjs::JS_GetPropertyStr(ctx, global, TRUESURFER_READY_PROP.as_ptr() as *const c_char);
    let mut ready_f = 0.0f64;
    let ready_flag = qjs::JS_ToFloat64(ctx, &mut ready_f as *mut f64, ready) == 0
        && ready_f.is_finite()
        && ready_f >= 1.0;

    let surfer = qjs::JS_GetPropertyStr(ctx, global, TRUESURFER_OBJ_PROP.as_ptr() as *const c_char);
    let set_html = if surfer.is_exception()
        || surfer.tag == qjs::JS_TAG_UNDEFINED
        || surfer.tag == qjs::JS_TAG_NULL
    {
        qjs::JSValue {
            u: qjs::JSValueUnion { int32: 0 },
            tag: qjs::JS_TAG_UNDEFINED,
        }
    } else {
        qjs::JS_GetPropertyStr(ctx, surfer, TRUESURFER_SET_HTML_PROP.as_ptr() as *const c_char)
    };
    let has_set_html = !set_html.is_exception()
        && set_html.tag != qjs::JS_TAG_UNDEFINED
        && set_html.tag != qjs::JS_TAG_NULL;

    qjs::js_free_value(ctx, set_html);
    qjs::js_free_value(ctx, surfer);
    qjs::js_free_value(ctx, ready);
    qjs::js_free_value(ctx, global);
    ready_flag || has_set_html
}

unsafe fn truesurfer_failed(ctx: *mut qjs::JSContext) -> bool {
    let global = qjs::JS_GetGlobalObject(ctx);
    let ready =
        qjs::JS_GetPropertyStr(ctx, global, TRUESURFER_READY_PROP.as_ptr() as *const c_char);
    let mut ready_f = 0.0f64;
    let failed = qjs::JS_ToFloat64(ctx, &mut ready_f as *mut f64, ready) == 0
        && ready_f.is_finite()
        && ready_f < 0.0;
    qjs::js_free_value(ctx, ready);
    qjs::js_free_value(ctx, global);
    failed
}

unsafe fn read_result_u32(ctx: *mut qjs::JSContext, obj: qjs::JSValueConst, key: &[u8]) -> u32 {
    let value = qjs::JS_GetPropertyStr(ctx, obj, key.as_ptr() as *const c_char);
    let mut out = 0.0f64;
    let ok =
        qjs::JS_ToFloat64(ctx, &mut out as *mut f64, value) == 0 && out.is_finite() && out >= 0.0;
    qjs::js_free_value(ctx, value);
    if ok { out as u32 } else { 0 }
}

unsafe fn read_result_f32(ctx: *mut qjs::JSContext, obj: qjs::JSValueConst, key: &[u8]) -> f32 {
    let value = qjs::JS_GetPropertyStr(ctx, obj, key.as_ptr() as *const c_char);
    let mut out = 0.0f64;
    let ok = qjs::JS_ToFloat64(ctx, &mut out as *mut f64, value) == 0 && out.is_finite();
    qjs::js_free_value(ctx, value);
    if ok { out as f32 } else { 0.0 }
}

unsafe fn read_result_string(
    ctx: *mut qjs::JSContext,
    obj: qjs::JSValueConst,
    key: &[u8],
) -> String {
    let value = qjs::JS_GetPropertyStr(ctx, obj, key.as_ptr() as *const c_char);
    if value.is_exception() || value.tag == qjs::JS_TAG_UNDEFINED || value.tag == qjs::JS_TAG_NULL {
        qjs::js_free_value(ctx, value);
        return String::new();
    }
    let out = js_value_to_string(ctx, value);
    qjs::js_free_value(ctx, value);
    strip_trueos_host_markers(out.as_str())
}

unsafe fn js_value_to_string(ctx: *mut qjs::JSContext, value: qjs::JSValueConst) -> String {
    let mut len = 0usize;
    let cstr = qjs::JS_ToCStringLen2(ctx, &mut len as *mut usize, value, 0);
    if cstr.is_null() {
        return String::new();
    }
    let bytes = core::slice::from_raw_parts(cstr as *const u8, len);
    let out = String::from_utf8_lossy(bytes).into_owned();
    qjs::JS_FreeCString(ctx, cstr);
    out
}

fn strip_trueos_host_markers(text: &str) -> String {
    let mut cleaned = strip_trueos_angle_markers(text);
    strip_trueos_bare_symbols(&mut cleaned);
    cleaned
}

fn strip_trueos_angle_markers(text: &str) -> String {
    const MARKER: &str = "<truesurfer-";
    const KNOWN_MARKERS: [&str; 13] = [
        "<truesurfer-parse5-trueos-host-core>",
        "<truesurfer-parse5-trueos-host-core",
        "<truesurfer-parse5-trueos-host-cor",
        "<truesurfer-parse5-trueos-host-event>",
        "<truesurfer-parse5-trueos-host-canvas>",
        "<truesurfer-parse5-trueos-host-dom>",
        "<truesurfer-parse5-trueos-host-fetch>",
        "<truesurfer-parse5-trueos-host-capture>",
        "<truesurfer-parse5-trueos-app.js>",
        "<truesurfer-parse5-trueos-app",
        "<truesurfer-init>",
        "<truesurfer-pixi-host-prelude>",
        "<truesurfer-pixi-capture-adapter>",
    ];

    let mut cleaned = String::from(text);
    if !cleaned.contains(MARKER) {
        return cleaned;
    }

    for marker in KNOWN_MARKERS {
        while let Some(idx) = cleaned.find(marker) {
            cleaned.replace_range(idx..idx + marker.len(), "");
        }
    }

    if !cleaned.contains(MARKER) {
        return cleaned;
    }

    let mut out = String::with_capacity(cleaned.len());
    let mut rest = cleaned.as_str();
    while let Some(idx) = rest.find(MARKER) {
        out.push_str(&rest[..idx]);
        let marker_tail = &rest[idx..];
        if let Some(end_rel) = marker_tail.find('>') {
            let marker_candidate = &marker_tail[..=end_rel];
            let marker_body = &marker_candidate[1..marker_candidate.len().saturating_sub(1)];
            if marker_body
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '.' || ch == '_')
            {
                rest = &marker_tail[end_rel + 1..];
                continue;
            }
        }
        out.push_str(MARKER);
        rest = &marker_tail[MARKER.len()..];
    }
    out.push_str(rest);
    out
}

fn strip_trueos_bare_symbols(text: &mut String) {
    const PREFIX: &str = "__trueos";
    const NUM_RESIDUE: &str = "Num";
    const NU_RESIDUE: &str = "Nu";
    const N_RESIDUE: &str = "N";
    strip_trueos_num_runs(text);
    loop {
        let before = text.len();
        while text.starts_with(PREFIX) {
            text.replace_range(0..PREFIX.len(), "");
        }
        while text.starts_with(NUM_RESIDUE) {
            text.replace_range(0..NUM_RESIDUE.len(), "");
        }
        while text.starts_with(NU_RESIDUE) {
            text.replace_range(0..NU_RESIDUE.len(), "");
        }
        while text == N_RESIDUE {
            text.clear();
        }
        if text.len() == before {
            break;
        }
    }
    if text.starts_with("__trueo") {
        text.clear();
    }
}

fn strip_trueos_num_runs(text: &mut String) {
    const RUN_PREFIX: &str = "__trueosN";
    while let Some(idx) = text.find(RUN_PREFIX) {
        let mut end = idx + RUN_PREFIX.len();
        while end < text.len() {
            let b = text.as_bytes()[end];
            if b != b'u' && b != b'm' {
                break;
            }
            end += 1;
        }
        text.replace_range(idx..end, "");
    }
}

fn clean_parse5_overlay_text(text: &str) -> String {
    const KNOWN_SYMBOLS: [&str; 5] = [
        "__trueosNumberValue",
        "__trueosHostNum",
        "__trueosNum",
        "__trueosNu",
        "__trueosN",
    ];
    let mut out = strip_trueos_host_markers(text);
    while out.contains("N__trueos") {
        out = out.replace("N__trueos", "__trueos");
    }
    for symbol in KNOWN_SYMBOLS {
        while out.contains(symbol) {
            out = out.replace(symbol, "");
        }
    }
    while out.starts_with('N') && out.contains("__trueos") {
        out.remove(0);
    }
    let mut out = String::from(out.trim());
    let residue_len = out
        .bytes()
        .take_while(|b| *b == b'u' || *b == b'm')
        .count();
    let next = out.as_bytes().get(residue_len).copied();
    if residue_len >= 2
        && next.map_or(true, |b| {
            b == b'('
                || b == b'['
                || b == b'{'
                || b == b'"'
                || b == b'\''
                || b.is_ascii_uppercase()
                || b.is_ascii_digit()
        })
    {
        out.replace_range(0..residue_len, "");
    }
    out
}

fn ui3_pointer_event_from_code(code: u32) -> &'static str {
    match code {
        1 => "pointerdown",
        2 => "pointerup",
        3 => "pointermove",
        4 => "pointerover",
        5 => "pointerout",
        6 => "pointerupoutside",
        7 => "contextmenu",
        _ => "pointerdown",
    }
}

fn compact_log_text_sample(text: &str, max_chars: usize) -> String {
    let mut out = String::new();
    let mut previous_space = false;
    for ch in text.chars() {
        if out.chars().count() >= max_chars {
            break;
        }
        let mapped = match ch {
            '\r' | '\n' | '\t' => ' ',
            '"' | '\\' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        };
        if mapped == ' ' {
            if out.is_empty() || previous_space {
                continue;
            }
            previous_space = true;
            out.push(mapped);
            continue;
        }
        previous_space = false;
        out.push(mapped);
    }
    out
}

unsafe fn log_parse5_trueos_bridge_stats(ctx: *mut qjs::JSContext, browser_instance_id: u32) {
    let global = qjs::JS_GetGlobalObject(ctx);
    let stats = qjs::JS_GetPropertyStr(
        ctx,
        global,
        TRUESURFER_TRUEOS_PIXI_BRIDGE_STATS_PROP.as_ptr() as *const c_char,
    );
    qjs::js_free_value(ctx, global);
    if stats.is_exception() || stats.tag == qjs::JS_TAG_UNDEFINED || stats.tag == qjs::JS_TAG_NULL {
        qjs::js_free_value(ctx, stats);
        return;
    }

    let render_nodes = read_result_u32(ctx, stats, TRUESURFER_BRIDGE_RENDER_NODES_PROP);
    let render_blocks = read_result_u32(ctx, stats, TRUESURFER_BRIDGE_RENDER_BLOCKS_PROP);
    let render_text = read_result_u32(ctx, stats, TRUESURFER_BRIDGE_RENDER_TEXT_PROP);
    let layout_boxes = read_result_u32(ctx, stats, TRUESURFER_BRIDGE_LAYOUT_BOXES_PROP);
    let layout_blocks = read_result_u32(ctx, stats, TRUESURFER_BRIDGE_LAYOUT_BLOCKS_PROP);
    let layout_text = read_result_u32(ctx, stats, TRUESURFER_BRIDGE_LAYOUT_TEXT_PROP);
    let layout_max_depth = read_result_u32(ctx, stats, TRUESURFER_BRIDGE_LAYOUT_MAX_DEPTH_PROP);
    let measure_text_calls = read_result_u32(ctx, stats, TRUESURFER_BRIDGE_MEASURE_TEXT_CALLS_PROP);
    let pixi_commands = read_result_u32(ctx, stats, TRUESURFER_BRIDGE_PIXI_COMMANDS_PROP);
    let render_tags = read_result_string(ctx, stats, TRUESURFER_BRIDGE_RENDER_TAGS_PROP);
    let render_text_samples =
        read_result_string(ctx, stats, TRUESURFER_BRIDGE_RENDER_TEXT_SAMPLES_PROP);
    let layout_text_samples =
        read_result_string(ctx, stats, TRUESURFER_BRIDGE_LAYOUT_TEXT_SAMPLES_PROP);
    let pixi_ops = read_result_string(ctx, stats, TRUESURFER_BRIDGE_PIXI_OPS_PROP);
    let pixi_unsupported = read_result_string(ctx, stats, TRUESURFER_BRIDGE_PIXI_UNSUPPORTED_PROP);
    qjs::js_free_value(ctx, stats);

    log_line(format!(
        "qjs-truesurfer[{}]: parse5 trueos bridge-stats render_nodes={} render_blocks={} render_text={} layout_boxes={} layout_blocks={} layout_text={} layout_depth={} measure_text_calls={} pixi_commands={} unsupported={}\n",
        browser_instance_id,
        render_nodes,
        render_blocks,
        render_text,
        layout_boxes,
        layout_blocks,
        layout_text,
        layout_max_depth,
        measure_text_calls,
        pixi_commands,
        if pixi_unsupported.is_empty() {
            "none"
        } else {
            pixi_unsupported.as_str()
        }
    ));
    if !render_tags.is_empty() || !pixi_ops.is_empty() {
        log_line(format!(
            "qjs-truesurfer[{}]: parse5 trueos bridge-tags render_tags={} pixi_ops={}\n",
            browser_instance_id,
            if render_tags.is_empty() {
                "none"
            } else {
                render_tags.as_str()
            },
            if pixi_ops.is_empty() {
                "none"
            } else {
                pixi_ops.as_str()
            }
        ));
    }
    if !render_text_samples.is_empty() {
        log_line(format!(
            "qjs-truesurfer[{}]: parse5 trueos bridge-render-text samples={}\n",
            browser_instance_id, render_text_samples
        ));
    }
    if !layout_text_samples.is_empty() {
        log_line(format!(
            "qjs-truesurfer[{}]: parse5 trueos bridge-layout-text samples={}\n",
            browser_instance_id, layout_text_samples
        ));
    }
}

unsafe fn submit_parse5_layout_text_overlays(
    ctx: *mut qjs::JSContext,
    browser_instance_id: u32,
    root_id: u32,
) -> u32 {
    if root_id == 0 {
        return 0;
    }

    let global = qjs::JS_GetGlobalObject(ctx);
    let overlays = qjs::JS_GetPropertyStr(
        ctx,
        global,
        TRUESURFER_TRUEOS_PIXI_LAYOUT_TEXT_OVERLAYS_PROP.as_ptr() as *const c_char,
    );
    qjs::js_free_value(ctx, global);
    if overlays.is_exception()
        || overlays.tag == qjs::JS_TAG_UNDEFINED
        || overlays.tag == qjs::JS_TAG_NULL
    {
        qjs::js_free_value(ctx, overlays);
        return 0;
    }

    let source_len = read_array_len(ctx, overlays);
    let overlay_count = source_len.min(512);
    let mut submitted = 0u32;
    let mut skipped = 0u32;
    let mut offscreen = 0u32;
    let mut samples = String::new();
    for idx in 0..overlay_count {
        let item = qjs::JS_GetPropertyUint32(ctx, overlays, idx);
        if item.is_exception() || item.tag == qjs::JS_TAG_UNDEFINED || item.tag == qjs::JS_TAG_NULL
        {
            qjs::js_free_value(ctx, item);
            skipped = skipped.saturating_add(1);
            continue;
        }

        let text = clean_parse5_overlay_text(
            read_result_string(ctx, item, TRUESURFER_UI3_OP_TEXT_PROP).as_str(),
        );
        let sample = compact_log_text_sample(text.as_str(), 96);
        if sample.is_empty()
            || sample == "true"
            || sample == "false"
            || sample.starts_with("<truesurfer-")
            || sample.starts_with("__trueos")
        {
            qjs::js_free_value(ctx, item);
            skipped = skipped.saturating_add(1);
            continue;
        }

        let x = read_result_f32(ctx, item, TRUESURFER_LAYOUT_TEXT_X_PROP);
        let y = read_result_f32(ctx, item, TRUESURFER_LAYOUT_TEXT_Y_PROP);
        if !(0.0..TRUESURFER_PARSE5_VISIBLE_TEXT_WIDTH).contains(&x)
            || !(0.0..TRUESURFER_PARSE5_VISIBLE_TEXT_HEIGHT).contains(&y)
        {
            qjs::js_free_value(ctx, item);
            offscreen = offscreen.saturating_add(1);
            skipped = skipped.saturating_add(1);
            continue;
        }

        let node = 900_000u32.saturating_add(idx);
        let ok = qjs::platform::ui::ui3_scene_node(browser_instance_id, node, 2)
            && qjs::platform::ui::ui3_scene_add_child(browser_instance_id, root_id, node)
            && qjs::platform::ui::ui3_scene_position(browser_instance_id, node, x, y)
            && qjs::platform::ui::ui3_scene_text_fill(browser_instance_id, node, 0x111111, 1.0)
            && qjs::platform::ui::ui3_scene_text(browser_instance_id, node, text.as_str());
        if ok {
            if submitted < 12 {
                if !samples.is_empty() {
                    samples.push('|');
                }
                samples.push_str(
                    format!("#{}@{}:{},{}", submitted, node, x as i32, y as i32).as_str(),
                );
                samples.push_str("=\"");
                samples.push_str(sample.as_str());
                samples.push('"');
            }
            submitted = submitted.saturating_add(1);
        } else {
            skipped = skipped.saturating_add(1);
        }
        qjs::js_free_value(ctx, item);
    }
    qjs::js_free_value(ctx, overlays);

    log_line(format!(
        "qjs-truesurfer[{}]: parse5 trueos rust-layout-text source_len={} submitted={} skipped={} offscreen={} samples={}\n",
        browser_instance_id,
        source_len,
        submitted,
        skipped,
        offscreen,
        if samples.is_empty() {
            "none"
        } else {
            samples.as_str()
        }
    ));
    submitted
}

unsafe fn submit_ui3_scene(
    ctx: *mut qjs::JSContext,
    browser_instance_id: u32,
    obj: qjs::JSValueConst,
) -> (u32, u32) {
    let scene_submit_start_ms = now_ms();
    log_line(format!("qjs-truesurfer[{}]: ui3 scene read begin\n", browser_instance_id));
    let scene_value = qjs::JS_GetPropertyStr(
        ctx,
        obj,
        TRUESURFER_RESULT_UI3_SCENE_PROP.as_ptr() as *const c_char,
    );
    if scene_value.is_exception()
        || scene_value.tag == qjs::JS_TAG_UNDEFINED
        || scene_value.tag == qjs::JS_TAG_NULL
    {
        log_line(format!("qjs-truesurfer[{}]: ui3 scene missing\n", browser_instance_id));
        qjs::js_free_value(ctx, scene_value);
        return (0, 0);
    }

    let root_id = read_result_u32(ctx, scene_value, TRUESURFER_UI3_SCENE_ROOT_ID_PROP);
    if root_id == 0 {
        log_line(format!("qjs-truesurfer[{}]: ui3 scene root missing\n", browser_instance_id));
        qjs::js_free_value(ctx, scene_value);
        return (0, 0);
    }
    let command_source =
        read_result_string(ctx, scene_value, TRUESURFER_UI3_SCENE_COMMAND_SOURCE_PROP);
    let scene_layout_text_ops =
        read_result_u32(ctx, scene_value, TRUESURFER_UI3_SCENE_LAYOUT_TEXT_OPS_PROP);
    let skip_empty_text_ops = command_source == "parse5-trueos-pixi";
    if !qjs::platform::ui::ui3_scene_begin(browser_instance_id, root_id) {
        log_line(format!(
            "qjs-truesurfer[{}]: ui3 scene begin rejected root={}\n",
            browser_instance_id, root_id
        ));
        qjs::js_free_value(ctx, scene_value);
        return (0, root_id);
    }
    log_line(format!(
        "qjs-truesurfer[{}]: ui3 scene begin root={} source={}\n",
        browser_instance_id, root_id, command_source
    ));

    let ops_value = qjs::JS_GetPropertyStr(
        ctx,
        scene_value,
        TRUESURFER_UI3_SCENE_OPS_PROP.as_ptr() as *const c_char,
    );
    if ops_value.is_exception()
        || ops_value.tag == qjs::JS_TAG_UNDEFINED
        || ops_value.tag == qjs::JS_TAG_NULL
    {
        log_line(format!(
            "qjs-truesurfer[{}]: ui3 scene ops missing root={}\n",
            browser_instance_id, root_id
        ));
        qjs::js_free_value(ctx, ops_value);
        qjs::js_free_value(ctx, scene_value);
        return (0, root_id);
    }

    let op_count = read_array_len(ctx, ops_value).min(TRUESURFER_UI3_SCENE_OP_LIMIT);
    log_line(format!(
        "qjs-truesurfer[{}]: ui3 scene ops count={} root={}\n",
        browser_instance_id, op_count, root_id
    ));
    let op_submit_start_ms = now_ms();
    let mut submitted = 0u32;
    let mut op_code_counts = [0u32; 29];
    let mut unknown_op_count = 0u32;
    let mut text_sample_count = 0u32;
    let mut listen_sample_count = 0u32;
    let mut texture_sample_count = 0u32;
    let mut skipped_empty_text_count = 0u32;
    for idx in 0..op_count {
        let op_value = qjs::JS_GetPropertyUint32(ctx, ops_value, idx);
        if op_value.is_exception()
            || op_value.tag == qjs::JS_TAG_UNDEFINED
            || op_value.tag == qjs::JS_TAG_NULL
        {
            qjs::js_free_value(ctx, op_value);
            continue;
        }

        let code = read_result_u32(ctx, op_value, TRUESURFER_UI3_OP_CODE_PROP);
        let node = read_result_u32(ctx, op_value, TRUESURFER_UI3_OP_NODE_PROP);
        let a = read_result_f32(ctx, op_value, TRUESURFER_UI3_OP_A_PROP);
        let b = read_result_f32(ctx, op_value, TRUESURFER_UI3_OP_B_PROP);
        let c = read_result_f32(ctx, op_value, TRUESURFER_UI3_OP_C_PROP);
        let d = read_result_f32(ctx, op_value, TRUESURFER_UI3_OP_D_PROP);
        if (code as usize) < op_code_counts.len() {
            op_code_counts[code as usize] = op_code_counts[code as usize].saturating_add(1);
        } else {
            unknown_op_count = unknown_op_count.saturating_add(1);
        }

        if idx < 8 {
            log_line(format!(
                "qjs-truesurfer[{}]: ui3 scene op#{} code={} node={} a={} b={} c={} d={}\n",
                browser_instance_id, idx, code, node, a, b, c, d
            ));
        }

        let ok = match code {
            1 => qjs::platform::ui::ui3_scene_node(browser_instance_id, node, a.max(0.0) as u32),
            2 => {
                qjs::platform::ui::ui3_scene_add_child(browser_instance_id, node, a.max(0.0) as u32)
            }
            3 => qjs::platform::ui::ui3_scene_position(browser_instance_id, node, a, b),
            4 => qjs::platform::ui::ui3_scene_graphics_clear(browser_instance_id, node),
            5 => qjs::platform::ui::ui3_scene_graphics_rect(browser_instance_id, node, a, b, c, d),
            6 => qjs::platform::ui::ui3_scene_graphics_fill(
                browser_instance_id,
                node,
                a.max(0.0) as u32,
                b,
            ),
            7 => qjs::platform::ui::ui3_scene_graphics_stroke(
                browser_instance_id,
                node,
                a.max(0.0) as u32,
                b,
                c,
            ),
            8 => {
                let text = read_result_string(ctx, op_value, TRUESURFER_UI3_OP_TEXT_PROP);
                if text_sample_count < 12 {
                    log_line(format!(
                        "qjs-truesurfer[{}]: ui3 scene text-op#{} op_index={} node={} chars={} bytes={} sample=\"{}\"\n",
                        browser_instance_id,
                        text_sample_count,
                        idx,
                        node,
                        text.chars().count(),
                        text.len(),
                        compact_log_text_sample(text.as_str(), 96)
                    ));
                }
                text_sample_count = text_sample_count.saturating_add(1);
                if skip_empty_text_ops && text.is_empty() {
                    skipped_empty_text_count = skipped_empty_text_count.saturating_add(1);
                    true
                } else {
                    qjs::platform::ui::ui3_scene_text(browser_instance_id, node, text.as_str())
                }
            }
            9 => qjs::platform::ui::ui3_scene_text_fill(
                browser_instance_id,
                node,
                a.max(0.0) as u32,
                b,
            ),
            10 => qjs::platform::ui::ui3_scene_add_child_at(
                browser_instance_id,
                node,
                a.max(0.0) as u32,
                b.max(0.0) as u32,
            ),
            11 => qjs::platform::ui::ui3_scene_set_child_index(
                browser_instance_id,
                node,
                a.max(0.0) as u32,
                b.max(0.0) as u32,
            ),
            12 => qjs::platform::ui::ui3_scene_remove_child(
                browser_instance_id,
                node,
                a.max(0.0) as u32,
            ),
            13 => qjs::platform::ui::ui3_scene_remove_from_parent(browser_instance_id, node),
            14 => qjs::platform::ui::ui3_scene_remove_children(browser_instance_id, node),
            15 => qjs::platform::ui::ui3_scene_visible(browser_instance_id, node, a != 0.0),
            23 => qjs::platform::ui::ui3_scene_alpha(browser_instance_id, node, a),
            28 => qjs::platform::ui::ui3_scene_scale(browser_instance_id, node, a, b),
            27 => qjs::platform::ui::ui3_scene_mask(browser_instance_id, node, a.max(0.0) as u32),
            16 => {
                let event = ui3_pointer_event_from_code(a.max(0.0) as u32);
                if listen_sample_count < 16 {
                    log_line(format!(
                        "qjs-truesurfer[{}]: ui3 scene listen-op#{} op_index={} node={} event={} event_code={}\n",
                        browser_instance_id,
                        listen_sample_count,
                        idx,
                        node,
                        event,
                        a.max(0.0) as u32
                    ));
                }
                listen_sample_count = listen_sample_count.saturating_add(1);
                qjs::platform::ui::ui3_scene_listen(browser_instance_id, node, event)
            }
            17 => qjs::platform::ui::ui3_scene_remove_all_listeners(browser_instance_id, node),
            18 => qjs::platform::ui::ui3_scene_graphics_circle(browser_instance_id, node, a, b, c),
            26 => {
                qjs::platform::ui::ui3_scene_graphics_ellipse(browser_instance_id, node, a, b, c, d)
            }
            19 => qjs::platform::ui::ui3_scene_graphics_move_to(browser_instance_id, node, a, b),
            20 => qjs::platform::ui::ui3_scene_graphics_line_to(browser_instance_id, node, a, b),
            25 => qjs::platform::ui::ui3_scene_graphics_close_path(browser_instance_id, node),
            24 => {
                let radius = read_result_string(ctx, op_value, TRUESURFER_UI3_OP_TEXT_PROP)
                    .parse::<f32>()
                    .unwrap_or(0.0);
                qjs::platform::ui::ui3_scene_graphics_round_rect(
                    browser_instance_id,
                    node,
                    a,
                    b,
                    c,
                    d,
                    radius,
                )
            }
            22 => {
                let h = read_result_string(ctx, op_value, TRUESURFER_UI3_OP_TEXT_PROP)
                    .parse::<f32>()
                    .unwrap_or(d);
                if texture_sample_count < 8 {
                    log_line(format!(
                        "qjs-truesurfer[{}]: ui3 scene texture-op#{} op_index={} node={} tex={} x={} y={} w={} h={}\n",
                        browser_instance_id,
                        texture_sample_count,
                        idx,
                        node,
                        a.max(0.0) as u32,
                        b,
                        c,
                        d,
                        h
                    ));
                }
                texture_sample_count = texture_sample_count.saturating_add(1);
                qjs::platform::ui::ui3_scene_texture_rect(
                    browser_instance_id,
                    node,
                    a.max(0.0) as u32,
                    b,
                    c,
                    d,
                    h,
                )
            }
            _ => false,
        };
        if ok {
            submitted = submitted.saturating_add(1);
        } else if idx < 32 {
            log_line(format!(
                "qjs-truesurfer[{}]: ui3 scene op#{} rejected code={} node={} a={} b={} c={} d={}\n",
                browser_instance_id, idx, code, node, a, b, c, d
            ));
        }
        qjs::js_free_value(ctx, op_value);
    }
    let op_submit_ms = now_ms().saturating_sub(op_submit_start_ms);
    log_line(format!(
        "qjs-truesurfer[{}]: ui3 scene op-counts node={} addChild={} position={} clear={} rect={} fill={} stroke={} text={} textFill={} addChildAt={} setChildIndex={} removeChild={} removeFromParent={} removeChildren={} visible={} listen={} removeAllListeners={} circle={} moveTo={} lineTo={} texture={} alpha={} roundRect={} closePath={} ellipse={} mask={} scale={} unknown={}\n",
        browser_instance_id,
        op_code_counts[1],
        op_code_counts[2],
        op_code_counts[3],
        op_code_counts[4],
        op_code_counts[5],
        op_code_counts[6],
        op_code_counts[7],
        op_code_counts[8],
        op_code_counts[9],
        op_code_counts[10],
        op_code_counts[11],
        op_code_counts[12],
        op_code_counts[13],
        op_code_counts[14],
        op_code_counts[15],
        op_code_counts[16],
        op_code_counts[17],
        op_code_counts[18],
        op_code_counts[19],
        op_code_counts[20],
        op_code_counts[22],
        op_code_counts[23],
        op_code_counts[24],
        op_code_counts[25],
        op_code_counts[26],
        op_code_counts[27],
        op_code_counts[28],
        unknown_op_count
    ));
    if skipped_empty_text_count != 0 {
        log_line(format!(
            "qjs-truesurfer[{}]: ui3 scene skipped_empty_text={} source={}\n",
            browser_instance_id, skipped_empty_text_count, command_source
        ));
    }
    let mut layout_text_submitted = scene_layout_text_ops;
    if command_source.starts_with("parse5-trueos-pix") {
        layout_text_submitted = layout_text_submitted.saturating_add(
            submit_parse5_layout_text_overlays(ctx, browser_instance_id, root_id),
        );
    }

    log_line(format!(
        "qjs-truesurfer[{}]: ui3 scene render begin root={} submitted={}/{} layout_text={} op_submit_ms={}\n",
        browser_instance_id, root_id, submitted, op_count, layout_text_submitted, op_submit_ms
    ));
    let render_start_ms = now_ms();
    let _ = qjs::platform::ui::ui3_scene_render(browser_instance_id, root_id);
    let render_ms = now_ms().saturating_sub(render_start_ms);
    log_line(format!(
        "qjs-truesurfer[{}]: ui3 scene render returned root={} submitted={}/{} render_ms={} total_submit_ms={}\n",
        browser_instance_id,
        root_id,
        submitted,
        op_count,
        render_ms,
        now_ms().saturating_sub(scene_submit_start_ms)
    ));

    qjs::js_free_value(ctx, ops_value);
    qjs::js_free_value(ctx, scene_value);
    (submitted, root_id)
}

unsafe fn submit_qjs_demo_text_widget_scene(
    ctx: *mut qjs::JSContext,
    browser_instance_id: u32,
    html: &str,
) -> (u32, u32) {
    let global = qjs::JS_GetGlobalObject(ctx);
    let mut builder = qjs::JS_GetPropertyStr(
        ctx,
        global,
        TRUESURFER_BUILD_TEXT_WIDGET_SCENE_PROP.as_ptr() as *const c_char,
    );
    if builder.is_exception()
        || builder.tag == qjs::JS_TAG_UNDEFINED
        || builder.tag == qjs::JS_TAG_NULL
    {
        qjs::js_free_value(ctx, builder);
        builder = qjs::JS_GetPropertyStr(
            ctx,
            global,
            TRUESURFER_BUILD_DEMO_TEXT_WIDGET_SCENE_PROP.as_ptr() as *const c_char,
        );
    }
    if builder.is_exception()
        || builder.tag == qjs::JS_TAG_UNDEFINED
        || builder.tag == qjs::JS_TAG_NULL
    {
        qjs::js_free_value(ctx, builder);
        qjs::js_free_value(ctx, global);
        log_line(format!(
            "qjs-truesurfer[{}]: qjs text widget builder unavailable\n",
            browser_instance_id
        ));
        return (0, 0);
    }

    let arg = qjs::JS_NewStringLen(ctx, html.as_ptr() as *const c_char, html.len());
    let widget_wrapper = qjs::JS_Call(ctx, builder, global, 1, &arg as *const qjs::JSValue);
    qjs::js_free_value(ctx, arg);
    qjs::js_free_value(ctx, builder);
    qjs::js_free_value(ctx, global);

    if widget_wrapper.is_exception() {
        qjs::qjs_diag::dump_last_exception(ctx, "truesurfer qjs text widget scene build");
        qjs::js_free_value(ctx, widget_wrapper);
        return (0, 0);
    }

    let widget_meta = qjs::JS_GetPropertyStr(
        ctx,
        widget_wrapper,
        TRUESURFER_WIDGET_PROP.as_ptr() as *const c_char,
    );
    if !widget_meta.is_exception()
        && widget_meta.tag != qjs::JS_TAG_UNDEFINED
        && widget_meta.tag != qjs::JS_TAG_NULL
    {
        let renderer = read_result_string(ctx, widget_meta, TRUESURFER_WIDGET_RENDERER_PROP);
        let tags = read_result_string(ctx, widget_meta, TRUESURFER_WIDGET_TAGS_PROP);
        let tag_counts = read_result_string(ctx, widget_meta, TRUESURFER_WIDGET_TAG_COUNTS_PROP);
        let widget_count = read_result_u32(ctx, widget_meta, TRUESURFER_WIDGET_COUNT_PROP);
        let button_count = read_result_u32(ctx, widget_meta, TRUESURFER_WIDGET_BUTTON_COUNT_PROP);
        let iframe_count = read_result_u32(ctx, widget_meta, TRUESURFER_WIDGET_IFRAME_COUNT_PROP);
        let iframe_srcdoc_count =
            read_result_u32(ctx, widget_meta, TRUESURFER_WIDGET_IFRAME_SRCDOC_COUNT_PROP);
        let text_count = read_result_u32(ctx, widget_meta, TRUESURFER_WIDGET_TEXT_COUNT_PROP);
        let text_bytes = read_result_u32(ctx, widget_meta, TRUESURFER_WIDGET_TEXT_BYTES_PROP);
        log_line(format!(
            "qjs-truesurfer[{}]: qjs text widget meta renderer={} widget_count={} button_count={} iframe_count={} iframe_srcdoc_count={} text_count={} text_bytes={} tags={} tag_counts={}\n",
            browser_instance_id,
            renderer,
            widget_count,
            button_count,
            iframe_count,
            iframe_srcdoc_count,
            text_count,
            text_bytes,
            tags,
            tag_counts
        ));
    }
    qjs::js_free_value(ctx, widget_meta);

    let submit_start_ms = now_ms();
    let (ops, root) = submit_ui3_scene(ctx, browser_instance_id, widget_wrapper);
    let submit_ms = now_ms().saturating_sub(submit_start_ms);
    qjs::js_free_value(ctx, widget_wrapper);
    log_line(format!(
        "qjs-truesurfer[{}]: qjs text widget scene submitted ops={} root={} submit_ms={}\n",
        browser_instance_id, ops, root, submit_ms
    ));
    (ops, root)
}

unsafe fn submit_parse5_trueos_pixi_scene(
    rt: *mut qjs::JSRuntime,
    ctx: *mut qjs::JSContext,
    browser_instance_id: u32,
    html: &str,
) -> (u32, u32) {
    let total_start_ms = now_ms();
    let host_chunks: [(&[u8], &[u8], &str); 6] = [
        (
            TRUESURFER_PARSE5_VITE_HOST_CORE_SOURCE,
            TRUESURFER_PARSE5_VITE_HOST_CORE_FILENAME,
            "truesurfer parse5 trueos host core",
        ),
        (
            TRUESURFER_PARSE5_VITE_HOST_EVENT_SOURCE,
            TRUESURFER_PARSE5_VITE_HOST_EVENT_FILENAME,
            "truesurfer parse5 trueos host event",
        ),
        (
            TRUESURFER_PARSE5_VITE_HOST_CANVAS_SOURCE,
            TRUESURFER_PARSE5_VITE_HOST_CANVAS_FILENAME,
            "truesurfer parse5 trueos host canvas",
        ),
        (
            TRUESURFER_PARSE5_VITE_HOST_DOM_SOURCE,
            TRUESURFER_PARSE5_VITE_HOST_DOM_FILENAME,
            "truesurfer parse5 trueos host dom",
        ),
        (
            TRUESURFER_PARSE5_VITE_HOST_FETCH_SOURCE,
            TRUESURFER_PARSE5_VITE_HOST_FETCH_FILENAME,
            "truesurfer parse5 trueos host fetch",
        ),
        (
            TRUESURFER_PARSE5_VITE_HOST_CAPTURE_SOURCE,
            TRUESURFER_PARSE5_VITE_HOST_CAPTURE_FILENAME,
            "truesurfer parse5 trueos host capture",
        ),
    ];
    let host_start_ms = now_ms();
    for (source, filename, label) in host_chunks {
        let host = qjs::js_eval_bytes(
            ctx,
            source,
            filename.as_ptr() as *const c_char,
            qjs::JS_EVAL_TYPE_GLOBAL,
        );
        if host.is_exception() {
            qjs::qjs_diag::dump_last_exception(ctx, label);
            qjs::js_free_value(ctx, host);
            return (0, 0);
        }
        qjs::js_free_value(ctx, host);
    }
    let host_ms = now_ms().saturating_sub(host_start_ms);
    set_global_string(ctx, TRUESURFER_TRUEOS_INPUT_HTML_PROP, html);
    let input_html_after_set = read_global_string(ctx, TRUESURFER_TRUEOS_INPUT_HTML_PROP);
    log_line(format!(
        "qjs-truesurfer[{}]: parse5 trueos host ok chunks={} html_bytes={} host_ms={} input_global_bytes={} input_global_sample=\"{}\"\n",
        browser_instance_id,
        host_chunks.len(),
        html.len(),
        host_ms,
        input_html_after_set.len(),
        compact_log_text_sample(input_html_after_set.as_str(), 120)
    ));

    log_line(format!(
        "qjs-truesurfer[{}]: parse5 trueos app eval begin bytes={} hash=0x{:08x} bundled_parse5={}\n",
        browser_instance_id,
        TRUESURFER_PARSE5_TRUEOS_APP_SOURCE.len(),
        fnv1a32(TRUESURFER_PARSE5_TRUEOS_APP_SOURCE),
        contains_bytes(TRUESURFER_PARSE5_TRUEOS_APP_SOURCE, b"TAG_ID") as u8
    ));
    let app_eval_start_ms = now_ms();
    let app = qjs::js_eval_bytes(
        ctx,
        TRUESURFER_PARSE5_TRUEOS_APP_SOURCE,
        TRUESURFER_PARSE5_TRUEOS_APP_FILENAME.as_ptr() as *const c_char,
        qjs::JS_EVAL_TYPE_GLOBAL,
    );
    if app.is_exception() {
        qjs::qjs_diag::dump_last_exception(ctx, "truesurfer parse5 trueos app eval");
        qjs::js_free_value(ctx, app);
        return (0, 0);
    }
    qjs::js_free_value(ctx, app);
    let app_eval_ms = now_ms().saturating_sub(app_eval_start_ms);
    log_line(format!(
        "qjs-truesurfer[{}]: parse5 trueos app eval returned app_eval_ms={}\n",
        browser_instance_id, app_eval_ms
    ));

    let immediate_app_ready = read_global_bool(ctx, TRUESURFER_TRUEOS_PIXI_APP_READY_PROP);
    let immediate_app_error = read_global_string(ctx, TRUESURFER_TRUEOS_PIXI_APP_ERROR_PROP);
    if !immediate_app_ready && !immediate_app_error.is_empty() {
        let app_phase = read_global_string(ctx, TRUESURFER_TRUEOS_PIXI_APP_PHASE_PROP);
        let capture_error = read_global_string(ctx, TRUESURFER_TRUEOS_PIXI_CAPTURE_ERROR_PROP);
        let capture_step = read_global_string(ctx, TRUESURFER_TRUEOS_PIXI_CAPTURE_STEP_PROP);
        let layout_step = read_global_string(ctx, TRUESURFER_TRUEOS_PIXI_LAYOUT_STEP_PROP);
        log_line(format!(
            "qjs-truesurfer[{}]: parse5 trueos app not ready pump_iters=0 pump_stopped=1 phase={} layout_step={} capture_step={} capture_error={} error={}\n",
            browser_instance_id,
            app_phase,
            layout_step,
            capture_step,
            capture_error,
            immediate_app_error
        ));
        return (0, 0);
    }

    let mut pump_iters = 0u32;
    let mut pump_stopped = false;
    let pump_start_ms = now_ms();
    for _ in 0..4096 {
        if read_global_bool(ctx, TRUESURFER_TRUEOS_PIXI_APP_READY_PROP) {
            break;
        }
        if !qjs::vm::pump_runtime_once(rt, ctx, "truesurfer-parse5-trueos") {
            pump_stopped = true;
            break;
        }
        pump_iters = pump_iters.saturating_add(1);
        if !runtime_has_pending_work(rt, ctx)
            && !read_global_bool(ctx, TRUESURFER_TRUEOS_PIXI_APP_READY_PROP)
        {
            pump_stopped = true;
            break;
        }
    }
    let pump_ms = now_ms().saturating_sub(pump_start_ms);

    if !read_global_bool(ctx, TRUESURFER_TRUEOS_PIXI_APP_READY_PROP) {
        let app_error = read_global_string(ctx, TRUESURFER_TRUEOS_PIXI_APP_ERROR_PROP);
        let app_phase = read_global_string(ctx, TRUESURFER_TRUEOS_PIXI_APP_PHASE_PROP);
        let capture_error = read_global_string(ctx, TRUESURFER_TRUEOS_PIXI_CAPTURE_ERROR_PROP);
        let capture_step = read_global_string(ctx, TRUESURFER_TRUEOS_PIXI_CAPTURE_STEP_PROP);
        let layout_step = read_global_string(ctx, TRUESURFER_TRUEOS_PIXI_LAYOUT_STEP_PROP);
        log_line(format!(
            "qjs-truesurfer[{}]: parse5 trueos app not ready pump_iters={} pump_stopped={} pump_ms={} phase={} layout_step={} capture_step={} capture_error={} error={}\n",
            browser_instance_id,
            pump_iters,
            pump_stopped as u8,
            pump_ms,
            app_phase,
            layout_step,
            capture_step,
            capture_error,
            app_error
        ));
        return (0, 0);
    }
    log_line(format!(
        "qjs-truesurfer[{}]: parse5 trueos app ready pump_iters={} pump_ms={}\n",
        browser_instance_id, pump_iters, pump_ms
    ));

    let asset_wait_start_ms = now_ms();
    let mut asset_pump_iters = 0u32;
    let mut asset_pump_stopped = false;
    log_line(format!("qjs-truesurfer[{}]: parse5 trueos asset probe begin\n", browser_instance_id));
    let had_asset_pending = qjs::async_ops::has_pending(ctx);
    log_line(format!(
        "qjs-truesurfer[{}]: parse5 trueos asset probe returned pending={}\n",
        browser_instance_id, had_asset_pending as u8
    ));
    if had_asset_pending {
        for _ in 0..TRUESURFER_PARSE5_ASSET_PUMP_BUDGET {
            log_line(format!(
                "qjs-truesurfer[{}]: parse5 trueos asset pump iter={} begin\n",
                browser_instance_id, asset_pump_iters
            ));
            if !qjs::async_ops::has_pending(ctx) {
                break;
            }
            if now_ms().saturating_sub(asset_wait_start_ms) >= TRUESURFER_PARSE5_ASSET_WAIT_MS {
                asset_pump_stopped = true;
                break;
            }
            let made_asset_progress = qjs::async_ops::pump_images(ctx);
            qjs::trueos_shims::trueos_cabi_poll_once();
            asset_pump_iters = asset_pump_iters.saturating_add(1);
            if !qjs::async_ops::has_pending(ctx) {
                break;
            }
            if !made_asset_progress {
                qjs::trueos_shims::trueos_cabi_poll_once();
            }
        }
        asset_pump_stopped |= qjs::async_ops::has_pending(ctx);
        log_line(format!(
            "qjs-truesurfer[{}]: parse5 trueos asset wait pending_before={} pending_after={} runtime_pending={} pump_iters={} pump_stopped={} pump_ms={}\n",
            browser_instance_id,
            had_asset_pending as u8,
            qjs::async_ops::has_pending(ctx) as u8,
            runtime_has_pending_work(rt, ctx) as u8,
            asset_pump_iters,
            asset_pump_stopped as u8,
            now_ms().saturating_sub(asset_wait_start_ms)
        ));
    }
    let asset_jobs_before = qjs::JS_IsJobPending(rt) > 0;
    if asset_jobs_before {
        log_line(format!(
            "qjs-truesurfer[{}]: parse5 trueos asset jobs drain skipped before=1 async_pending={} reason=native-assets-ready\n",
            browser_instance_id,
            qjs::async_ops::has_pending(ctx) as u8
        ));
        let global = qjs::JS_GetGlobalObject(ctx);
        let repaint = qjs::JS_GetPropertyStr(
            ctx,
            global,
            TRUESURFER_TRUEOS_PIXI_REPAINT_NOW_PROP.as_ptr() as *const c_char,
        );
        if !repaint.is_exception()
            && repaint.tag != qjs::JS_TAG_UNDEFINED
            && repaint.tag != qjs::JS_TAG_NULL
        {
            let repaint_result = qjs::JS_Call(ctx, repaint, global, 0, core::ptr::null());
            let repaint_exception = repaint_result.is_exception();
            log_line(format!(
                "qjs-truesurfer[{}]: parse5 trueos asset repaint returned exception={} async_pending={}\n",
                browser_instance_id,
                repaint_exception as u8,
                qjs::async_ops::has_pending(ctx) as u8
            ));
            if repaint_exception {
                qjs::qjs_diag::dump_last_exception(ctx, "truesurfer trueos asset repaint");
            }
            qjs::js_free_value(ctx, repaint_result);
        }
        qjs::js_free_value(ctx, repaint);
        qjs::js_free_value(ctx, global);
    }
    log_line(format!(
        "qjs-truesurfer[{}]: parse5 trueos scene builder lookup begin\n",
        browser_instance_id
    ));

    let global = qjs::JS_GetGlobalObject(ctx);
    let build_scene = qjs::JS_GetPropertyStr(
        ctx,
        global,
        TRUESURFER_PARSE5_BUILD_SCENE_PROP.as_ptr() as *const c_char,
    );
    if build_scene.is_exception()
        || build_scene.tag == qjs::JS_TAG_UNDEFINED
        || build_scene.tag == qjs::JS_TAG_NULL
    {
        qjs::js_free_value(ctx, build_scene);
        qjs::js_free_value(ctx, global);
        log_line(format!(
            "qjs-truesurfer[{}]: parse5 trueos scene builder missing\n",
            browser_instance_id
        ));
        return (0, 0);
    }

    let scene_build_start_ms = now_ms();
    let wrapper = qjs::JS_Call(ctx, build_scene, global, 0, core::ptr::null());
    let scene_build_ms = now_ms().saturating_sub(scene_build_start_ms);
    qjs::js_free_value(ctx, build_scene);
    qjs::js_free_value(ctx, global);
    if wrapper.is_exception() {
        qjs::qjs_diag::dump_last_exception(ctx, "truesurfer parse5 trueos scene build");
        qjs::js_free_value(ctx, wrapper);
        return (0, 0);
    }
    log_line(format!(
        "qjs-truesurfer[{}]: parse5 trueos scene build returned scene_build_ms={}\n",
        browser_instance_id, scene_build_ms
    ));
    log_parse5_trueos_bridge_stats(ctx, browser_instance_id);

    let scene_submit_start_ms = now_ms();
    let (ops, root) = submit_ui3_scene(ctx, browser_instance_id, wrapper);
    let scene_submit_ms = now_ms().saturating_sub(scene_submit_start_ms);
    qjs::js_free_value(ctx, wrapper);
    log_line(format!(
        "qjs-truesurfer[{}]: parse5 trueos ui3 submit ops={} root={} scene_submit_ms={} total_ms={}\n",
        browser_instance_id,
        ops,
        root,
        scene_submit_ms,
        now_ms().saturating_sub(total_start_ms)
    ));
    (ops, root)
}

unsafe fn read_array_len(ctx: *mut qjs::JSContext, obj: qjs::JSValueConst) -> u32 {
    static LENGTH_PROP: &[u8] = b"length\0";
    read_result_u32(ctx, obj, LENGTH_PROP)
}

fn take_queued_html_for_browser(browser_instance_id: u32) -> Option<PendingHtml> {
    let queue = html_handoff_queue(browser_instance_id)?;
    let mut receiver = queue.receiver.lock();
    let slot = receiver.try_receive()?;
    let pending = PendingHtml {
        html: core::mem::take(&mut slot.html),
        url: core::mem::take(&mut slot.url),
    };
    receiver.receive_done();
    log_line(format!(
        "[surfer] pipeline DIFFBOX browser={} pull html bytes={} url={}\n",
        browser_instance_id,
        pending.html.len(),
        pending.url
    ));
    Some(pending)
}

async fn wait_for_queued_html(browser_instance_id: u32) {
    let Some(ready_signal) = html_ready_signal(browser_instance_id) else {
        Timer::after(EmbassyDuration::from_millis(8)).await;
        return;
    };
    let _ = with_timeout(EmbassyDuration::from_millis(8), ready_signal.wait()).await;
}

fn take_queued_ui3_pointer_event_for_browser(
    browser_instance_id: u32,
) -> Option<QueuedUi3PointerEvent> {
    let queue = ui3_pointer_queue(browser_instance_id)?;
    let mut guard = queue.queue.lock();
    guard.pop_front()
}

fn ui3_pointer_queue_len_for_browser(browser_instance_id: u32) -> usize {
    let Some(queue) = ui3_pointer_queue(browser_instance_id) else {
        return 0;
    };
    queue.queue.lock().len()
}

fn take_queued_ui3_keyboard_event_for_browser(
    browser_instance_id: u32,
) -> Option<QueuedUi3KeyboardEvent> {
    let queue = ui3_keyboard_queue(browser_instance_id)?;
    let mut guard = queue.queue.lock();
    guard.pop_front()
}

fn ui3_keyboard_queue_len_for_browser(browser_instance_id: u32) -> usize {
    let Some(queue) = ui3_keyboard_queue(browser_instance_id) else {
        return 0;
    };
    queue.queue.lock().len()
}

unsafe fn submit_parse5_trueos_pixi_scene_from_capture(
    ctx: *mut qjs::JSContext,
    browser_instance_id: u32,
    reason: &str,
) -> (u32, u32) {
    let global = qjs::JS_GetGlobalObject(ctx);
    let build_scene = qjs::JS_GetPropertyStr(
        ctx,
        global,
        TRUESURFER_PARSE5_BUILD_SCENE_PROP.as_ptr() as *const c_char,
    );
    if build_scene.is_exception()
        || build_scene.tag == qjs::JS_TAG_UNDEFINED
        || build_scene.tag == qjs::JS_TAG_NULL
    {
        qjs::js_free_value(ctx, build_scene);
        qjs::js_free_value(ctx, global);
        log_line(format!(
            "qjs-truesurfer[{}]: parse5 trueos scene builder missing reason={}\n",
            browser_instance_id, reason
        ));
        return (0, 0);
    }

    let scene_build_start_ms = now_ms();
    let wrapper = qjs::JS_Call(ctx, build_scene, global, 0, core::ptr::null());
    let scene_build_ms = now_ms().saturating_sub(scene_build_start_ms);
    qjs::js_free_value(ctx, build_scene);
    qjs::js_free_value(ctx, global);
    if wrapper.is_exception() {
        qjs::qjs_diag::dump_last_exception(ctx, "truesurfer parse5 trueos scene build");
        qjs::js_free_value(ctx, wrapper);
        return (0, 0);
    }
    log_line(format!(
        "qjs-truesurfer[{}]: parse5 trueos scene build returned reason={} scene_build_ms={}\n",
        browser_instance_id, reason, scene_build_ms
    ));
    log_parse5_trueos_bridge_stats(ctx, browser_instance_id);

    let scene_submit_start_ms = now_ms();
    let (ops, root) = submit_ui3_scene(ctx, browser_instance_id, wrapper);
    let scene_submit_ms = now_ms().saturating_sub(scene_submit_start_ms);
    qjs::js_free_value(ctx, wrapper);
    log_line(format!(
        "qjs-truesurfer[{}]: parse5 trueos ui3 submit reason={} ops={} root={} scene_submit_ms={}\n",
        browser_instance_id, reason, ops, root, scene_submit_ms
    ));
    (ops, root)
}

unsafe fn dispatch_queued_ui3_pointer_events(
    _rt: *mut qjs::JSRuntime,
    ctx: *mut qjs::JSContext,
    browser_instance_id: u32,
) -> bool {
    let mut dispatched = 0u32;
    let mut needs_rebuild = false;
    let mut rebuilt = false;

    while dispatched < 32 {
        let Some(event) = take_queued_ui3_pointer_event_for_browser(browser_instance_id) else {
            break;
        };
        let global = qjs::JS_GetGlobalObject(ctx);
        let dispatch = qjs::JS_GetPropertyStr(
            ctx,
            global,
            TRUESURFER_TRUEOS_PIXI_POINTER_DISPATCH_PROP.as_ptr() as *const c_char,
        );
        if dispatch.is_exception()
            || dispatch.tag == qjs::JS_TAG_UNDEFINED
            || dispatch.tag == qjs::JS_TAG_NULL
        {
            qjs::js_free_value(ctx, dispatch);
            qjs::js_free_value(ctx, global);
            log_line(format!(
                "qjs-truesurfer[{}]: ui3 pointer dispatch missing target={} kind={}\n",
                browser_instance_id, event.target_node, event.kind
            ));
            continue;
        }

        let kind_js =
            qjs::JS_NewStringLen(ctx, event.kind.as_ptr() as *const c_char, event.kind.len());
        let args = [
            qjs::JS_NewFloat64(ctx, event.target_node as f64),
            kind_js,
            qjs::JS_NewFloat64(ctx, event.x as f64),
            qjs::JS_NewFloat64(ctx, event.y as f64),
            qjs::JS_NewFloat64(ctx, event.pointer_id as f64),
            qjs::JS_NewFloat64(ctx, event.buttons as f64),
        ];
        if event.kind != "pointermove" || event.buttons != 0 || dispatched < 8 {
            log_line(format!(
                "qjs-truesurfer[{}]: ui3 pointer dispatch call target={} kind={} x={} y={} pointer={} buttons=0x{:X}\n",
                browser_instance_id,
                event.target_node,
                event.kind,
                event.x,
                event.y,
                event.pointer_id,
                event.buttons
            ));
        }
        let result = qjs::JS_Call(ctx, dispatch, global, args.len() as i32, args.as_ptr());
        if event.kind != "pointermove" || event.buttons != 0 || dispatched < 8 {
            log_line(format!(
                "qjs-truesurfer[{}]: ui3 pointer dispatch returned target={} kind={} exception={}\n",
                browser_instance_id,
                event.target_node,
                event.kind,
                result.is_exception() as u8
            ));
        }
        qjs::js_free_value(ctx, args[1]);
        qjs::js_free_value(ctx, dispatch);
        qjs::js_free_value(ctx, global);

        if result.is_exception() {
            qjs::qjs_diag::dump_last_exception(ctx, "truesurfer trueos pixi pointer dispatch");
            qjs::js_free_value(ctx, result);
            continue;
        }

        let handled = read_result_u32(ctx, result, b"handled\0");
        let listener_count = read_result_u32(ctx, result, b"listenerCount\0");
        let painted = read_result_u32(ctx, result, b"painted\0");
        let target_found = read_result_u32(ctx, result, b"targetFound\0");
        qjs::js_free_value(ctx, result);
        dispatched = dispatched.saturating_add(1);

        if event.kind != "pointermove" || event.buttons != 0 || dispatched <= 8 {
            log_line(format!(
                "qjs-truesurfer[{}]: ui3 pointer dispatched target={} kind={} x={} y={} pointer={} buttons=0x{:X} target_found={} handled={} listeners={} painted={}\n",
                browser_instance_id,
                event.target_node,
                event.kind,
                event.x,
                event.y,
                event.pointer_id,
                event.buttons,
                target_found,
                handled,
                listener_count,
                painted
            ));
        }

        needs_rebuild |= target_found != 0 && painted != 0;
    }

    if needs_rebuild {
        let (ops, root) =
            submit_parse5_trueos_pixi_scene_from_capture(ctx, browser_instance_id, "ui3-pointer");
        rebuilt = ops != 0 && root != 0;
    }

    dispatched != 0 || rebuilt
}

unsafe fn dispatch_queued_ui3_keyboard_events(
    _rt: *mut qjs::JSRuntime,
    ctx: *mut qjs::JSContext,
    browser_instance_id: u32,
) -> bool {
    let mut dispatched = 0u32;
    let mut needs_rebuild = false;
    let mut rebuilt = false;

    while dispatched < 32 {
        let Some(event) = take_queued_ui3_keyboard_event_for_browser(browser_instance_id) else {
            break;
        };
        let global = qjs::JS_GetGlobalObject(ctx);
        let dispatch = qjs::JS_GetPropertyStr(
            ctx,
            global,
            TRUESURFER_TRUEOS_PIXI_KEYDOWN_DISPATCH_PROP.as_ptr() as *const c_char,
        );
        if dispatch.is_exception()
            || dispatch.tag == qjs::JS_TAG_UNDEFINED
            || dispatch.tag == qjs::JS_TAG_NULL
        {
            qjs::js_free_value(ctx, dispatch);
            qjs::js_free_value(ctx, global);
            log_line(format!(
                "qjs-truesurfer[{}]: ui3 keyboard dispatch missing key={} slot={} pointer={}\n",
                browser_instance_id, event.key, event.slot_id, event.pointer_id
            ));
            continue;
        }

        let key_js =
            qjs::JS_NewStringLen(ctx, event.key.as_ptr() as *const c_char, event.key.len());
        let args = [
            key_js,
            qjs::JS_NewFloat64(ctx, event.pointer_id as f64),
            qjs::JS_NewFloat64(ctx, event.modifiers as f64),
            qjs::JS_NewFloat64(ctx, event.slot_id as f64),
        ];
        log_line(format!(
            "qjs-truesurfer[{}]: ui3 keyboard dispatch call key={} slot={} pointer={} modifiers=0x{:X}\n",
            browser_instance_id, event.key, event.slot_id, event.pointer_id, event.modifiers
        ));
        let result = qjs::JS_Call(ctx, dispatch, global, args.len() as i32, args.as_ptr());
        log_line(format!(
            "qjs-truesurfer[{}]: ui3 keyboard dispatch returned key={} exception={}\n",
            browser_instance_id,
            event.key,
            result.is_exception() as u8
        ));
        qjs::js_free_value(ctx, args[0]);
        qjs::js_free_value(ctx, dispatch);
        qjs::js_free_value(ctx, global);

        if result.is_exception() {
            qjs::qjs_diag::dump_last_exception(ctx, "truesurfer trueos pixi keyboard dispatch");
            qjs::js_free_value(ctx, result);
            continue;
        }

        let handled = read_result_u32(ctx, result, b"handled\0");
        let listener_count = read_result_u32(ctx, result, b"listenerCount\0");
        let painted = read_result_u32(ctx, result, b"painted\0");
        let default_prevented = read_result_u32(ctx, result, b"defaultPrevented\0");
        qjs::js_free_value(ctx, result);
        dispatched = dispatched.saturating_add(1);

        log_line(format!(
            "qjs-truesurfer[{}]: ui3 keyboard dispatched key={} slot={} pointer={} handled={} listeners={} painted={} default_prevented={}\n",
            browser_instance_id,
            event.key,
            event.slot_id,
            event.pointer_id,
            handled,
            listener_count,
            painted,
            default_prevented
        ));

        needs_rebuild |= handled != 0 || painted != 0 || default_prevented != 0;
    }

    if needs_rebuild {
        let (ops, root) =
            submit_parse5_trueos_pixi_scene_from_capture(ctx, browser_instance_id, "ui3-keyboard");
        rebuilt = ops != 0 && root != 0;
    }

    dispatched != 0 || rebuilt
}

unsafe fn dispatch_html(
    rt: *mut qjs::JSRuntime,
    ctx: *mut qjs::JSContext,
    browser_instance_id: u32,
    pending: PendingHtml,
) -> bool {
    let global = qjs::JS_GetGlobalObject(ctx);
    let surfer = qjs::JS_GetPropertyStr(ctx, global, TRUESURFER_OBJ_PROP.as_ptr() as *const c_char);
    let set_html =
        qjs::JS_GetPropertyStr(ctx, surfer, TRUESURFER_SET_HTML_PROP.as_ptr() as *const c_char);
    let html_js =
        qjs::JS_NewStringLen(ctx, pending.html.as_ptr() as *const c_char, pending.html.len());
    let meta = qjs::JS_NewObject(ctx);
    let url_js =
        qjs::JS_NewStringLen(ctx, pending.url.as_ptr() as *const c_char, pending.url.len());
    let _ = qjs::JS_SetPropertyStr(
        ctx,
        meta,
        TRUESURFER_META_URL_PROP.as_ptr() as *const c_char,
        url_js,
    );
    let args = [html_js, meta];
    log_line(format!(
        "qjs-truesurfer[{}]: setHtml call bytes={} url={}\n",
        browser_instance_id,
        pending.html.len(),
        pending.url
    ));
    let result = qjs::JS_Call(ctx, set_html, surfer, 2, args.as_ptr());
    log_line(format!(
        "qjs-truesurfer[{}]: setHtml returned exception={}\n",
        browser_instance_id,
        if result.is_exception() { 1 } else { 0 }
    ));

    if result.is_exception() {
        qjs::qjs_diag::dump_last_exception(ctx, "truesurfer setHtml");
        let parse_result = ParseResult {
            ok: false,
            url: pending.url.clone(),
            bytes: pending.html.len() as u32,
            lines: pending.html.lines().count() as u32,
            error: String::from("truesurfer setHtml exception"),
            ..ParseResult::default()
        };
        let _ = with_browser_state_mut(browser_instance_id, |state| {
            state.last_parse_result = Some(parse_result.clone());
        });
        qjs::js_free_value(ctx, result);
        qjs::js_free_value(ctx, set_html);
        qjs::js_free_value(ctx, surfer);
        qjs::js_free_value(ctx, global);
        qjs::js_free_value(ctx, args[0]);
        qjs::js_free_value(ctx, args[1]);
        return false;
    }

    log_line(format!("qjs-truesurfer[{}]: result read begin\n", browser_instance_id));
    let parse_result = ParseResult {
        ok: read_result_u32(ctx, result, TRUESURFER_RESULT_OK_PROP) >= 1,
        url: pending.url.clone(),
        bytes: read_result_u32(ctx, result, TRUESURFER_RESULT_BYTES_PROP),
        lines: read_result_u32(ctx, result, TRUESURFER_RESULT_LINES_PROP),
        parse_ms: read_result_u32(ctx, result, TRUESURFER_RESULT_PARSE_MS_PROP),
        title: read_result_string(ctx, result, TRUESURFER_RESULT_TITLE_PROP),
        favicon_url: read_result_string(ctx, result, TRUESURFER_RESULT_FAVICON_URL_PROP),
        shell_bytes: read_result_u32(ctx, result, TRUESURFER_RESULT_SHELL_BYTES_PROP),
        body_bytes: read_result_u32(ctx, result, TRUESURFER_RESULT_BODY_BYTES_PROP),
        style_count: read_result_u32(ctx, result, TRUESURFER_RESULT_STYLE_COUNT_PROP),
        style_bytes: read_result_u32(ctx, result, TRUESURFER_RESULT_STYLE_BYTES_PROP),
        script_count: read_result_u32(ctx, result, TRUESURFER_RESULT_SCRIPT_COUNT_PROP),
        script_bytes: read_result_u32(ctx, result, TRUESURFER_RESULT_SCRIPT_BYTES_PROP),
        error: read_result_string(ctx, result, TRUESURFER_RESULT_ERROR_PROP),
    };
    log_line(format!(
        "qjs-truesurfer[{}]: result read done ok={} bytes={} body_bytes={} styles={} scripts={}\n",
        browser_instance_id,
        if parse_result.ok { 1 } else { 0 },
        parse_result.bytes,
        parse_result.body_bytes,
        parse_result.style_count,
        parse_result.script_count
    ));
    log_line(format!(
        "qjs-truesurfer[{}]: gadget snapshot skipped reason=parse5-trueos-pixi-path\n",
        browser_instance_id
    ));
    log_line(format!("qjs-truesurfer[{}]: ui3 submit begin\n", browser_instance_id));
    let (mut ui3_ops, mut ui3_root) =
        submit_parse5_trueos_pixi_scene(rt, ctx, browser_instance_id, pending.html.as_str());
    if ui3_ops == 0 || ui3_root == 0 {
        log_line(format!(
            "qjs-truesurfer[{}]: parse5 trueos ui3 unavailable; trying h1/p text widget fallback\n",
            browser_instance_id
        ));
        let (fallback_ops, fallback_root) =
            submit_qjs_demo_text_widget_scene(ctx, browser_instance_id, pending.html.as_str());
        if fallback_ops > 0 && fallback_root != 0 {
            ui3_ops = fallback_ops;
            ui3_root = fallback_root;
            log_line(format!(
                "qjs-truesurfer[{}]: h1/p text widget fallback submitted ops={} root={}\n",
                browser_instance_id, ui3_ops, ui3_root
            ));
        } else {
            log_line(format!(
                "qjs-truesurfer[{}]: h1/p text widget fallback unavailable\n",
                browser_instance_id
            ));
        }
    }
    log_line(format!(
        "qjs-truesurfer[{}]: ui3 submit done ops={} root={}\n",
        browser_instance_id, ui3_ops, ui3_root
    ));

    let _ = with_browser_state_mut(browser_instance_id, |state| {
        let parse_changed = state
            .last_parse_result
            .as_ref()
            .map(|prev| prev != &parse_result)
            .unwrap_or(true);

        if parse_changed {
            state.last_parse_result = Some(parse_result.clone());
        }
    });

    if parse_result.ok {
        log_line(format!(
            "[TrueSurfer -> UI3] browser={} handover ui3_ops={} root={} gadgets={} url={}\n",
            browser_instance_id, ui3_ops, ui3_root, 0, parse_result.url,
        ));
        log_line(format!(
            "qjs-truesurfer[{}]: parsed bytes={} title={} ms={} shell_bytes={} body_bytes={} ui3_ops={} gadgets={} styles={} scripts={} url={}\n",
            browser_instance_id,
            parse_result.bytes,
            parse_result.title,
            parse_result.parse_ms,
            parse_result.shell_bytes,
            parse_result.body_bytes,
            ui3_ops,
            0,
            parse_result.style_count,
            parse_result.script_count,
            parse_result.url
        ));
    } else {
        log_error(format!(
            "qjs-truesurfer[{}]: parse failed url={} err={}\n",
            browser_instance_id, parse_result.url, parse_result.error
        ));
    }

    log_line(format!(
        "qjs-truesurfer[{}]: qjs value release deferred reason=post-ui3-handoff\n",
        browser_instance_id
    ));
    true
}

unsafe fn runtime_has_pending_work(rt: *mut qjs::JSRuntime, ctx: *mut qjs::JSContext) -> bool {
    qjs::JS_IsJobPending(rt) > 0
        || qjs::async_ops::has_pending(ctx)
        || qjs::timers::has_pending(ctx)
        || qjs::workers::has_pending_for_ctx(ctx)
}

unsafe fn runtime_has_schedulable_work(rt: *mut qjs::JSRuntime, ctx: *mut qjs::JSContext) -> bool {
    let _ = ctx;
    qjs::JS_IsJobPending(rt) > 0
}

#[embassy_executor::task(pool_size = TRUESURFER_TASK_POOL_SIZE)]
pub async fn truesurfer_task(browser_instance_id: u32) {
    if !browser_valid(browser_instance_id) {
        log_error(format!("qjs-truesurfer[{}]: invalid browser instance\n", browser_instance_id));
        return;
    }

    let _ = with_browser_state_mut(browser_instance_id, |state| {
        state.started = true;
        state.api_ready = false;
    });
    log_line(format!("qjs-truesurfer[{}]: starting parser host\n", browser_instance_id));

    unsafe {
        let Some(vm) = qjs::vm::QjsVm::new_node_with_profile(qjs::node::RuntimeProfile::Browser)
        else {
            log_error(format!("qjs-truesurfer[{}]: JS runtime init failed\n", browser_instance_id));
            let _ = with_browser_state_mut(browser_instance_id, |state| {
                state.started = false;
            });
            return;
        };
        let ctx = vm.ctx_ptr();
        let rt = vm.rt_ptr();

        set_global_i32(ctx, TRUESURFER_ID_PROP, browser_instance_id as i32);

        for (source, filename, label) in [
            (
                TRUESURFER_PIXI_HOST_PRELUDE_SOURCE,
                TRUESURFER_PIXI_HOST_PRELUDE_FILENAME,
                "truesurfer pixi host prelude",
            ),
            (
                TRUESURFER_PIXI_BUNDLE_SOURCE,
                TRUESURFER_PIXI_BUNDLE_FILENAME,
                "truesurfer pixi bundle",
            ),
            (
                TRUESURFER_PIXI_COLLECTOR_SOURCE,
                TRUESURFER_PIXI_COLLECTOR_FILENAME,
                "truesurfer pixi collector",
            ),
            (
                TRUESURFER_PIXI_CAPTURE_ADAPTER_SOURCE,
                TRUESURFER_PIXI_CAPTURE_ADAPTER_FILENAME,
                "truesurfer pixi capture adapter",
            ),
        ] {
            let value = qjs::js_eval_bytes(
                ctx,
                source,
                filename.as_ptr() as *const c_char,
                qjs::JS_EVAL_TYPE_GLOBAL,
            );
            if value.is_exception() {
                qjs::qjs_diag::dump_last_exception(ctx, label);
                qjs::js_free_value(ctx, value);
                let _ = with_browser_state_mut(browser_instance_id, |state| {
                    state.started = false;
                });
                return;
            }
            qjs::js_free_value(ctx, value);
        }
        log_line(format!(
            "qjs-truesurfer[{}]: pixi hotpath loaded bundle_bytes={} adapter_bytes={}\n",
            browser_instance_id,
            TRUESURFER_PIXI_BUNDLE_SOURCE.len(),
            TRUESURFER_PIXI_CAPTURE_ADAPTER_SOURCE.len()
        ));

        let boot = qjs::js_eval_bytes(
            ctx,
            TRUESURFER_IMPORT_SOURCE,
            TRUESURFER_IMPORT_FILENAME.as_ptr() as *const c_char,
            qjs::JS_EVAL_TYPE_GLOBAL,
        );
        if boot.is_exception() {
            qjs::qjs_diag::dump_last_exception(ctx, "truesurfer init");
            qjs::js_free_value(ctx, boot);
            let _ = with_browser_state_mut(browser_instance_id, |state| {
                state.started = false;
            });
            return;
        }
        qjs::js_free_value(ctx, boot);

        let mut last_ready = false;

        loop {
            let mut busy = false;
            let mut runtime_alive = true;
            let queued_pointer_depth = ui3_pointer_queue_len_for_browser(browser_instance_id);
            let queued_keyboard_depth = ui3_keyboard_queue_len_for_browser(browser_instance_id);
            if queued_pointer_depth != 0 || queued_keyboard_depth != 0 {
                let log_idx = TRUESURFER_UI3_POINTER_LOOP_LOGS.fetch_add(1, Ordering::Relaxed);
                if log_idx < 64 || log_idx % 128 == 0 {
                    log_line(format!(
                        "qjs-truesurfer[{}]: ui3 input loop pointer_depth={} keyboard_depth={} ready={} js_jobs={} async_pending={} timers_pending={} workers_pending={}\n",
                        browser_instance_id,
                        queued_pointer_depth,
                        queued_keyboard_depth,
                        truesurfer_ready(ctx) as u8,
                        (qjs::JS_IsJobPending(rt) > 0) as u8,
                        qjs::async_ops::has_pending(ctx) as u8,
                        qjs::timers::has_pending(ctx) as u8,
                        qjs::workers::has_pending_for_ctx(ctx) as u8
                    ));
                }
            }

            if truesurfer_ready(ctx)
                && queued_pointer_depth == 0
                && queued_keyboard_depth == 0
            {
                if let Some(pending) = take_queued_html_for_browser(browser_instance_id) {
                    let _ = dispatch_html(rt, ctx, browser_instance_id, pending);
                    continue;
                }
                wait_for_queued_html(browser_instance_id).await;
                continue;
            }

            if dispatch_queued_ui3_pointer_events(rt, ctx, browser_instance_id) {
                busy = true;
            }
            if dispatch_queued_ui3_keyboard_events(rt, ctx, browser_instance_id) {
                busy = true;
            }

            if busy && truesurfer_ready(ctx) {
                Timer::after(EmbassyDuration::from_millis(TRUESURFER_BUSY_SLEEP_MS)).await;
                continue;
            }

            for _ in 0..TRUESURFER_BUSY_PUMP_BUDGET {
                if dispatch_queued_ui3_pointer_events(rt, ctx, browser_instance_id) {
                    busy = true;
                }
                if dispatch_queued_ui3_keyboard_events(rt, ctx, browser_instance_id) {
                    busy = true;
                }
                if truesurfer_ready(ctx) && !runtime_has_schedulable_work(rt, ctx) {
                    break;
                }
                if !qjs::vm::pump_runtime_once(rt, ctx, "truesurfer") {
                    runtime_alive = false;
                    break;
                }

                let ready = truesurfer_ready(ctx);
                let failed = truesurfer_failed(ctx);
                if ready != last_ready {
                    log_line(format!(
                        "qjs-truesurfer[{}]: ready={}\n",
                        browser_instance_id,
                        if ready { 1 } else { 0 }
                    ));
                    last_ready = ready;
                }
                if failed {
                    log_line(format!("qjs-truesurfer[{}]: startup failed\n", browser_instance_id));
                    runtime_alive = false;
                    break;
                }
                let _ = with_browser_state_mut(browser_instance_id, |state| {
                    state.api_ready = ready;
                });
                let mut dispatched_html = false;
                let mut dispatched_ui3_pointer = false;
                let mut dispatched_ui3_keyboard = false;
                if ready {
                    while let Some(pending) = take_queued_html_for_browser(browser_instance_id) {
                        let _ = dispatch_html(rt, ctx, browser_instance_id, pending);
                        dispatched_html = true;
                    }
                    if dispatched_html {
                        log_line(format!(
                            "qjs-truesurfer[{}]: html dispatch returned to input loop\n",
                            browser_instance_id
                        ));
                        busy = true;
                        break;
                    }
                    dispatched_ui3_pointer =
                        dispatch_queued_ui3_pointer_events(rt, ctx, browser_instance_id);
                    dispatched_ui3_keyboard =
                        dispatch_queued_ui3_keyboard_events(rt, ctx, browser_instance_id);
                }

                busy = !ready
                    || dispatched_html
                    || dispatched_ui3_pointer
                    || dispatched_ui3_keyboard
                    || runtime_has_schedulable_work(rt, ctx);
                if !busy {
                    break;
                }
            }

            if !runtime_alive {
                break;
            }

            if !busy && !runtime_has_schedulable_work(rt, ctx) {
                if truesurfer_ready(ctx) {
                    if let Some(pending) = take_queued_html_for_browser(browser_instance_id) {
                        let _ = dispatch_html(rt, ctx, browser_instance_id, pending);
                        continue;
                    }
                }
                if dispatch_queued_ui3_pointer_events(rt, ctx, browser_instance_id) {
                    continue;
                }
                if dispatch_queued_ui3_keyboard_events(rt, ctx, browser_instance_id) {
                    continue;
                }
                wait_for_queued_html(browser_instance_id).await;
                continue;
            }

            Timer::after(EmbassyDuration::from_millis(TRUESURFER_BUSY_SLEEP_MS)).await;
        }

        let _ = qjs::vm::teardown_main_context(rt, ctx, 500).await;
    }

    let _ = with_browser_state_mut(browser_instance_id, |state| {
        state.started = false;
        state.api_ready = false;
    });
    log_line(format!("qjs-truesurfer[{}]: parser host ended\n", browser_instance_id));
}
