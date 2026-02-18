import * as PIXI from 'https://cdn.jsdelivr.net/npm/pixi.js@7.4.3/+esm';
var G = (typeof globalThis !== 'undefined') ? globalThis : this;
var W = Number((G.window && G.window.innerWidth) || 0);
var H = Number((G.window && G.window.innerHeight) || 0);
if (!isFinite(W) || W < 320) W = 1280;
if (!isFinite(H) || H < 240) H = 800;
if (G && G.console && G.console.log) G.console.log('pixi_gui: W/H', W, H);

var canvas = (G.document && G.document.createElement) ? G.document.createElement('canvas') : null;
if (!canvas) throw new Error('no canvas available');
canvas.width = W;
canvas.height = H;
G.__trueos_canvas = canvas;

var renderer = new PIXI.Renderer({
	view: canvas,
	width: W,
	height: H,
	backgroundColor: 0xFFFFFF,
	antialias: false
});

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
	if (G && G.console && G.console.log) G.console.log('pixi_gui: bg texture failed', String(e));
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

// --- Shared chrome / app host
var chrome = new PIXI.Container();
stage.addChild(chrome);

var appHost = new PIXI.Container();
stage.addChild(appHost);

var frameX = Math.max(24, Math.floor(W * 0.08));
var frameY = Math.max(40, Math.floor(H * 0.10));
var frameW = Math.max(200, W - frameX - 24);
var frameH = Math.max(160, H - frameY - 32);

var frame = new PIXI.Graphics();
frame.lineStyle(2, 0x202020, 1);
frame.beginFill(0xFFFFFF, 0.18);
frame.drawRect(frameX, frameY, frameW, frameH);
frame.endFill();
chrome.addChild(frame);

// 3 buttons that stick outside on the left/top
var btn = new PIXI.Graphics();
btn.beginFill(0x202020, 1);
btn.drawRect(frameX - 18, frameY + 18, 14, 14);
btn.drawRect(frameX - 18, frameY + 38, 14, 14);
btn.drawRect(frameX + 18, frameY - 18, 14, 14);
btn.endFill();
chrome.addChild(btn);

var nameGfx = new PIXI.Graphics();
chrome.addChild(nameGfx);

var countGfx = new PIXI.Graphics();
chrome.addChild(countGfx);

// --- Multi-app registry
var UI = {
	apps: [],
	active: -1,
	_activeInst: null,
	registerApp: function(name, factory) {
		this.apps.push({ name: String(name || 'APP'), factory: factory });
		return this.apps.length - 1;
	},
	setActive: function(idx) {
		idx = Number(idx|0);
		if (idx < 0 || idx >= this.apps.length) return;
		this.active = idx;
		if (this._activeInst && this._activeInst.root) {
			try { appHost.removeChild(this._activeInst.root); } catch (e) {}
		}
		var spec = this.apps[idx];
		if (G && G.console && G.console.log) G.console.log('pixi_gui: setActive name=', spec.name, 'len=', String(spec.name).length);
		var inst = spec.factory ? spec.factory() : null;
		if (!inst) inst = { root: new PIXI.Container(), tick: function(){} };
		if (!inst.root) inst.root = new PIXI.Container();
		inst.root.x = frameX;
		inst.root.y = frameY;
		appHost.addChild(inst.root);
		this._activeInst = inst;
		drawPixelText(nameGfx, spec.name, frameX, Math.max(2, frameY - 28), 2, 0x202020);
	},
	start: function() {
		if (this.apps.length === 0) {
			this.registerApp('DEMO', function() {
				var root = new PIXI.Container();
				return { root: root, tick: function(dt) {} };
			});
		}
		this.setActive(0);
	},
	tick: function(dt) {
		if (this._activeInst && this._activeInst.tick) {
			try { this._activeInst.tick(dt); } catch (e) {}
		}
		// element count: number only, bottom-right outside frame
		var n = (this._activeInst && this._activeInst.root) ? countNodes(this._activeInst.root) : 0;
		var s = String(n|0);
		var x = frameX + frameW - (s.length * 12) - 4;
		var y = Math.min(H - 24, frameY + frameH + 8);
		drawPixelText(countGfx, s, x, y, 2, 0x202020);
	}
};

G.__trueos_ui = UI;

var last = 0;
var __mx = Math.floor(W / 2);
var __my = Math.floor(H / 2);
var __mb = 0;

function __dispatchMouse(target, type, props) {
	if (!target || !target.dispatchEvent) return;
	var e = props || {};
	e.type = type;
	try { target.dispatchEvent(e); } catch (err) {}
}

function __pumpMouse() {
	if (!G.__trueos_poll_mouse_raw) return;
	var canvas = G.__trueos_canvas;
	for (var k = 0; k < 64; k++) {
		var ev = null;
		try { ev = G.__trueos_poll_mouse_raw(); } catch (e) { return; }
		if (!ev) return;
		var dx = (ev.dx|0);
		var dy = (ev.dy|0);
		var wh = (ev.wheel|0);
		var buttons = (ev.buttons|0);
		if (dx || dy) {
			__mx = Math.max(0, Math.min(W - 1, __mx + dx));
			__my = Math.max(0, Math.min(H - 1, __my + dy));
			var props = { clientX: __mx, clientY: __my, movementX: dx, movementY: dy, buttons: buttons };
			__dispatchMouse(canvas, 'mousemove', props);
			__dispatchMouse(G, 'mousemove', props);
		}
		if (wh) {
			var propsw = { clientX: __mx, clientY: __my, deltaY: wh, buttons: buttons };
			__dispatchMouse(canvas, 'wheel', propsw);
			__dispatchMouse(G, 'wheel', propsw);
		}
		var changed = (__mb ^ buttons) & 0xFF;
		if (changed) {
			var map = [ [1,0], [4,1], [2,2] ]; // HID:1 left,4 middle,2 right
			for (var i = 0; i < map.length; i++) {
				var mask = map[i][0];
				var btn = map[i][1];
				if (changed & mask) {
					var down = (buttons & mask) ? 1 : 0;
					var t = down ? 'mousedown' : 'mouseup';
					var propsb = { clientX: __mx, clientY: __my, button: btn, buttons: buttons };
					__dispatchMouse(canvas, t, propsb);
					__dispatchMouse(G, t, propsb);
					if (!down) {
						__dispatchMouse(canvas, 'click', propsb);
						__dispatchMouse(G, 'click', propsb);
					}
				}
			}
			__mb = buttons;
		}
	}
}

G.__trueos_pixi_ui_tick = function(angleRad) {
	var now = (Date.now && Date.now()) ? Date.now() : (last + 50);
	var dt = (last === 0) ? 50 : (now - last);
	last = now;
	if (G.__trueos_mouse_pump) {
		try { G.__trueos_mouse_pump(); } catch (e) {}
	} else {
		__pumpMouse();
	}
	if (G.__trueos_ui) {
		if (G.__trueos_ui.active < 0) G.__trueos_ui.start();
		G.__trueos_ui.tick(dt);
	}
	renderer.render(stage);
};
