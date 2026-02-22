import * as PIXI from '/qjs/vendor/pixi.mjs';
// TRUEOS native module providing prebuilt font atlases (RGBA + alpha).
// Using this avoids per-frame Graphics geometry explosions from pixel-text.
import { getFontAtlasSmall } from 'trueos:text';
var G = (typeof globalThis !== 'undefined') ? globalThis : this;
var W = Number((G.window && G.window.innerWidth) || 0);
var H = Number((G.window && G.window.innerHeight) || 0);
if (!isFinite(W) || W < 320) W = 1280;
if (!isFinite(H) || H < 240) H = 800;
if (G && G.console && typeof G.console.log === 'function') {
	try { G.console.log('pixi_gui: W/H ' + String(W) + ' ' + String(H)); } catch (e) {}
}

var canvas = (G.document && G.document.createElement) ? G.document.createElement('canvas') : null;
if (!canvas) throw new Error('no canvas available');
canvas.width = W;
canvas.height = H;
G.__trueos_canvas = canvas;

var __Renderer = (PIXI && typeof PIXI.Renderer === 'function')
	? PIXI.Renderer
	: (PIXI && PIXI.default && typeof PIXI.default.Renderer === 'function')
		? PIXI.default.Renderer
		: null;
if (!__Renderer) {
	if (G && G.console && typeof G.console.log === 'function') {
		try {
			G.console.log('pixi_gui: missing PIXI.Renderer; keys=' + String(Object.keys(PIXI || {}).slice(0, 20)));
			G.console.log('pixi_gui: has default=' + String(!!(PIXI && PIXI.default)));
		} catch (e) {}
	}
	throw new Error('pixi_gui: PIXI.Renderer not found');
}

	G.__trueos_canvas = canvas;

	var rendererOpts = {
		view: canvas,
		width: W,
		height: H,
		backgroundColor: 0xFFFFFF,
		antialias: false
	};

	var renderer = null;
	// NOTE: Do not use PIXI.autoDetectRenderer() in TRUEOS.
	// It tries to create its own “test canvas” and probe contexts; if that canvas
	// doesn’t have getContext wired up in our shim, Pixi will fail to detect.
	var __Renderer = (PIXI && typeof PIXI.Renderer === 'function')
		? PIXI.Renderer
		: (PIXI && PIXI.default && typeof PIXI.default.Renderer === 'function')
			? PIXI.default.Renderer
			: null;
	if (__Renderer) {
		renderer = new __Renderer(rendererOpts);
	}
	if (!renderer) {
		throw new Error('pixi_gui: no renderer export (expected PIXI.Renderer)');
	}

var stage = new PIXI.Container();

// --- Atlas-backed small text renderer (6x6 fixed cell font)
// Falls back to drawPixelText(Graphics) if atlas upload fails.
var __smallFontAtlas = {
	ready: false,
	base: null,
	index: null, // Uint16Array char->slot
	cellW: 0,
	cellH: 0,
	gridW: 0,
	gridH: 0,
	texBySlot: null,
	slotQ: 0,
	slotSpace: 0,
	_initTried: false,
	_lastErr: null,
	_dbgLogged: false,
	init: function() {
		if (this.ready || this._initTried) return this.ready;
		this._initTried = true;
		try {
			if (!getFontAtlasSmall) return false;
			var a = getFontAtlasSmall();
			if (!a || !a.pixels) return false;
			var w = Number(a.width) | 0;
			var h = Number(a.height) | 0;
			this.cellW = Number(a.cellW) | 0;
			this.cellH = Number(a.cellH) | 0;
			this.gridW = Number(a.gridW) | 0;
			this.gridH = Number(a.gridH) | 0;
			if (!isFinite(w) || !isFinite(h) || w <= 0 || h <= 0) return false;
			if (this.cellW <= 0 || this.cellH <= 0 || this.gridW <= 0 || this.gridH <= 0) return false;
			var pixelsU8 = (a.pixels instanceof ArrayBuffer) ? new Uint8Array(a.pixels) : new Uint8Array(a.pixels || []);
			if (!pixelsU8 || pixelsU8.length < (w * h * 4)) return false;
			if (!(PIXI.BaseTexture && PIXI.BaseTexture.fromBuffer)) return false;
			var base = PIXI.BaseTexture.fromBuffer(pixelsU8, w, h);
			// NOTE: This atlas is typically NPOT (non power-of-two). Be strict about
			// WebGL1-friendly settings to avoid silent black textures.
			if (PIXI.MIPMAP_MODES && typeof base.mipmap !== 'undefined') {
				base.mipmap = PIXI.MIPMAP_MODES.OFF;
			}
			if (PIXI.WRAP_MODES && typeof base.wrapMode !== 'undefined') {
				base.wrapMode = PIXI.WRAP_MODES.CLAMP;
			}
			if (PIXI.ALPHA_MODES && typeof base.alphaMode !== 'undefined') {
				base.alphaMode = PIXI.ALPHA_MODES.NO_PREMULTIPLIED_ALPHA;
			}
			// Nearest sampling keeps 6x6 pixel glyphs crisp when scaled.
			if (typeof base.scaleMode !== 'undefined' && PIXI.SCALE_MODES) {
				base.scaleMode = PIXI.SCALE_MODES.NEAREST;
			}
			if (typeof base.update === 'function') {
				try { base.update(); } catch (e) {}
			}
			this.base = base;
			// index is provided as little-endian u16 bytes; TRUEOS runs little-endian.
			this.index = new Uint16Array(a.index);
			this.texBySlot = new Array(this.gridW * this.gridH);
			// Prefer '?' and space as fallbacks if available.
			this.slotQ = this._slotForCharCode(63); // '?'
			this.slotSpace = this._slotForCharCode(32); // ' '
			this.ready = true;
			if (!this._dbgLogged && G && G.console && typeof G.console.log === 'function') {
				this._dbgLogged = true;
				try {
					G.console.log('pixi_gui: small font atlas ok ' + String(w) + 'x' + String(h) + ' cell ' + String(this.cellW) + 'x' + String(this.cellH) + ' grid ' + String(this.gridW) + 'x' + String(this.gridH));
				} catch (e) {}
			}
			return true;
		} catch (e) {
			this._lastErr = String(e);
			if (!this._dbgLogged && G && G.console && typeof G.console.log === 'function') {
				this._dbgLogged = true;
				try { G.console.log('pixi_gui: small font atlas failed ' + String(e)); } catch (e2) {}
			}
			this.ready = false;
			return false;
		}
	},
	_slotForCharCode: function(code) {
		code = Number(code) | 0;
		if (!this.index || code < 0 || code > 255) return 0xFFFF;
		var slot = this.index[code] | 0;
		return slot;
	},
	_slotForChar: function(ch) {
		if (ch === ' ') return this.slotSpace;
		var code = (ch && ch.length) ? (ch.charCodeAt(0) | 0) : 32;
		if (code < 0 || code > 255) code = 63;
		var slot = this._slotForCharCode(code);
		if (slot === 0xFFFF) slot = this.slotQ;
		if (slot === 0xFFFF) slot = this.slotSpace;
		return slot;
	},
	_texForSlot: function(slot) {
		slot = Number(slot) | 0;
		if (!this.base || !this.texBySlot) return null;
		if (slot < 0 || slot >= this.texBySlot.length) return null;
		var t = this.texBySlot[slot];
		if (t) return t;
		var gx = (slot % this.gridW) | 0;
		var gy = ((slot / this.gridW) | 0);
		var rx = gx * this.cellW;
		var ry = gy * this.cellH;
		var rect = (PIXI.Rectangle)
			? new PIXI.Rectangle(rx, ry, this.cellW, this.cellH)
			: { x: rx, y: ry, width: this.cellW, height: this.cellH };
		t = new PIXI.Texture(this.base, rect);
		// Some WebGL shims/backends can get stuck with stale UVs for sub-rect textures.
		// Nudging Pixi to recompute avoids "invisible glyph" failures.
		if (t) {
			if (typeof t.updateUvs === 'function') {
				try { t.updateUvs(); } catch (e) {}
			} else if (typeof t._updateUvs === 'function') {
				try { t._updateUvs(); } catch (e2) {}
			}
		}
		this.texBySlot[slot] = t;
		return t;
	},
	createText: function() {
		if (!this.init()) return null;
		var self = this;
		var obj = {
			node: new PIXI.Container(),
			sprites: [],
			_lastText: null,
			_lastX: 0,
			_lastY: 0,
			_lastScale: 0,
			_lastColor: 0,
			setText: function(text, x, y, scale, color) {
				text = String(text == null ? '' : text);
				x = Number(x) | 0;
				y = Number(y) | 0;
				scale = Math.max(1, Number(scale) | 0);
				color = (Number(color) >>> 0);
				if (this._lastText === text && this._lastX === x && this._lastY === y && this._lastScale === scale && this._lastColor === color) {
					return;
				}
				this._lastText = text;
				this._lastX = x;
				this._lastY = y;
				this._lastScale = scale;
				this._lastColor = color;

				var cx = x;
				var cy = y;
				var want = text.length;
				for (var i = 0; i < want; i++) {
					var ch = text[i];
					var slot = self._slotForChar(ch);
					var tex = self._texForSlot(slot);
					var spr = this.sprites[i];
					if (!spr) {
						spr = new PIXI.Sprite(tex);
						this.sprites[i] = spr;
						this.node.addChild(spr);
					} else if (tex && spr.texture !== tex) {
						spr.texture = tex;
					}
					spr.visible = true;
					spr.x = cx;
					spr.y = cy;
					spr.width = self.cellW * scale;
					spr.height = self.cellH * scale;
					spr.tint = color;
					cx += (self.cellW + 1) * scale;
				}
				for (var j = want; j < this.sprites.length; j++) {
					if (this.sprites[j]) this.sprites[j].visible = false;
				}
			}
		};
		return obj;
	}
};

function createUiText() {
	// Atlas-backed text is the fast path. For debugging/fallback you can force the
	// old Graphics-based font:
	// `globalThis.__trueos_force_pixel_text = 1;`
	var forcePixel = !!(G && G.__trueos_force_pixel_text);
	if (!forcePixel) {
		var t = __smallFontAtlas.createText();
		if (t) return t;
	}
	// Fallback: pixel font via Graphics (slow, but keeps UI usable).
	var g = new PIXI.Graphics();
	return {
		node: g,
		_lastText: null,
		_lastX: 0,
		_lastY: 0,
		_lastScale: 0,
		_lastColor: 0,
		setText: function(text, x, y, scale, color) {
			text = String(text == null ? '' : text);
			x = Number(x) | 0;
			y = Number(y) | 0;
			scale = Math.max(1, Number(scale) | 0);
			color = (Number(color) >>> 0);
			if (this._lastText === text && this._lastX === x && this._lastY === y && this._lastScale === scale && this._lastColor === color) {
				return;
			}
			this._lastText = text;
			this._lastX = x;
			this._lastY = y;
			this._lastScale = scale;
			this._lastColor = color;
			drawPixelText(g, text, x, y, scale, color);
		}
	};
}

// Debug marker sprite that uses the same simple textured-sprite pipeline as the
// background (no Graphics). This should remain visible even if Graphics paths
// are flaky.
var __dbg_mark = null;

// --- Desktop background: left -> right white -> black gradient.
// Use a 2x1 texture stretched fullscreen so we don't depend on vertex color attributes.
var bg = new PIXI.Container();
stage.addChild(bg);

function bgFallback() {
	var g = new PIXI.Graphics();
	var mid = Math.floor(W / 2);
	g.beginFill(0xFFFFFF, 1);
	g.drawRect(0, 0, mid, H);
	g.endFill();
	g.beginFill(0x000000, 1);
	g.drawRect(mid, 0, Math.max(1, W - mid), H);
	g.endFill();
	bg.addChild(g);
}

try {
	var buf = new Uint8Array([
		255, 255, 255, 255,
		0, 0, 0, 255,
	]);
	var base = (PIXI.BaseTexture && PIXI.BaseTexture.fromBuffer)
		? PIXI.BaseTexture.fromBuffer(buf, 2, 1)
		: null;
	if (!base) throw new Error('BaseTexture.fromBuffer missing');
	if (typeof base.scaleMode !== 'undefined' && PIXI.SCALE_MODES) {
		base.scaleMode = PIXI.SCALE_MODES.LINEAR;
	}
	var tex = new PIXI.Texture(base);
	var spr = new PIXI.Sprite(tex);
	spr.x = 0;
	spr.y = 0;
	spr.width = W;
	spr.height = H;
	bg.addChild(spr);

	// A smaller sprite using the same texture as an always-on marker.
	__dbg_mark = new PIXI.Sprite(tex);
	__dbg_mark.width = 64;
	__dbg_mark.height = 64;
	__dbg_mark.alpha = 1;
	__dbg_mark.x = Math.floor(W / 2) - 32;
	__dbg_mark.y = 16;
} catch (e) {
	if (G && G.console && typeof G.console.log === 'function') {
		try { G.console.log('pixi_gui: bg texture failed ' + String(e)); } catch (e2) {}
	}
	bgFallback();
}

// --- Very small pixel font (5x7), drawn using Graphics so it doesn't rely on canvas text.
// Only includes: A-Z, 0-9, space, dash, dot.
var FONT = {
	' ': [
		'00000','00000','00000','00000','00000','00000','00000'
	],
	'-': [
		'00000','00000','00000','11111','00000','00000','00000'
	],
	'.': [
		'00000','00000','00000','00000','00000','00110','00110'
	],
	'0': ['01110','10001','10011','10101','11001','10001','01110'],
	'1': ['00100','01100','00100','00100','00100','00100','01110'],
	'2': ['01110','10001','00001','00010','00100','01000','11111'],
	'3': ['11110','00001','00001','01110','00001','00001','11110'],
	'4': ['00010','00110','01010','10010','11111','00010','00010'],
	'5': ['11111','10000','10000','11110','00001','00001','11110'],
	'6': ['00110','01000','10000','11110','10001','10001','01110'],
	'7': ['11111','00001','00010','00100','01000','01000','01000'],
	'8': ['01110','10001','10001','01110','10001','10001','01110'],
	'9': ['01110','10001','10001','01111','00001','00010','11100'],
	'A': ['01110','10001','10001','11111','10001','10001','10001'],
	'B': ['11110','10001','10001','11110','10001','10001','11110'],
	'C': ['01110','10001','10000','10000','10000','10001','01110'],
	'D': ['11100','10010','10001','10001','10001','10010','11100'],
	'E': ['11111','10000','10000','11110','10000','10000','11111'],
	'F': ['11111','10000','10000','11110','10000','10000','10000'],
	'G': ['01110','10001','10000','10111','10001','10001','01110'],
	'H': ['10001','10001','10001','11111','10001','10001','10001'],
	'I': ['01110','00100','00100','00100','00100','00100','01110'],
	'J': ['00111','00010','00010','00010','00010','10010','01100'],
	'K': ['10001','10010','10100','11000','10100','10010','10001'],
	'L': ['10000','10000','10000','10000','10000','10000','11111'],
	'M': ['10001','11011','10101','10101','10001','10001','10001'],
	'N': ['10001','11001','10101','10011','10001','10001','10001'],
	'O': ['01110','10001','10001','10001','10001','10001','01110'],
	'P': ['11110','10001','10001','11110','10000','10000','10000'],
	'Q': ['01110','10001','10001','10001','10101','10010','01101'],
	'R': ['11110','10001','10001','11110','10100','10010','10001'],
	'S': ['01111','10000','10000','01110','00001','00001','11110'],
	'T': ['11111','00100','00100','00100','00100','00100','00100'],
	'U': ['10001','10001','10001','10001','10001','10001','01110'],
	'V': ['10001','10001','10001','10001','10001','01010','00100'],
	'W': ['10001','10001','10001','10101','10101','10101','01010'],
	'X': ['10001','10001','01010','00100','01010','10001','10001'],
	'Y': ['10001','10001','01010','00100','00100','00100','00100'],
	'Z': ['11111','00001','00010','00100','01000','10000','11111'],
};

function drawPixelText(g, text, x, y, scale, color) {
	g.clear();
	g.beginFill(color >>> 0);
	var cx = x;
	var cy = y;
	var s = scale;
	for (var i = 0; i < text.length; i++) {
		var ch = String(text[i]).toUpperCase();
		var glyph = FONT[ch] || FONT[' '];
		for (var row = 0; row < 7; row++) {
			var line = glyph[row];
			for (var col = 0; col < 5; col++) {
				if (line[col] === '1') {
					g.drawRect(cx + col * s, cy + row * s, s, s);
				}
			}
		}
		cx += (6 * s);
	}
	g.endFill();
}

function countNodes(root) {
	// Counts DisplayObject tree nodes including root.
	var n = 0;
	var stack = [root];
	while (stack.length) {
		var cur = stack.pop();
		if (!cur) continue;
		n++;
		if (cur.children && cur.children.length) {
			for (var i = 0; i < cur.children.length; i++) {
				stack.push(cur.children[i]);
			}
		}
	}
	return n;
}

function createWindow(frameX, frameY, frameW, frameH, appIdx) {
	var win = {
		frameX: frameX|0,
		frameY: frameY|0,
		frameW: frameW|0,
		frameH: frameH|0,
		appIdx: appIdx|0,
		inst: null,
		chrome: new PIXI.Container(),
		host: new PIXI.Container(),
		nameText: createUiText(),
		countText: createUiText(),
		_countLast: null,
	};
	stage.addChild(win.host);
	stage.addChild(win.chrome);

	var frame = new PIXI.Graphics();
	frame.lineStyle(2, 0x202020, 1);
	frame.beginFill(0xFFFFFF, 0.18);
	frame.drawRect(win.frameX, win.frameY, win.frameW, win.frameH);
	frame.endFill();
	win.chrome.addChild(frame);

	// 3 buttons that stick outside on the left/top
	var btn = new PIXI.Graphics();
	btn.beginFill(0x202020, 1);
	btn.drawRect(win.frameX - 18, win.frameY + 18, 14, 14);
	btn.drawRect(win.frameX - 18, win.frameY + 38, 14, 14);
	btn.drawRect(win.frameX + win.frameW - 32, win.frameY - 18, 14, 14);
	btn.endFill();
	win.chrome.addChild(btn);

	win.chrome.addChild(win.nameText.node);
	win.chrome.addChild(win.countText.node);
	return win;
}

// --- Multi-app registry
var UI = {
	apps: [],
	windows: [],
	started: false,
	_cursor: null,
	_cursorX: 0,
	_cursorY: 0,
	registerApp: function(name, factory) {
		this.apps.push({ name: String(name || 'APP'), factory: factory });
		return this.apps.length - 1;
	},
	start: function() {
		if (this.started) return;
		this.started = true;

		if (this.apps.length === 0) {
			this.registerApp('DEMO', function() {
				var root = new PIXI.Container();
				return { root: root, tick: function(dt) {} };
			});
			this.registerApp('BOX', function() {
				var root = new PIXI.Container();
				var g = new PIXI.Graphics();
				g.lineStyle(2, 0x202020, 1);
				g.beginFill(0xFFFFFF, 0.35);
				g.drawRect(10, 10, 180, 120);
				g.endFill();
				g.beginFill(0x000000, 0.20);
				g.drawRect(20, 150, 160, 220);
				g.endFill();
				root.addChild(g);
				return { root: root, tick: function(dt) {} };
			});
			// Minimal HTML->pixels demo.
			// This is the first building block for a real browser UI: prove we can
			// fetch/import a JS HTML parser (parse5), parse HTML, and paint readable
			// output via the existing Pixi/WebGL layer.
			this.registerApp('WEB', function() {
				var root = new PIXI.Container();
				var frame = new PIXI.Graphics();
				var titleText = createUiText();
				var statusText = createUiText();
				var lines = [];
				var lineText = [];

				root.addChild(frame);
				root.addChild(titleText.node);
				root.addChild(statusText.node);

				// 12 lines should fit in the default window sizes.
				for (var i = 0; i < 12; i++) {
					var lt = createUiText();
					lineText.push(lt);
					root.addChild(lt.node);
				}

				var st = {
					started: false,
					done: false,
					err: null,
					dirty: true,
					bytes: 0,
					phase: 'init',
					worker: null,
				};

				function safeStr(s) {
					s = String(s == null ? '' : s);
					// avoid excessive redraw cost
					if (s.length > 120) s = s.slice(0, 120);
					return s;
				}

				function splitPreview(text) {
					text = String(text == null ? '' : text);
					var out = [];
					var parts = text.split(/\r?\n/);
					for (var i = 0; i < parts.length && out.length < 12; i++) {
						var s = parts[i];
						if (typeof s !== 'string') continue;
						s = s.replace(/\s+/g, ' ').trim();
						if (!s) continue;
						out.push(s);
					}
					if (out.length === 0) out.push('(empty)');
					return out;
				}

				function startParse() {
					if (st.started) return;
					st.started = true;
					st.err = null;
					st.dirty = true;

					var url = 'https://example.de/';
					st.url = url;
					st.bytes = 0;
					st.phase = 'worker';

					// Load in a dedicated worker VM so the UI tick stays responsive.
					// NOTE: TRUEOS Worker() currently takes a code string (not a module URL).
					var code = '';
					code += "import { parentPort } from 'node:worker_threads';\n";
					code += "function splitPreview(text) {\n";
					code += "  text = String(text == null ? '' : text);\n";
					code += "  var out = [];\n";
					code += "  var parts = text.split(/\\r?\\n/);\n";
					code += "  for (var i = 0; i < parts.length && out.length < 12; i++) {\n";
					code += "    var s = parts[i];\n";
					code += "    if (typeof s !== 'string') continue;\n";
					code += "    s = s.replace(/\\s+/g, ' ').trim();\n";
					code += "    if (!s) continue;\n";
					code += "    if (s.length > 120) s = s.slice(0, 120);\n";
					code += "    out.push(s);\n";
					code += "  }\n";
					code += "  if (out.length === 0) out.push('(empty)');\n";
					code += "  return out;\n";
					code += "}\n";
					code += "(async function(){\n";
					code += "  try {\n";
					code += "    if (typeof fetch !== 'function') throw new Error('fetch() missing');\n";
					code += "    var url = " + JSON.stringify(url) + ";\n";
					code += "    var body = await fetch(url);\n";
					code += "    body = String(body == null ? '' : body);\n";
					code += "    var msg = { kind: 'web-ready', ok: 1, url: url, bytes: (body.length|0), lines: splitPreview(body) };\n";
					code += "    parentPort.postMessage(JSON.stringify(msg));\n";
					code += "  } catch (e) {\n";
					code += "    var msg = { kind: 'web-ready', ok: 0, err: String(e) };\n";
					code += "    parentPort.postMessage(JSON.stringify(msg));\n";
					code += "  }\n";
					code += "})();\n";

					try {
						st.worker = new Worker(code);
					} catch (e) {
						st.err = e;
						st.phase = 'err';
						st.dirty = true;
						return;
					}

					// Worker callbacks receive a single string argument (not an event object).
					st.worker.onMessage(function(msgStr) {
						var obj = null;
						try { obj = JSON.parse(String(msgStr)); } catch (e) { obj = null; }
						if (!obj || obj.kind !== 'web-ready') return;
						if (obj.ok) {
							st.bytes = Number(obj.bytes || 0) | 0;
							lines = Array.isArray(obj.lines) ? obj.lines : [];
							st.done = true;
							st.phase = 'done';
						} else {
							st.err = obj.err || 'worker failed';
							st.done = false;
							st.phase = 'err';
						}
						st.dirty = true;
						try { st.worker.terminate(); } catch (e) {}
						st.worker = null;
					});
				}

				function redraw() {
					st.dirty = false;

					frame.clear();
					frame.lineStyle(2, 0x202020, 1);
					frame.beginFill(0xFFFFFF, 0.10);
					frame.drawRect(6, 6, 188, 388);
					frame.endFill();

					titleText.setText('WEB', 12, 12, 2, 0x202020);

					var status = 'loading in worker...';
					if (st.phase === 'done') status = 'ok bytes ' + String(st.bytes|0);
					if (st.err) status = 'ERR ' + safeStr(st.err);
					statusText.setText(safeStr(status), 12, 34, 2, 0x202020);

					// Only "present" content once it is ready.
					for (var i = 0; i < lineText.length; i++) {
						var t = (st.done && i < lines.length) ? lines[i] : '';
						lineText[i].setText(safeStr(t), 12, 60 + i * 22, 2, 0x202020);
					}
				}

				return {
					root: root,
					tick: function(dt) {
						if (!st.started) startParse();
						if (st.dirty) redraw();
					}
				};
			});
		}

		// Smaller default windows, placed left/center/right.
		var y = Math.max(40, Math.floor(H * 0.10));
		var w = Math.min(200, Math.max(120, Math.floor((W - 24 * 4) / 3)));
		var h = Math.min(400, Math.max(160, H - y - 40));
		var xL = 24;
		var xC = Math.max(24, Math.floor((W - w) / 2));
		var xR = Math.max(24, W - w - 24);

		this.windows.push(createWindow(xL, y, w, h, 0));
		this.windows.push(createWindow(xC, y, w, h, 1));
		// Keep the default desktop lightweight; WEB can be enabled later.
		this.windows.push(createWindow(xR, y, w, h, 0));

		// Cursor triangle (single triangle). Tip is exactly at cursor position.
		this._cursorX = Math.floor(W / 2);
		this._cursorY = Math.floor(H / 2);
		this._cursorAngle = 0;
		// Prefer a sprite cursor so we don't depend on PIXI.Graphics (which may
		// rely on stencil/mask paths we don't fully implement).
		this._cursor = null;
		try {
			var cw = 24, ch = 24;
			var cbuf = new Uint8Array(cw * ch * 4);
			// Simple filled triangle with a thin 1px outline-ish edge.
			for (var y = 0; y < ch; y++) {
				for (var x = 0; x < cw; x++) {
					// Triangle with tip at (0,0), base towards bottom-right.
					// Condition: x >= 0, y >= 0, and x <= y*0.75 + 10
					var inside = (x >= 0 && y >= 0 && x <= ((y * 3) >> 2) + 10);
					if (!inside) continue;
					var idx = (y * cw + x) * 4;
					// Dark fill
					cbuf[idx + 0] = 0x20;
					cbuf[idx + 1] = 0x20;
					cbuf[idx + 2] = 0x20;
					cbuf[idx + 3] = 0xFF;
					// Add a light edge at the left/top boundaries
					if (x === 0 || y === 0) {
						cbuf[idx + 0] = 0xFF;
						cbuf[idx + 1] = 0xFF;
						cbuf[idx + 2] = 0xFF;
						cbuf[idx + 3] = 0xFF;
					}
				}
			}
			var cbase = (PIXI.BaseTexture && PIXI.BaseTexture.fromBuffer)
				? PIXI.BaseTexture.fromBuffer(cbuf, cw, ch)
				: null;
			if (!cbase) throw new Error('cursor BaseTexture.fromBuffer missing');
			if (typeof cbase.scaleMode !== 'undefined' && PIXI.SCALE_MODES) {
				cbase.scaleMode = PIXI.SCALE_MODES.NEAREST;
			}
			var ctex = new PIXI.Texture(cbase);
			var cspr = new PIXI.Sprite(ctex);
			cspr.x = this._cursorX | 0;
			cspr.y = this._cursorY | 0;
			cspr.alpha = 1;
			this._cursor = cspr;
			stage.addChild(this._cursor);
		} catch (e) {
			this._cursor = new PIXI.Graphics();
			stage.addChild(this._cursor);
		}

		// Add the debug marker last so it stays on top of windows.
		if (__dbg_mark && !__dbg_mark.__trueos_added) {
			__dbg_mark.__trueos_added = 1;
			stage.addChild(__dbg_mark);
		}

		if (G && G.addEventListener) {
			var self = this;
			try {
				G.addEventListener('mousemove', function(e) {
					if (!e) return;
					var dx = Number(e.movementX);
					var dy = Number(e.movementY);
					if (isFinite(dx) && isFinite(dy) && ((dx|0) !== 0 || (dy|0) !== 0)) {
						self._cursorX = Math.max(0, Math.min(W - 1, (self._cursorX + (dx|0))|0));
						self._cursorY = Math.max(0, Math.min(H - 1, (self._cursorY + (dy|0))|0));
						return;
					}
					var x = Number(e.clientX);
					var y = Number(e.clientY);
					if (!isFinite(x) || !isFinite(y)) return;
					self._cursorX = Math.max(0, Math.min(W - 1, x|0));
					self._cursorY = Math.max(0, Math.min(H - 1, y|0));
				}, false);
			} catch (e) {}
		}
	},
	_ensureWindowInst: function(win) {
		if (!win || win.inst) return;
		var idx = (this.apps.length > 0) ? (win.appIdx % this.apps.length) : 0;
		var spec = this.apps[idx];
		var inst = spec.factory ? spec.factory() : null;
		if (!inst) inst = { root: new PIXI.Container(), tick: function(){} };
		if (!inst.root) inst.root = new PIXI.Container();
		inst.root.x = win.frameX;
		inst.root.y = win.frameY;
		win.host.addChild(inst.root);
		win.inst = inst;
		win.nameText.setText(spec.name, win.frameX, Math.max(2, win.frameY - 28), 2, 0x202020);
	},
	tick: function(dt, angleRad) {
		this._cursorAngle = Number(angleRad);

		// Debug marker follows the cursor position (uses Sprite pipeline).
		if (__dbg_mark) {
			var mx = this._cursorX|0;
			var my = this._cursorY|0;
			__dbg_mark.x = (mx - 32) | 0;
			__dbg_mark.y = (my - 32) | 0;
			__dbg_mark.rotation = 0;
		}

		// Visual heartbeat: gently wobble the center window left/right.
		var wobble = 0;
		if (isFinite(this._cursorAngle)) {
			wobble = (Math.sin(this._cursorAngle) * 16) | 0;
		}
		for (var i = 0; i < this.windows.length; i++) {
			var win = this.windows[i];
			// Only the middle window wobbles.
			if (win && win.host && win.chrome) {
				var dx = (i === 1) ? wobble : 0;
				win.host.x = dx;
				win.chrome.x = dx;
			}
			this._ensureWindowInst(win);
			if (win && win.inst && win.inst.tick) {
				try { win.inst.tick(dt); } catch (e) {}
			}
			// element count: number only, bottom-right outside frame (per window)
			var n = (win && win.inst && win.inst.root) ? countNodes(win.inst.root) : 0;
			var s = String(n|0);
			if (win._countLast !== s) {
				win._countLast = s;
				var x = win.frameX + win.frameW - (s.length * 14) - 4;
				var y = Math.min(H - 24, win.frameY + win.frameH + 8);
				win.countText.setText(s, x, y, 2, 0x202020);
			}
		}

		// Draw cursor last.
		if (this._cursor) {
			var x = this._cursorX|0;
			var y = this._cursorY|0;
			if (this._cursor.clear) {
				// Graphics fallback.
				this._cursor.clear();
				this._cursor.lineStyle(1, 0xFFFFFF, 1);
				this._cursor.beginFill(0x202020, 1);
				this._cursor.moveTo(x, y);
				this._cursor.lineTo(x + 14, y + 7);
				this._cursor.lineTo(x + 6, y + 20);
				this._cursor.lineTo(x, y);
				this._cursor.endFill();
			} else {
				// Sprite cursor.
				this._cursor.x = x;
				this._cursor.y = y;
			}
		}
	}
};

G.__trueos_ui = UI;

var last = 0;
G.__trueos_pixi_ui_tick = function(angleRad) {
	var now = (Date.now && Date.now()) ? Date.now() : (last + 50);
	var dt = (last === 0) ? 50 : (now - last);
	last = now;
	if (G.__trueos_mouse_pump) {
		try { G.__trueos_mouse_pump(); } catch (e) {}
	}
	if (G.__trueos_ui) {
		if (!G.__trueos_ui.started) G.__trueos_ui.start();
		G.__trueos_ui.tick(dt, angleRad);
	}
	renderer.render(stage);
};

// Cursor mode toggling was removed; cursor always follows mouse coordinates.

// Self-driven 20Hz loop (preferred). If timers are unavailable, Rust can still
// drive `__trueos_pixi_ui_tick` externally.
if (!G.__trueos_pixi_ui_running && typeof setInterval === 'function') {
	G.__trueos_pixi_ui_running = true;
	G.__trueos_pixi_ui_a = G.__trueos_pixi_ui_a || 0;
	G.__trueos_pixi_ui_timer = setInterval(function() {
		G.__trueos_pixi_ui_a = (G.__trueos_pixi_ui_a || 0) + 0.03;
		if (G.__trueos_pixi_ui_tick) G.__trueos_pixi_ui_tick(G.__trueos_pixi_ui_a);
	}, 50);
	G.__trueos_pixi_ui_stop = function() {
		try {
			if (G.__trueos_pixi_ui_timer && typeof clearInterval === 'function') {
				clearInterval(G.__trueos_pixi_ui_timer);
			}
		} catch (e) {}
		G.__trueos_pixi_ui_timer = null;
		G.__trueos_pixi_ui_running = false;
	};
}
