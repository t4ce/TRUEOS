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
		txtMain = new PIXI.Text({
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
		txtLive = new PIXI.Text({
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
		return;
	}
	renderer.render({ container: stage });
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
		if (txtLive) {
			txtLive.text = 'ticks=' + String(txtTick);
		}
		if (txtMain && (txtTick % 40) === 0) {
			txtMain.text = 'TRUEOS Text Smoke ' + String((txtTick / 40) | 0);
		}
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
	if (!G.__trueos_gl_trace_installed) {
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
