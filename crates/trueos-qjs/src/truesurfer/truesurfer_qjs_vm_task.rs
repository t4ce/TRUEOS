#![cfg(feature = "trueos")]

extern crate alloc;

use alloc::boxed::Box;
use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::ffi::c_char;
use core::sync::atomic::{AtomicU32, Ordering};

use embassy_sync::blocking_mutex::raw::RawMutex;
use embassy_sync::signal::Signal;
use embassy_sync::zerocopy_channel::{Channel, Receiver, Sender};
use embassy_time::{Duration as EmbassyDuration, Timer};
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
    include_bytes!("../../../../src/ui3/pixi_host_prelude.js");
const TRUESURFER_PIXI_BUNDLE_SOURCE: &[u8] =
    include_bytes!("../../../../src/ui3/pixi_bundle.min.js");
const TRUESURFER_PIXI_CAPTURE_ADAPTER_SOURCE: &[u8] =
    include_bytes!("../../../../src/ui3/pixi_capture_adapter.js");
const TRUESURFER_PARSE5_TRUEOS_APP_SOURCE: &[u8] = b"";

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
      case "listen": return push({ code: 16, node: node, text: String(arguments[2] || "") });
      case "removeAllListeners": return push({ code: 17, node: node });
      case "clear": return push({ code: 4, node: node });
      case "rect": return push({ code: 5, node: node, a: num(arguments[2], 0), b: num(arguments[3], 0), c: num(arguments[4], 0), d: num(arguments[5], 0) });
      case "circle": return push({ code: 18, node: node, a: num(arguments[2], 0), b: num(arguments[3], 0), c: num(arguments[4], 0) });
      case "moveTo": return push({ code: 19, node: node, a: num(arguments[2], 0), b: num(arguments[3], 0) });
      case "lineTo": return push({ code: 20, node: node, a: num(arguments[2], 0), b: num(arguments[3], 0) });
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
  G.innerWidth = Math.max(1, num(G.innerWidth, 1920) | 0);
  G.innerHeight = Math.max(1, num(G.innerHeight, 1080) | 0);
  G.dispatchEvent = G.dispatchEvent || function () { return true; };
  G.addEventListener = G.addEventListener || function () {};
  G.removeEventListener = G.removeEventListener || function () {};
  G.requestAnimationFrame = G.requestAnimationFrame || function (fn) {
    if (typeof fn === "function") fn((G.performance && G.performance.now && G.performance.now()) || 0);
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
    if (typeof node.text === "string" && textHasInk(node.text)) ops.push({ code: 8, node: id, text: node.text });
    var children = Array.isArray(node.children) ? node.children : [];
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
    return value && typeof value.width === "number" ? num(value.width, 1) : 1;
  }
  function commandKind(target) {
    target = String(target || "");
    if (target.indexOf("Graphics") >= 0) return 1;
    if (target.indexOf("Text") >= 0) return 2;
    return 0;
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
        case "roundRect":
          ops.push({ code: 5, node: id, a: num(args[0], 0), b: num(args[1], 0), c: num(args[2], 0), d: num(args[3], 0) });
          break;
        case "circle": ops.push({ code: 18, node: id, a: num(args[0], 0), b: num(args[1], 0), c: num(args[2], 0) }); break;
        case "ellipse": ops.push({ code: 18, node: id, a: num(args[0], 0), b: num(args[1], 0), c: Math.max(num(args[2], 0), num(args[3], 0)) }); break;
        case "moveTo": ops.push({ code: 19, node: id, a: num(args[0], 0), b: num(args[1], 0) }); break;
      case "lineTo": ops.push({ code: 20, node: id, a: num(args[0], 0), b: num(args[1], 0) }); break;
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
        case "closePath": break;
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
        case "on": if (!hasSnapshot && cmd.event) ops.push({ code: 16, node: id, text: String(cmd.event) }); break;
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
G.innerWidth = Math.max(1, __trueosNum(G.innerWidth, 1920) | 0);
G.innerHeight = Math.max(1, __trueosNum(G.innerHeight, 1080) | 0);
G.addEventListener = G.addEventListener || function () {};
G.removeEventListener = G.removeEventListener || function () {};
G.dispatchEvent = G.dispatchEvent || function () { return true; };
G.performance = G.performance || { now: function () { return 0; } };
G.requestAnimationFrame = G.requestAnimationFrame || function (fn) {
  if (typeof fn === "function") fn(G.performance.now());
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
  return value && typeof value.width === "number" ? __trueosNum(value.width, 1) : 1;
}
function __trueosCommandKind(target) {
  target = String(target || "");
  if (target.indexOf("Graphics") >= 0) return 1;
  if (target.indexOf("Text") >= 0) return 2;
  return 0;
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
function __trueosLogTextSample(value) {
  var s = String(value == null ? "" : value);
  var out = "";
  for (var i = 0; i < s.length && out.length < 96; i += 1) {
    var ch = s.charAt(i);
    out += (ch === "\r" || ch === "\n" || ch === "\t" || ch === "|" || ch === "\"" || ch === "\\") ? "_" : ch;
  }
  return out;
}
function __trueosPushSnapshotNode(node, parent, ops, seen, snapshotSeen, textSlots) {
  if (!node || typeof node !== "object") return 0;
  var id = __trueosNum(node.id, 0) | 0;
  if (id <= 0) return 0;
  var kind = 0;
  if (!seen[id]) {
    var type = String(node.type || "");
    kind = type.indexOf("Graphics") >= 0 ? 1 : (type.indexOf("Text") >= 0 ? 2 : 0);
    ops.push({ code: 1, node: id, a: kind });
    seen[id] = true;
  }
  snapshotSeen[id] = true;
  if (kind === 2 && textSlots) textSlots.push(id);
  if (parent > 0) ops.push({ code: 2, node: parent, a: id });
  ops.push({ code: 3, node: id, a: __trueosNum(node.x, 0), b: __trueosNum(node.y, 0) });
  if (node.visible === false) ops.push({ code: 15, node: id, a: 0 });
  if (typeof node.text === "string" && __trueosTextHasInk(node.text)) ops.push({ code: 8, node: id, text: node.text });
  var children = node.children && node.children.length ? node.children : [];
  for (var i = 0; i < children.length; i += 1) __trueosPushSnapshotNode(children[i], id, ops, seen, snapshotSeen, textSlots);
  return id;
}
G.__trueosParse5BuildSceneFromCapture = function () {
  var cap = G.__pixiCapture;
  var commands = cap && cap.commands && cap.commands.length ? cap.commands : [];
  var ops = [];
  var seen = {};
  var snapshotSeen = {};
  var textSlots = [];
  var textMap = {};
  var textHasFill = {};
  var pendingTextFill = {};
  var rootId = 0;
  var snapshot = null;
  var i;
  for (i = commands.length - 1; i >= 0; i -= 1) {
    if (commands[i] && commands[i].op === "snapshot" && commands[i].args && commands[i].args[0]) {
      snapshot = commands[i].args[0];
      rootId = __trueosNum(commands[i].id, __trueosNum(snapshot.id, 0)) | 0;
      break;
    }
  }
  var hasSnapshot = !!snapshot;
  if (snapshot) rootId = __trueosPushSnapshotNode(snapshot, 0, ops, seen, snapshotSeen, textSlots) || rootId;
  function __trueosMappedTextNode(commandId, textValueHasInk, commandWasSnapshotSeen) {
    return commandId;
  }
  for (i = 0; i < commands.length; i += 1) {
    var cmd = commands[i] || {};
    var id = __trueosNum(cmd.id, 0) | 0;
    if (id <= 0) continue;
    var wasSnapshotSeen = !!snapshotSeen[id];
    if (!seen[id]) {
      ops.push({ code: 1, node: id, a: __trueosCommandKind(cmd.target) });
      seen[id] = true;
    }
    var args = cmd.args && cmd.args.length ? cmd.args : [];
    switch (cmd.op) {
      case "clear": ops.push({ code: 4, node: id }); break;
      case "rect":
      case "roundRect":
        ops.push({ code: 5, node: id, a: __trueosNum(args[0], 0), b: __trueosNum(args[1], 0), c: __trueosNum(args[2], 0), d: __trueosNum(args[3], 0) });
        break;
      case "circle": ops.push({ code: 18, node: id, a: __trueosNum(args[0], 0), b: __trueosNum(args[1], 0), c: __trueosNum(args[2], 0) }); break;
      case "ellipse": ops.push({ code: 18, node: id, a: __trueosNum(args[0], 0), b: __trueosNum(args[1], 0), c: Math.max(__trueosNum(args[2], 0), __trueosNum(args[3], 0)) }); break;
      case "moveTo": ops.push({ code: 19, node: id, a: __trueosNum(args[0], 0), b: __trueosNum(args[1], 0) }); break;
      case "lineTo": ops.push({ code: 20, node: id, a: __trueosNum(args[0], 0), b: __trueosNum(args[1], 0) }); break;
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
      case "closePath": break;
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
      case "removeChild":
        if (!hasSnapshot && __trueosNum(args[0], 0) > 0) ops.push({ code: 12, node: id, a: __trueosNum(args[0], 0) });
        break;
      case "removeChildren": if (!hasSnapshot) ops.push({ code: 14, node: id }); break;
      case "removeAllListeners": if (!hasSnapshot) ops.push({ code: 17, node: id }); break;
      case "on": if (!hasSnapshot && cmd.event) ops.push({ code: 16, node: id, text: String(cmd.event) }); break;
      case "text.text.set":
        var textValue = String(args[0] == null ? "" : args[0]);
        var hasInk = __trueosTextHasInk(textValue);
        if (hasSnapshot && !hasInk) break;
        var textNode = __trueosMappedTextNode(id, hasInk, wasSnapshotSeen);
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
          var fill = __trueosColorArg(args[0].fill, 0xffffff);
          var mapped = textMap[id] || (wasSnapshotSeen ? id : 0);
          if (mapped) {
            ops.push({ code: 9, node: mapped, a: fill, b: 1 });
            textHasFill[mapped] = true;
          } else pendingTextFill[id] = fill;
        }
        break;
    }
  }
  if (G.console && typeof G.console.log === "function") {
    var textLog = [];
    for (i = 0; i < ops.length && textLog.length < 12; i += 1) {
      if (ops[i] && ops[i].code === 8) {
        textLog.push("#" + textLog.length + "@node=" + ops[i].node + " chars=" + String(ops[i].text || "").length + " sample=\"" + __trueosLogTextSample(ops[i].text) + "\"");
      }
    }
    G.console.log("[parse5 trueos host] ui3-scene-text " + textLog.join("|"));
  }
  return { ok: 1, ui3Scene: { version: 1, commandSource: "parse5-trueos-pixi", rootId: rootId, opCount: ops.length, ops: ops } };
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
const TRUESURFER_PARSE5_BUILD_SCENE_PROP: &[u8] = b"__trueosParse5BuildSceneFromCapture\0";
const TRUESURFER_UI3_SCENE_COMMAND_SOURCE_PROP: &[u8] = b"commandSource\0";
const TRUESURFER_UI3_SCENE_ROOT_ID_PROP: &[u8] = b"rootId\0";
const TRUESURFER_UI3_SCENE_OPS_PROP: &[u8] = b"ops\0";
const TRUESURFER_UI3_OP_CODE_PROP: &[u8] = b"code\0";
const TRUESURFER_UI3_OP_NODE_PROP: &[u8] = b"node\0";
const TRUESURFER_UI3_OP_A_PROP: &[u8] = b"a\0";
const TRUESURFER_UI3_OP_B_PROP: &[u8] = b"b\0";
const TRUESURFER_UI3_OP_C_PROP: &[u8] = b"c\0";
const TRUESURFER_UI3_OP_D_PROP: &[u8] = b"d\0";
const TRUESURFER_UI3_OP_TEXT_PROP: &[u8] = b"text\0";
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
const TRUESURFER_PARSE5_ASSET_PUMP_BUDGET: usize = 8192;
const TRUESURFER_PARSE5_ASSET_WAIT_MS: u64 = 2500;
const TRUESURFER_UI3_SCENE_OP_LIMIT: u32 = 8192;
const HOSTED_BROWSER_DIRTY_CONTENT: u32 = 1 << 0;
const HOSTED_BROWSER_DIRTY_INTERACTIVE: u32 = 1 << 1;

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

#[derive(Clone, Default)]
struct HtmlHandoffSlot {
    html: String,
    url: String,
}

struct BrowserHtmlQueue {
    sender: Mutex<Sender<'static, SpinRawMutex, HtmlHandoffSlot>>,
    receiver: Mutex<Receiver<'static, SpinRawMutex, HtmlHandoffSlot>>,
}

#[derive(Default)]
struct BrowserInstanceState {
    started: bool,
    api_ready: bool,
    last_parse_result: Option<ParseResult>,
    window_id: u32,
    render_tex_id: u32,
    surface_seq: u32,
    interactive_seq: u32,
    surface_state: HostedBrowserSurfaceState,
}

static TRUESURFER_STATE: Mutex<BTreeMap<u32, BrowserInstanceState>> = Mutex::new(BTreeMap::new());
static BROWSER_RPC_SEQ: AtomicU32 = AtomicU32::new(1);
static TRUESURFER_HTML_QUEUES: Once<Vec<BrowserHtmlQueue>> = Once::new();
static TRUESURFER_HTML_READY: [Signal<SpinRawMutex, ()>; MAX_BROWSER_INSTANCE_ID as usize] =
    [const { Signal::new() }; MAX_BROWSER_INSTANCE_ID as usize];

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
fn signal_hosted_browser_dirty(browser_instance_id: u32, flags: u32) {
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

pub fn hosted_surface_state_for_browser(browser_instance_id: u32) -> HostedBrowserSurfaceState {
    with_browser_state(browser_instance_id, |state| state.surface_state).unwrap_or_default()
}

pub fn hosted_interactive_state_for_browser(
    _browser_instance_id: u32,
) -> HostedBrowserInteractiveState {
    HostedBrowserInteractiveState::default()
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
        signal_hosted_browser_dirty(browser_instance_id, HOSTED_BROWSER_DIRTY_CONTENT);
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
        signal_hosted_browser_dirty(browser_instance_id, HOSTED_BROWSER_DIRTY_CONTENT);
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
        signal_hosted_browser_dirty(browser_instance_id, HOSTED_BROWSER_DIRTY_INTERACTIVE);
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
    strip_trueos_bare_symbols(&mut cleaned);
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
    const SYMBOLS: [&str; 3] = ["__trueosNum", "__trueosNu", "__trueosN"];
    for symbol in SYMBOLS {
        while let Some(idx) = text.find(symbol) {
            text.replace_range(idx..idx + symbol.len(), "");
        }
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
    let mut op_code_counts = [0u32; 23];
    let mut unknown_op_count = 0u32;
    let mut text_sample_count = 0u32;
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
            // Listener registration is Pixi vocabulary, but first-frame rendering does not
            // need event dispatch wired yet. Keep it accepted so visual submit cannot hang.
            16 => true,
            17 => qjs::platform::ui::ui3_scene_remove_all_listeners(browser_instance_id, node),
            18 => qjs::platform::ui::ui3_scene_graphics_circle(browser_instance_id, node, a, b, c),
            19 => qjs::platform::ui::ui3_scene_graphics_move_to(browser_instance_id, node, a, b),
            20 => qjs::platform::ui::ui3_scene_graphics_line_to(browser_instance_id, node, a, b),
            22 => {
                let h = read_result_string(ctx, op_value, TRUESURFER_UI3_OP_TEXT_PROP)
                    .parse::<f32>()
                    .unwrap_or(d);
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
        "qjs-truesurfer[{}]: ui3 scene op-counts node={} addChild={} position={} clear={} rect={} fill={} stroke={} text={} textFill={} addChildAt={} setChildIndex={} removeChild={} removeFromParent={} removeChildren={} visible={} listen={} removeAllListeners={} circle={} moveTo={} lineTo={} texture={} unknown={}\n",
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
        unknown_op_count
    ));
    if skipped_empty_text_count != 0 {
        log_line(format!(
            "qjs-truesurfer[{}]: ui3 scene skipped_empty_text={} source={}\n",
            browser_instance_id, skipped_empty_text_count, command_source
        ));
    }

    log_line(format!(
        "qjs-truesurfer[{}]: ui3 scene render begin root={} submitted={}/{} op_submit_ms={}\n",
        browser_instance_id, root_id, submitted, op_count, op_submit_ms
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

unsafe fn submit_parse5_trueos_pixi_scene(
    rt: *mut qjs::JSRuntime,
    ctx: *mut qjs::JSContext,
    browser_instance_id: u32,
    html: &str,
) -> (u32, u32) {
    let _ = (rt, ctx, html);
    log_line(format!(
        "qjs-truesurfer[{}]: parse5 trueos vite app disabled; using text widget fallback\n",
        browser_instance_id
    ));
    return (0, 0);

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
    let had_asset_pending = qjs::async_ops::has_pending(ctx);
    if had_asset_pending {
        for _ in 0..TRUESURFER_PARSE5_ASSET_PUMP_BUDGET {
            if !runtime_has_pending_work(rt, ctx) {
                break;
            }
            if now_ms().saturating_sub(asset_wait_start_ms) >= TRUESURFER_PARSE5_ASSET_WAIT_MS {
                asset_pump_stopped = true;
                break;
            }
            if !qjs::vm::pump_runtime_once(rt, ctx, "truesurfer-parse5-trueos-assets") {
                asset_pump_stopped = true;
                break;
            }
            asset_pump_iters = asset_pump_iters.saturating_add(1);
        }
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
    let Some(signal) = html_ready_signal(browser_instance_id) else {
        return;
    };
    signal.wait().await;
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
    log_line(format!("qjs-truesurfer[{}]: raw ui3 submit begin\n", browser_instance_id));
    let (ui3_ops, ui3_root) =
        submit_parse5_trueos_pixi_scene(rt, ctx, browser_instance_id, pending.html.as_str());
    log_line(format!(
        "qjs-truesurfer[{}]: raw ui3 submit done ops={} root={}\n",
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
            "[TrueSurfer raw] browser={} ui3_ops={} root={} url={}\n",
            browser_instance_id, ui3_ops, ui3_root, parse_result.url,
        ));
        log_line(format!(
            "qjs-truesurfer[{}]: parsed bytes={} title={} ms={} shell_bytes={} body_bytes={} ui3_ops={} styles={} scripts={} url={}\n",
            browser_instance_id,
            parse_result.bytes,
            parse_result.title,
            parse_result.parse_ms,
            parse_result.shell_bytes,
            parse_result.body_bytes,
            ui3_ops,
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

            for _ in 0..TRUESURFER_BUSY_PUMP_BUDGET {
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
                if ready {
                    while let Some(pending) = take_queued_html_for_browser(browser_instance_id) {
                        let _ = dispatch_html(rt, ctx, browser_instance_id, pending);
                        dispatched_html = true;
                    }
                }

                busy = !ready || dispatched_html || runtime_has_pending_work(rt, ctx);
                if !busy {
                    break;
                }
            }

            if !runtime_alive {
                break;
            }

            if !busy && !runtime_has_pending_work(rt, ctx) && truesurfer_ready(ctx) {
                if let Some(pending) = take_queued_html_for_browser(browser_instance_id) {
                    let _ = dispatch_html(rt, ctx, browser_instance_id, pending);
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
