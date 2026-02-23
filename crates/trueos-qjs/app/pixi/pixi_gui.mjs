var G = (typeof globalThis !== 'undefined') ? globalThis : this;

var W = Number((G.window && G.window.innerWidth) || 0);
var H = Number((G.window && G.window.innerHeight) || 0);
if (!isFinite(W) || W < 320) W = 1280;
if (!isFinite(H) || H < 240) H = 800;

if (G && G.console && typeof G.console.log === 'function') {
	try { G.console.log('pixi_gui: sprite demo W/H ' + String(W) + ' ' + String(H)); } catch (e) {}
}

var canvas = (G.document && G.document.createElement) ? G.document.createElement('canvas') : null;
if (!canvas) throw new Error('pixi_gui: no canvas available');
canvas.width = W;
canvas.height = H;
G.__trueos_canvas = canvas;

import * as PIXI from '/qjs/vendor/pixi.mjs';

// NOTE: Do not use PIXI.autoDetectRenderer() in TRUEOS; it probes contexts via
// extra canvases and can fail if our shims are incomplete.
var __Renderer = (PIXI && typeof PIXI.Renderer === 'function')
	? PIXI.Renderer
	: (PIXI && PIXI.default && typeof PIXI.default.Renderer === 'function')
		? PIXI.default.Renderer
		: null;
if (!__Renderer) throw new Error('pixi_gui: PIXI.Renderer not found');

var renderer = new __Renderer({
	view: canvas,
	width: W,
	height: H,
	backgroundColor: 0x101215,
	antialias: false,
});

var stage = new PIXI.Container();

function makeTriangleTexture() {
	// Build a small RGBA triangle texture so we stay on the Sprite path.
	var tw = 64;
	var th = 64;
	var buf = new Uint8Array(tw * th * 4);
	var cx = (tw / 2) | 0;
	var topY = 6;
	var baseY = th - 6;
	var halfBase = 24;
	for (var y = topY; y <= baseY; y++) {
		var t = (y - topY) / Math.max(1, (baseY - topY));
		var xMin = (cx - (halfBase * t)) | 0;
		var xMax = (cx + (halfBase * t)) | 0;
		if (xMin < 0) xMin = 0;
		if (xMax >= tw) xMax = tw - 1;
		for (var x = xMin; x <= xMax; x++) {
			var i = (y * tw + x) * 4;
			buf[i + 0] = 0xFF;
			buf[i + 1] = 0xFF;
			buf[i + 2] = 0xFF;
			buf[i + 3] = 0xFF;
		}
	}
	var base = (PIXI.BaseTexture && PIXI.BaseTexture.fromBuffer)
		? PIXI.BaseTexture.fromBuffer(buf, tw, th)
		: null;
	if (!base) throw new Error('pixi_gui: BaseTexture.fromBuffer missing');
	if (typeof base.scaleMode !== 'undefined' && PIXI.SCALE_MODES) {
		base.scaleMode = PIXI.SCALE_MODES.NEAREST;
	}
	return new PIXI.Texture(base);
}

var triTex = makeTriangleTexture();

// Root hierarchy: top-right anchor node.
var root = new PIXI.Container();
root.x = W;
root.y = 0;
stage.addChild(root);

// Tiny top-right triangle, about 50x50.
var tri = new PIXI.Sprite(triTex);
if (tri.anchor && tri.anchor.set) {
	tri.anchor.set(1, 0);
} else {
	tri.pivot.x = triTex.width;
	tri.pivot.y = 0;
}
tri.x = -8;
tri.y = 8;
tri.width = 50;
tri.height = 50;
tri.tint = 0xFF3B30;
tri.alpha = 0.95;
root.addChild(tri);

// A child container that rotates as a group (tests hierarchy + rotation).
var orbit = new PIXI.Container();
orbit.x = -32;
orbit.y = 64;
root.addChild(orbit);

var s1 = new PIXI.Sprite(triTex);
var s2 = new PIXI.Sprite(triTex);
var s3 = new PIXI.Sprite(triTex);
if (s1.anchor && s1.anchor.set) {
	s1.anchor.set(0.5, 0.5);
	s2.anchor.set(0.5, 0.5);
	s3.anchor.set(0.5, 0.5);
}

s1.x = 0;
s1.y = 0;
s1.width = 36;
s1.height = 36;
s1.tint = 0x00D084;
s1.alpha = 0.70;

s2.x = -40;
s2.y = 12;
s2.width = 28;
s2.height = 28;
s2.tint = 0x4C6FFF;
s2.alpha = 0.55;

s3.x = 32;
s3.y = 22;
s3.width = 22;
s3.height = 22;
s3.tint = 0xFFD43B;
s3.alpha = 0.50;

orbit.addChild(s1);
orbit.addChild(s2);
orbit.addChild(s3);

if (G && G.console && typeof G.console.log === 'function') {
	try { G.console.log('pixi_gui: using Sprite+Container hierarchy'); } catch (e) {}
}

var last = 0;
G.__trueos_pixi_ui_tick = function(angleRad) {
	var now = (Date.now && Date.now()) ? Date.now() : (last + 50);
	var dt = (last === 0) ? 50 : (now - last);
	last = now;

	var a = Number(angleRad);
	if (!isFinite(a)) a = 0;
	tri.rotation = a;
	orbit.rotation = -a * 1.2;
	s1.rotation = a * 0.7;
	renderer.render(stage);
};

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
