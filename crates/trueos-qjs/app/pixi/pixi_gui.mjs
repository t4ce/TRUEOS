import * as PIXI from 'https://esm.sh/pixi.js@7.4.3?bundle';
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
	if (PIXI && typeof PIXI.autoDetectRenderer === 'function') {
		renderer = PIXI.autoDetectRenderer(rendererOpts);
	} else if (PIXI && typeof PIXI.Renderer === 'function') {
		renderer = new PIXI.Renderer(rendererOpts);
	}
	if (!renderer) {
		throw new Error('pixi_gui: no renderer export (expected PIXI.autoDetectRenderer or PIXI.Renderer)');
	}

var stage = new PIXI.Container();

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
		nameGfx: new PIXI.Graphics(),
		countGfx: new PIXI.Graphics(),
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

	win.chrome.addChild(win.nameGfx);
	win.chrome.addChild(win.countGfx);
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
		this.windows.push(createWindow(xR, y, w, h, 0));

		// Cursor triangle (single triangle). Tip is exactly at cursor position.
		this._cursorX = Math.floor(W / 2);
		this._cursorY = Math.floor(H / 2);
		this._cursor = new PIXI.Graphics();
		stage.addChild(this._cursor);

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
		drawPixelText(win.nameGfx, spec.name, win.frameX, Math.max(2, win.frameY - 28), 2, 0x202020);
	},
	tick: function(dt) {
		for (var i = 0; i < this.windows.length; i++) {
			var win = this.windows[i];
			this._ensureWindowInst(win);
			if (win && win.inst && win.inst.tick) {
				try { win.inst.tick(dt); } catch (e) {}
			}
			// element count: number only, bottom-right outside frame (per window)
			var n = (win && win.inst && win.inst.root) ? countNodes(win.inst.root) : 0;
			var s = String(n|0);
			var x = win.frameX + win.frameW - (s.length * 12) - 4;
			var y = Math.min(H - 24, win.frameY + win.frameH + 8);
			drawPixelText(win.countGfx, s, x, y, 2, 0x202020);
		}

		// Draw cursor last.
		if (this._cursor) {
			var x = this._cursorX|0;
			var y = this._cursorY|0;
			this._cursor.clear();
			this._cursor.lineStyle(1, 0xFFFFFF, 1);
			this._cursor.beginFill(0x202020, 1);
			// Simple arrow-like triangle; tip is exactly at (x,y).
			this._cursor.moveTo(x, y);
			this._cursor.lineTo(x + 14, y + 7);
			this._cursor.lineTo(x + 6, y + 20);
			this._cursor.lineTo(x, y);
			this._cursor.endFill();
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
		G.__trueos_ui.tick(dt);
	}
	renderer.render(stage);
};
