import * as PIXI from '/qjs/vendor/pixi.mjs';

// Minimal Pixi scene for TRUEOS: one rotating triangle.
// This file intentionally avoids the previous windowing/text demo to keep the
// rendering path simple while we validate the WebGL/virgl pipeline.

var G = (typeof globalThis !== 'undefined') ? globalThis : this;

var W = Number((G.window && G.window.innerWidth) || 0);
var H = Number((G.window && G.window.innerHeight) || 0);
if (!isFinite(W) || W < 320) W = 1280;
if (!isFinite(H) || H < 240) H = 800;

if (G && G.console && typeof G.console.log === 'function') {
	try { G.console.log('pixi_gui: minimal triangle W/H ' + String(W) + ' ' + String(H)); } catch (e) {}
}

var canvas = (G.document && G.document.createElement) ? G.document.createElement('canvas') : null;
if (!canvas) throw new Error('pixi_gui: no canvas available');
canvas.width = W;
canvas.height = H;
G.__trueos_canvas = canvas;

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

function makeTriangleSprite() {
	// Use a Sprite path on purpose.
	//
	// Pixi's Graphics pipeline uses different shaders/state than the default Sprite
	// batcher, and our WebGL shim primarily targets the Sprite path.
	var tw = 128;
	var th = 128;
	var buf = new Uint8Array(tw * th * 4);
	var cx = (tw / 2) | 0;
	var topY = 12;
	var baseY = th - 12;
	var halfBase = 52;
	for (var y = topY; y <= baseY; y++) {
		var t = (y - topY) / Math.max(1, (baseY - topY));
		var xMin = (cx - (halfBase * t)) | 0;
		var xMax = (cx + (halfBase * t)) | 0;
		if (xMin < 0) xMin = 0;
		if (xMax >= tw) xMax = tw - 1;
		for (var x = xMin; x <= xMax; x++) {
			var i = (y * tw + x) * 4;
			buf[i + 0] = 0xFF;
			buf[i + 1] = 0x3B;
			buf[i + 2] = 0x30;
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
	var tex = new PIXI.Texture(base);
	var spr = new PIXI.Sprite(tex);
	if (typeof spr.anchor !== 'undefined' && spr.anchor && typeof spr.anchor.set === 'function') {
		spr.anchor.set(0.5, 0.5);
	} else {
		spr.pivot.x = (tw / 2) | 0;
		spr.pivot.y = (th / 2) | 0;
	}
	spr.width = 220;
	spr.height = 220;
	return spr;
}

var tri = makeTriangleSprite();
if (G && G.console && typeof G.console.log === 'function') {
	try { G.console.log('pixi_gui: using Sprite triangle path'); } catch (e) {}
}

tri.x = (W / 2) | 0;
tri.y = (H / 2) | 0;
stage.addChild(tri);

var last = 0;
G.__trueos_pixi_ui_tick = function(angleRad) {
	var now = (Date.now && Date.now()) ? Date.now() : (last + 50);
	var dt = (last === 0) ? 50 : (now - last);
	last = now;
	var a = Number(angleRad);
	if (!isFinite(a)) {
		a = (dt / 1000.0) * 0.7;
	}
	tri.rotation = a;
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
