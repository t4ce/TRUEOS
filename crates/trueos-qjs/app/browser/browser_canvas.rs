#![cfg(feature = "trueos")]

pub const SIMPLE_DOM_CANVAS_SHIM_JS: &[u8] = br#"
function __trueosInstallSimpleDomCanvas(G, __trueosMakeGpuCanvasContext) {
  // Basic atlas-like "large font" approximation used by the canvas text shim.
  // We intentionally ignore rich font/style settings and keep a stable metric model.
  function __trueosLargeFontTextWidth(text) {
    const s = String(text || '');
    if (s.length === 0) return 0;
    let w = 0;
    for (let i = 0; i < s.length; i++) {
      const ch = s.charCodeAt(i) | 0;
      if (ch === 32) { w += 6; continue; } // space
      if (ch === 9) { w += 24; continue; } // tab
      // Thin glyphs.
      if (ch === 33 || ch === 39 || ch === 44 || ch === 46 || ch === 58 || ch === 59 || ch === 73 || ch === 105 || ch === 108 || ch === 124) { w += 5; continue; }
      // Wide glyphs.
      if (ch === 35 || ch === 37 || ch === 38 || ch === 64 || ch === 77 || ch === 87 || ch === 109 || ch === 119) { w += 12; continue; }
      // Uppercase tends wider than lowercase in our atlas-like approximation.
      if (ch >= 65 && ch <= 90) { w += 10; continue; }
      // Digits.
      if (ch >= 48 && ch <= 57) { w += 9; continue; }
      w += 9;
    }
    return w;
  }

  function __trueosMakeSimpleCanvas2dContext(canvas) {
    const state = {
      canvas,
      font: '16px sans-serif',
      textBaseline: 'alphabetic',
      textAlign: 'left',
      direction: 'ltr',
      lineWidth: 1,
      lineJoin: 'miter',
      lineCap: 'butt',
      miterLimit: 10,
      fillStyle: '#000000',
      strokeStyle: '#000000',
      globalAlpha: 1,
      globalCompositeOperation: 'source-over',
      imageSmoothingEnabled: true,
      imageSmoothingQuality: 'low',
      shadowColor: 'rgba(0,0,0,0)',
      shadowBlur: 0,
      shadowOffsetX: 0,
      shadowOffsetY: 0,
      letterSpacing: '0px',
      textLetterSpacing: '0px',
    };

    const mkImageData = (w, h) => {
      const wi = Math.max(0, Number(w || 0) | 0);
      const hi = Math.max(0, Number(h || 0) | 0);
      return {
        width: wi,
        height: hi,
        data: new Uint8ClampedArray((wi * hi * 4) | 0),
      };
    };

    const mkGradient = () => ({ addColorStop() {} });
    const mkPattern = () => ({ setTransform() {} });

    return {
      canvas,
      get font() { return state.font; },
      set font(v) { state.font = String(v || '16px sans-serif'); },
      get textBaseline() { return state.textBaseline; },
      set textBaseline(v) { state.textBaseline = String(v || 'alphabetic'); },
      get textAlign() { return state.textAlign; },
      set textAlign(v) { state.textAlign = String(v || 'left'); },
      get direction() { return state.direction; },
      set direction(v) { state.direction = String(v || 'ltr'); },
      get lineWidth() { return state.lineWidth; },
      set lineWidth(v) { state.lineWidth = Number(v || 1); },
      get lineJoin() { return state.lineJoin; },
      set lineJoin(v) { state.lineJoin = String(v || 'miter'); },
      get lineCap() { return state.lineCap; },
      set lineCap(v) { state.lineCap = String(v || 'butt'); },
      get miterLimit() { return state.miterLimit; },
      set miterLimit(v) { state.miterLimit = Number(v || 10); },
      get fillStyle() { return state.fillStyle; },
      set fillStyle(v) { state.fillStyle = v; },
      get strokeStyle() { return state.strokeStyle; },
      set strokeStyle(v) { state.strokeStyle = v; },
      get globalAlpha() { return state.globalAlpha; },
      set globalAlpha(v) { state.globalAlpha = Number(v || 1); },
      get globalCompositeOperation() { return state.globalCompositeOperation; },
      set globalCompositeOperation(v) { state.globalCompositeOperation = String(v || 'source-over'); },
      get imageSmoothingEnabled() { return state.imageSmoothingEnabled; },
      set imageSmoothingEnabled(v) { state.imageSmoothingEnabled = !!v; },
      get imageSmoothingQuality() { return state.imageSmoothingQuality; },
      set imageSmoothingQuality(v) { state.imageSmoothingQuality = String(v || 'low'); },
      get shadowColor() { return state.shadowColor; },
      set shadowColor(v) { state.shadowColor = v; },
      get shadowBlur() { return state.shadowBlur; },
      set shadowBlur(v) { state.shadowBlur = Number(v || 0); },
      get shadowOffsetX() { return state.shadowOffsetX; },
      set shadowOffsetX(v) { state.shadowOffsetX = Number(v || 0); },
      get shadowOffsetY() { return state.shadowOffsetY; },
      set shadowOffsetY(v) { state.shadowOffsetY = Number(v || 0); },
      get letterSpacing() { return state.letterSpacing; },
      set letterSpacing(v) { state.letterSpacing = String(v || '0px'); },
      get textLetterSpacing() { return state.textLetterSpacing; },
      set textLetterSpacing(v) { state.textLetterSpacing = String(v || '0px'); },

      save() {},
      restore() {},
      resetTransform() {},
      setTransform() {},
      transform() {},
      scale() {},
      rotate() {},
      translate() {},

      clearRect() {},
      fillRect() {},
      strokeRect() {},
      beginPath() {},
      closePath() {},
      moveTo() {},
      lineTo() {},
      arc() {},
      rect() {},
      stroke() {},
      fill() {},
      clip() {},
      drawImage() {},

      createLinearGradient() { return mkGradient(); },
      createRadialGradient() { return mkGradient(); },
      createPattern() { return mkPattern(); },

      fillText() {},
      strokeText() {},
      measureText(text) {
        const width = __trueosLargeFontTextWidth(text);
        return {
          width,
          actualBoundingBoxLeft: 0,
          actualBoundingBoxRight: width,
          actualBoundingBoxAscent: 12,
          actualBoundingBoxDescent: 4,
        };
      },

      getImageData(x, y, w, h) {
        return mkImageData(w, h);
      },
      putImageData() {},
    };
  }

  function __trueosMakeWebGlProbeContext(canvas, isWebGl2) {
    return {
      canvas,
      MAX_TEXTURE_IMAGE_UNITS: 0x8872,
      FRAGMENT_SHADER: 0x8B30,
      HIGH_FLOAT: 0x8DF2,
      COMPILE_STATUS: 0x8B81,
      isContextLost() {
        return false;
      },
      getContextAttributes() {
        return {
          alpha: true,
          antialias: false,
          depth: true,
          stencil: true,
          premultipliedAlpha: true,
          preserveDrawingBuffer: false,
        };
      },
      getParameter(p) {
        if ((Number(p) | 0) === this.MAX_TEXTURE_IMAGE_UNITS) return 16;
        return 0;
      },
      getShaderPrecisionFormat() {
        return { rangeMin: 127, rangeMax: 127, precision: 23 };
      },
      createShader(_type) {
        return { __kind: isWebGl2 ? 'webgl2-probe-shader' : 'webgl-probe-shader' };
      },
      shaderSource() {},
      compileShader() {},
      getShaderParameter(_shader, pname) {
        return (Number(pname) | 0) === this.COMPILE_STATUS;
      },
      deleteShader() {},
      getExtension(name) {
        const n = String(name || '').toUpperCase();
        if (n === 'WEBGL_LOSE_CONTEXT') {
          return { loseContext() {}, restoreContext() {} };
        }
        return null;
      },
    };
  }

  if (typeof G.__trueosMakeCanvas2dContext !== 'function') {
    G.__trueosMakeCanvas2dContext = (canvas) => __trueosMakeSimpleCanvas2dContext(canvas);
  }

  const mkNode = () => ({
    style: {},
    children: [],
    parentNode: null,
    ownerDocument: null,
    eventMode: 'none',
    appendChild(ch) { this.children.push(ch); ch.parentNode = this; return ch; },
    removeChild(ch) { this.children = this.children.filter((x) => x !== ch); ch.parentNode = null; return ch; },
    addEventListener() {},
    removeEventListener() {},
    dispatchEvent() { return true; },
    setAttribute() {},
    getAttribute() { return null; },
    contains(node) { if (node === this) return true; for (const c of this.children) { if (c && typeof c.contains === 'function' && c.contains(node)) return true; } return false; },
    getBoundingClientRect() { return { x: 0, y: 0, left: 0, top: 0, width: this.width || 0, height: this.height || 0 }; },
  });

  if (G.document) return;

  const doc = mkNode();
  doc.ownerDocument = doc;
  doc.documentElement = mkNode();
  doc.head = mkNode();
  doc.body = mkNode();
  doc.documentElement.ownerDocument = doc;
  doc.head.ownerDocument = doc;
  doc.body.ownerDocument = doc;
  doc.documentElement.appendChild(doc.head);
  doc.documentElement.appendChild(doc.body);

  const mkCanvas = () => {
    const c = mkNode();
    c.tagName = 'CANVAS';
    c.width = G.window.innerWidth | 0;
    c.height = G.window.innerHeight | 0;
    let gpuCtx = null;
    let webglProbeCtx = null;
    let webgl2ProbeCtx = null;
    c.getContext = (kind) => {
      const k = String(kind || '').toLowerCase();
      if (k === 'webgpu') {
        if (!gpuCtx) gpuCtx = __trueosMakeGpuCanvasContext(c);
        return gpuCtx;
      }
      if (k === 'webgl2') {
        if (!webgl2ProbeCtx) webgl2ProbeCtx = __trueosMakeWebGlProbeContext(c, true);
        return webgl2ProbeCtx;
      }
      if (k === 'webgl' || k === 'experimental-webgl') {
        if (!webglProbeCtx) webglProbeCtx = __trueosMakeWebGlProbeContext(c, false);
        return webglProbeCtx;
      }
      if (k !== '2d') return null;
      if (typeof G.__trueosMakeCanvas2dContext === 'function') {
        return G.__trueosMakeCanvas2dContext(c);
      }
      return __trueosMakeSimpleCanvas2dContext(c);
    };
    return c;
  };

  doc.createElement = (tag) => {
    const t = String(tag || '').toLowerCase();
    const n = t === 'canvas' ? mkCanvas() : mkNode();
    n.tagName = String(tag || '').toUpperCase();
    n.ownerDocument = doc;
    return n;
  };
  doc.getElementById = () => null;
  doc.addEventListener = () => {};
  doc.removeEventListener = () => {};
  doc.dispatchEvent = () => true;
  G.document = doc;
}
"#;