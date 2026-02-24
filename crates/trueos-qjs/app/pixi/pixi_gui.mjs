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

// --- TRUEOS text shim: route PIXI.Text to a mesh-only bitmap path ---
// This avoids Canvas/Text raster plumbing and keeps text on the proven mesh+texture route.
var __TRUEOS_BITMAP_5x7 = {
	' ': [0,0,0,0,0,0,0],
	'0': [14,17,19,21,25,17,14],
	'1': [4,12,4,4,4,4,14],
	'2': [14,17,1,2,4,8,31],
	'3': [30,1,1,14,1,1,30],
	'4': [2,6,10,18,31,2,2],
	'5': [31,16,16,30,1,1,30],
	'6': [14,16,16,30,17,17,14],
	'7': [31,1,2,4,8,8,8],
	'8': [14,17,17,14,17,17,14],
	'9': [14,17,17,15,1,1,14],
	'A': [14,17,17,31,17,17,17],
	'B': [30,17,17,30,17,17,30],
	'C': [14,17,16,16,16,17,14],
	'D': [30,17,17,17,17,17,30],
	'E': [31,16,16,30,16,16,31],
	'F': [31,16,16,30,16,16,16],
	'G': [14,17,16,23,17,17,14],
	'H': [17,17,17,31,17,17,17],
	'I': [14,4,4,4,4,4,14],
	'J': [1,1,1,1,17,17,14],
	'K': [17,18,20,24,20,18,17],
	'L': [16,16,16,16,16,16,31],
	'M': [17,27,21,21,17,17,17],
	'N': [17,25,21,19,17,17,17],
	'O': [14,17,17,17,17,17,14],
	'P': [30,17,17,30,16,16,16],
	'Q': [14,17,17,17,21,18,13],
	'R': [30,17,17,30,20,18,17],
	'S': [14,17,16,14,1,17,14],
	'T': [31,4,4,4,4,4,4],
	'U': [17,17,17,17,17,17,14],
	'V': [17,17,17,17,17,10,4],
	'W': [17,17,17,21,21,21,10],
	'X': [17,17,10,4,10,17,17],
	'Y': [17,17,10,4,4,4,4],
	'Z': [31,1,2,4,8,16,31],
	'.': [0,0,0,0,0,12,12],
	'-': [0,0,0,31,0,0,0],
	':': [0,12,12,0,12,12,0]
};

function __trueosTextStyle(style) {
	var s = style || {};
	var fill = Number(s.fill);
	if (!isFinite(fill)) fill = 0x102A43;
	var fontSize = Number(s.fontSize);
	if (!isFinite(fontSize) || fontSize <= 0) fontSize = 18;
	var alpha = Number(s.alpha);
	if (!isFinite(alpha) || alpha < 0) alpha = 1.0;
	if (alpha > 1) alpha = 1.0;
	return { fill: fill >>> 0, fontSize: fontSize, alpha: alpha };
}

function __buildBitmapMesh(text, style) {
	var t = String(text == null ? '' : text);
	var st = __trueosTextStyle(style);
	var scale = Math.max(1, (st.fontSize / 8) | 0);
	var glyphW = 5 * scale;
	var glyphH = 7 * scale;
	var advance = glyphW + scale;

	var maxQuads = t.length * 5 * 7 * scale * scale;
	var pos = new Float32Array(maxQuads * 6 * 2);
	var uv = new Float32Array(maxQuads * 6 * 2);
	var p = 0;
	var u = 0;
	var xBase = 0;

	function pushQuad(x0, y0, x1, y1) {
		pos[p++] = x0; pos[p++] = y0;
		pos[p++] = x1; pos[p++] = y0;
		pos[p++] = x1; pos[p++] = y1;
		pos[p++] = x0; pos[p++] = y0;
		pos[p++] = x1; pos[p++] = y1;
		pos[p++] = x0; pos[p++] = y1;
		for (var i = 0; i < 6; i++) {
			uv[u++] = 0.5;
			uv[u++] = 0.5;
		}
	}

	for (var ci = 0; ci < t.length; ci++) {
		var ch = t.charAt(ci);
		var key = ch.toUpperCase();
		var rows = __TRUEOS_BITMAP_5x7[key] || __TRUEOS_BITMAP_5x7['?'] || __TRUEOS_BITMAP_5x7[' '];
		for (var ry = 0; ry < 7; ry++) {
			var bits = rows[ry] | 0;
			for (var rx = 0; rx < 5; rx++) {
				if ((bits & (1 << (4 - rx))) === 0) continue;
				var px = xBase + rx * scale;
				var py = ry * scale;
				// One quad per glyph pixel (scaled), not scale*scale quads.
				// This keeps the hot-loop vertex count bounded.
				pushQuad(px, py, px + scale, py + scale);
			}
		}
		xBase += advance;
	}

	var posUsed = pos.subarray(0, p);
	var uvUsed = uv.subarray(0, u);
	var geo = new PIXI.Geometry({
		attributes: {
			aPosition: posUsed,
			aUV: uvUsed,
		},
	});
	var mesh = new PIXI.Mesh({
		geometry: geo,
		texture: PIXI.Texture.WHITE,
	});
	mesh.tint = st.fill;
	mesh.alpha = st.alpha;
	return mesh;
}

function __createTrueosTextNode(arg) {
	var text = '';
	var style = {};
	if (typeof arg === 'string' || typeof arg === 'number') {
		text = String(arg);
	} else if (arg && typeof arg === 'object') {
		if (arg.text != null) text = String(arg.text);
		if (arg.style && typeof arg.style === 'object') style = arg.style;
	}

	var node = new PIXI.Container();
	node.__trueos_text = text;
	node.__trueos_style = style;
	node.__trueos_mesh = null;

	node.__rebuildText = function() {
		if (node.__trueos_mesh && typeof node.removeChild === 'function') {
			try { node.removeChild(node.__trueos_mesh); } catch (_e0) {}
			try { node.__trueos_mesh.destroy && node.__trueos_mesh.destroy(true); } catch (_e1) {}
		}
		node.__trueos_mesh = __buildBitmapMesh(node.__trueos_text, node.__trueos_style);
		node.addChild(node.__trueos_mesh);
	};

	Object.defineProperty(node, 'text', {
		get: function() { return node.__trueos_text; },
		set: function(v) {
			var next = String(v == null ? '' : v);
			if (next === node.__trueos_text) return;
			node.__trueos_text = next;
			node.__rebuildText();
		},
		enumerable: true,
		configurable: true,
	});

	Object.defineProperty(node, 'style', {
		get: function() { return node.__trueos_style; },
		set: function(v) {
			var next = (v && typeof v === 'object') ? v : {};
			if (next === node.__trueos_style) return;
			node.__trueos_style = next;
			node.__rebuildText();
		},
		enumerable: true,
		configurable: true,
	});

	node.__rebuildText();
	return node;
}

var __TRUEOS_TextFactory = function(arg) {
	return __createTrueosTextNode(arg);
};

function installTrueosTextShim() {
	if (!PIXI || typeof PIXI !== 'object') return;
	try {
		// If this succeeds, normal `new PIXI.Text(...)` call sites keep working.
		PIXI.Text = function TrueosTextShim(arg) {
			return __createTrueosTextNode(arg);
		};
		__TRUEOS_TextFactory = function(arg) { return new PIXI.Text(arg); };
	} catch (_e) {
		// Some Pixi bundles expose a non-extensible namespace object.
		// Keep using local factory without mutating PIXI exports.
		__TRUEOS_TextFactory = function(arg) {
			return __createTrueosTextNode(arg);
		};
	}
}

installTrueosTextShim();

function resolveRendererCtor() {
	if (PIXI && typeof PIXI.Renderer === 'function') return PIXI.Renderer;
	if (PIXI && typeof PIXI.WebGLRenderer === 'function') return PIXI.WebGLRenderer;
	if (PIXI && PIXI.default && typeof PIXI.default.Renderer === 'function') return PIXI.default.Renderer;
	if (PIXI && PIXI.default && typeof PIXI.default.WebGLRenderer === 'function') return PIXI.default.WebGLRenderer;
	return null;
}

var renderer = null;
var stage = null;

function setXY(node, x, y) {
	if (node && node.position && typeof node.position.set === 'function') {
		node.position.set(x, y);
		return;
	}
	if (node) {
		node.x = x;
		node.y = y;
	}
}

function setPivot(node, x, y) {
	if (node && node.pivot && typeof node.pivot.set === 'function') {
		node.pivot.set(x, y);
		return;
	}
	if (node && node.pivot) {
		node.pivot.x = x;
		node.pivot.y = y;
	}
}

function makeTriangleMesh(size, color, alpha) {
	var h = size * 0.5;
	var verts = new Float32Array([
		0, -h,
		-h, h,
		h, h,
	]);
	var uvs = new Float32Array([
		0.5, 0,
		0, 1,
		1, 1,
	]);
	var indices = new Uint16Array([0, 1, 2]);
	var geo = new PIXI.Geometry({
		attributes: {
			aPosition: verts,
			aUV: uvs,
		},
		indexBuffer: indices,
	});
	var mesh = new PIXI.Mesh({
		geometry: geo,
		texture: PIXI.Texture.WHITE,
	});
	mesh.tint = color;
	mesh.alpha = alpha;
	return mesh;
}
var tri = null;
var orbit = null;
var m1 = null;
var gtest = null;
var txtMain = null;
var txtLive = null;
var txtTick = 0;
var __render_mode = 2; // Pixi v8 path: render({ container })
var __render_failed = false;
var __contains_probe_done = false;

function logMsg(msg) {
	if (G && G.console && typeof G.console.log === 'function') {
		try { G.console.log(msg); } catch (e) {}
	}
}

function logErr(msg) {
	if (G && G.console && typeof G.console.error === 'function') {
		try { G.console.error(msg); } catch (e) {}
	}
}

function runSceneGraphSmoke() {
	var pass = 0;
	var fail = 0;
	function check(name, fn) {
		try {
			fn();
			pass++;
			logMsg('pixi_gui:test:PASS scene ' + name);
		} catch (e) {
			fail++;
			logErr('pixi_gui:test:FAIL scene ' + name + ' -> ' + String(e && e.message ? e.message : e));
		}
	}

	check('Container.addChild', function() {
		var c = new PIXI.Container();
		var a = new PIXI.Container();
		c.addChild(a);
		if (c.children.length !== 1) throw new Error('children len != 1');
	});

	check('Container.removeChildren', function() {
		var c = new PIXI.Container();
		c.addChild(new PIXI.Container());
		c.addChild(new PIXI.Container());
		var removed = c.removeChildren();
		if (!removed || removed.length !== 2) throw new Error('removed len != 2');
		if (c.children.length !== 0) throw new Error('children not empty');
	});

	check('stage add/remove', function() {
		var tmp = new PIXI.Container();
		stage.addChild(tmp);
		if (stage.children.indexOf(tmp) < 0) throw new Error('not added');
		stage.removeChild(tmp);
		if (stage.children.indexOf(tmp) >= 0) throw new Error('not removed');
	});

	logMsg('pixi_gui:test:scene done pass=' + String(pass) + ' fail=' + String(fail));
}

function runGraphicsSmoke(root) {
	var pass = 0;
	var fail = 0;
	function check(name, fn) {
		try {
			fn();
			pass++;
			logMsg('pixi_gui:test:PASS gfx ' + name);
		} catch (e) {
			fail++;
			logErr('pixi_gui:test:FAIL gfx ' + name + ' -> ' + String(e && e.message ? e.message : e));
		}
	}

	gtest = new PIXI.Graphics();
	root.addChild(gtest);

	check('rect', function() {
		gtest.rect(24, 24, 120, 56);
	});
	check('fill', function() {
		gtest.fill({ color: 0xFF6B6B, alpha: 1.0 });
	});
	check('stroke', function() {
		gtest.stroke({ color: 0x111111, width: 4, alpha: 1.0 });
	});
	check('circle', function() {
		gtest.circle(220, 56, 28);
		gtest.fill({ color: 0x4ECDC4, alpha: 0.95 });
	});
	check('moveTo/lineTo/closePath', function() {
		gtest.moveTo(280, 24);
		gtest.lineTo(360, 90);
		gtest.lineTo(300, 112);
		gtest.closePath();
		gtest.fill({ color: 0x1A535C, alpha: 0.95 });
	});
	check('clear', function() {
		gtest.clear();
		// redraw final visible state after clear
		gtest.rect(24, 24, 120, 56);
		gtest.fill({ color: 0xFF6B6B, alpha: 1.0 });
		gtest.stroke({ color: 0x111111, width: 4, alpha: 1.0 });
		gtest.circle(220, 56, 28);
		gtest.fill({ color: 0x4ECDC4, alpha: 0.95 });
		gtest.moveTo(280, 24);
		gtest.lineTo(360, 90);
		gtest.lineTo(300, 112);
		gtest.closePath();
		gtest.fill({ color: 0x1A535C, alpha: 0.95 });
	});

	logMsg('pixi_gui:test:gfx done pass=' + String(pass) + ' fail=' + String(fail));
}

function runTextSmoke(root) {
	if (!PIXI || typeof PIXI.Text !== 'function') {
		logMsg('pixi_gui:test:SKIP text ctor missing');
		return;
	}
	var pass = 0;
	var fail = 0;
	function check(name, fn) {
		try {
			fn();
			pass++;
			logMsg('pixi_gui:test:PASS text ' + name);
		} catch (e) {
			fail++;
			logErr('pixi_gui:test:FAIL text ' + name + ' -> ' + String(e && e.message ? e.message : e));
		}
	}

	check('new Text({ text, style })', function() {
		txtMain = __TRUEOS_TextFactory({
			text: 'TRUEOS Text Smoke',
			style: {
				fill: 0x102A43,
				fontSize: 26,
				fontWeight: '700',
			},
		});
		setXY(txtMain, 24, 128);
		root.addChild(txtMain);
	});

	check('secondary label', function() {
		txtLive = __TRUEOS_TextFactory({
			text: 'ticks=0',
			style: {
				fill: 0x334E68,
				fontSize: 18,
			},
		});
		setXY(txtLive, 24, 162);
		root.addChild(txtLive);
	});

	logMsg('pixi_gui:test:text done pass=' + String(pass) + ' fail=' + String(fail));
}

function runApplicationSmoke() {
	if (!PIXI || typeof PIXI.Application !== 'function') {
		logMsg('pixi_gui:test:SKIP app ctor missing');
		return Promise.resolve();
	}
	var app = new PIXI.Application();
	if (!app || typeof app.init !== 'function') {
		logMsg('pixi_gui:test:SKIP app.init missing');
		return Promise.resolve();
	}
	var c = (G.document && G.document.createElement) ? G.document.createElement('canvas') : null;
	if (!c) {
		logMsg('pixi_gui:test:SKIP app no canvas');
		return Promise.resolve();
	}
	c.width = 64;
	c.height = 64;
	return Promise.resolve(app.init({
		canvas: c,
		width: 64,
		height: 64,
		backgroundColor: 0xFFFFFF,
		background: 0xFFFFFF,
		antialias: false,
		preference: 'webgl',
	})).then(function() {
		logMsg('pixi_gui:test:PASS app.init');
	}).catch(function(e) {
		logErr('pixi_gui:test:FAIL app.init -> ' + String(e && e.message ? e.message : e));
	}).then(function() {
		try {
			if (app && typeof app.destroy === 'function') app.destroy(true);
		} catch (_e) {}
	});
}

function initScene() {
	// Root hierarchy: top-right anchor node.
	var root = new PIXI.Container();
	setXY(root, W, 0);
	stage.addChild(root);

	// Tiny top-right triangle, about 50x50.
	tri = makeTriangleMesh(50, 0xFF3B30, 0.95);
	setPivot(tri, 25, 0);
	setXY(tri, -8, 8);
	root.addChild(tri);

	// A child container that rotates as a group (tests hierarchy + rotation).
	orbit = new PIXI.Container();
	setXY(orbit, -32, 64);
	root.addChild(orbit);

	m1 = makeTriangleMesh(36, 0x00D084, 0.70);
	var m2 = makeTriangleMesh(28, 0x4C6FFF, 0.55);
	var m3 = makeTriangleMesh(22, 0xFFD43B, 0.50);

	setXY(m1, 0, 0);
	setXY(m2, -40, 12);
	setXY(m3, 32, 22);

	orbit.addChild(m1);
	orbit.addChild(m2);
	orbit.addChild(m3);

	runSceneGraphSmoke();
	runGraphicsSmoke(stage);
	runTextSmoke(stage);

	if (G && G.console && typeof G.console.log === 'function') {
		try { G.console.log('pixi_gui: using Mesh+Container hierarchy'); } catch (e) {}
	}
}

var last = 0;
function renderStage() {
	if (G.document && G.document.body && typeof G.document.body.contains !== 'function') {
		G.document.body.contains = function() { return false; };
	}
	if (!__contains_probe_done) {
		__contains_probe_done = true;
		try {
			var tBody = (G.document && G.document.body) ? typeof G.document.body : 'missing';
			var tContains = (G.document && G.document.body) ? typeof G.document.body.contains : 'missing';
			var tCanvasCtor = typeof G.HTMLCanvasElement;
			var containsResult = 'n/a';
			var instanceofResult = 'n/a';
			if (G.document && G.document.body && typeof G.document.body.contains === 'function') {
				try {
					containsResult = String(G.document.body.contains(canvas));
				} catch (eContains) {
					containsResult = 'err:' + String(eContains && eContains.message ? eContains.message : eContains);
				}
			}
			if (typeof G.HTMLCanvasElement === 'function') {
				try {
					instanceofResult = String(canvas instanceof G.HTMLCanvasElement);
				} catch (eInst) {
					instanceofResult = 'err:' + String(eInst && eInst.message ? eInst.message : eInst);
				}
			}
			if (G.console && typeof G.console.log === 'function') {
				G.console.log('pixi_gui: probe body=' + String(tBody) + ' contains=' + String(tContains) + ' HTMLCanvasElement=' + String(tCanvasCtor) + ' contains(canvas)=' + containsResult + ' inst=' + instanceofResult);
			}
		} catch (_probeErr) {}
	}
	if (!renderer || !stage || typeof renderer.render !== 'function') return;
	if (__render_mode === 2) {
		renderer.render({ container: stage });
		if (G.__trueos_gl_ctx && typeof G.__trueos_gl_ctx.flush === 'function') {
			try { G.__trueos_gl_ctx.flush(); } catch (_eFlush0) {}
		}
		return;
	}
	renderer.render({ container: stage });
	if (G.__trueos_gl_ctx && typeof G.__trueos_gl_ctx.flush === 'function') {
		try { G.__trueos_gl_ctx.flush(); } catch (_eFlush1) {}
	}
	__render_mode = 2;
}

G.__trueos_pixi_ui_tick = function(angleRad) {
	if (__render_failed) return;
	if (!renderer || !stage || !tri || !orbit || !m1) return;
	var now = (Date.now && Date.now()) ? Date.now() : (last + 50);
	last = now;

	var a = Number(angleRad);
	if (!isFinite(a)) a = 0;
	try {
		tri.rotation = a;
		orbit.rotation = -a * 1.2;
		m1.rotation = a * 0.7;
		txtTick++;
		// Keep text static in the hot loop to preserve bounded allocations.
		renderStage();
	} catch (e) {
		__render_failed = true;
		if (G && G.console && typeof G.console.error === 'function') {
			try {
				var msg = 'pixi_gui: render failed, stopping tick: ' + String(e && e.message ? e.message : e);
				if (e && e.stack) msg += '\n' + String(e.stack);
				G.console.error(msg);
			} catch (_e) {}
		}
		if (G.__trueos_pixi_ui_stop) G.__trueos_pixi_ui_stop();
	}
};

function startPixi() {
	var webglCtor = (PIXI && typeof PIXI.WebGLRenderer === 'function')
		? PIXI.WebGLRenderer
		: resolveRendererCtor();
	if (!webglCtor) throw new Error('pixi_gui: WebGL renderer ctor not found');
	if (!canvas.getContext) throw new Error('pixi_gui: canvas.getContext missing');
	var gl = canvas.getContext('webgl2', { alpha: false, antialias: false, stencil: true })
		|| canvas.getContext('webgl', { alpha: false, antialias: false, stencil: true });
	if (!gl) throw new Error('pixi_gui: WebGL context not available');
	G.__trueos_gl_ctx = gl;
	// Optional GL tracing (very expensive): set `globalThis.__trueos_pixi_gl_trace = true`.
	if (G.__trueos_pixi_gl_trace === true && !G.__trueos_gl_trace_installed) {
		G.__trueos_gl_trace_installed = true;
		G.__trueos_gl_calls = G.__trueos_gl_calls || Object.create(null);
		var names = [];
		try { names = Object.getOwnPropertyNames(gl); } catch (e) { names = []; }
		for (var i = 0; i < names.length; i++) {
			var k = names[i];
			var fn = gl[k];
			if (typeof fn !== 'function') continue;
			(function(name, orig) {
				var seen = false;
				try {
					gl[name] = function() {
						var c = (G.__trueos_gl_calls[name] || 0) + 1;
						G.__trueos_gl_calls[name] = c;
						if (!seen) {
							seen = true;
							if (G.console && typeof G.console.log === 'function') {
								try { G.console.log('pixi_gui: gl call ' + name); } catch (eLog) {}
							}
						}
						return orig.apply(gl, arguments);
					};
				} catch (eWrap) {}
			})(k, fn);
		}
	}
	var opts = {
		canvas: canvas,
		context: gl,
		width: W,
		height: H,
		backgroundColor: 0xFFFFFF,
		background: 0xFFFFFF,
		antialias: false,
		preference: 'webgl',
	};
	var r = new webglCtor();
	if (r && typeof r.init === 'function') {
		return Promise.resolve(r.init(opts)).then(function() {
			renderer = r;
			stage = new PIXI.Container();
			initScene();
			return runApplicationSmoke();
		});
	}
	renderer = new webglCtor(opts);
	stage = new PIXI.Container();
	initScene();
	return runApplicationSmoke();
}

Promise.resolve(startPixi()).catch(function(e) {
	throw new Error('pixi_gui: webgl bootstrap failed: ' + String(e && e.message ? e.message : e));
});

// Self-driven 60Hz loop (preferred). If timers are unavailable, Rust can still
// drive `__trueos_pixi_ui_tick` externally.
if (!G.__trueos_pixi_ui_running && typeof setInterval === 'function') {
	G.__trueos_pixi_ui_running = true;
	G.__trueos_pixi_ui_a = G.__trueos_pixi_ui_a || 0;
	G.__trueos_pixi_ui_timer = setInterval(function() {
		G.__trueos_pixi_ui_a = (G.__trueos_pixi_ui_a || 0) + 0.03;
		if (G.__trueos_pixi_ui_tick) G.__trueos_pixi_ui_tick(G.__trueos_pixi_ui_a);
	}, 16);
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
