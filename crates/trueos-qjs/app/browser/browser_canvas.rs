#![cfg(feature = "trueos")]

pub const CANVAS_2D_SHIM_JS: &[u8] = br#"
(function () {
    const G = (typeof globalThis !== 'undefined') ? globalThis : this;
    if (typeof G.__trueosMakeCanvas2dContext === 'function') return;

    G.__trueosMakeCanvas2dContext = function __trueosMakeCanvas2dContext() {
        return {
            font: '16px sans-serif',
            textBaseline: 'alphabetic',
            textAlign: 'left',
            lineWidth: 1,
            fillStyle: '#000000',
            strokeStyle: '#000000',
            shadowColor: '#000000',
            shadowBlur: 0,
            shadowOffsetX: 0,
            shadowOffsetY: 0,
            letterSpacing: '0px',
            textLetterSpacing: '0px',
            measureText: (s) => ({
                width: (String(s).length || 0) * 8,
                actualBoundingBoxLeft: 0,
                actualBoundingBoxRight: (String(s).length || 0) * 8,
                actualBoundingBoxAscent: 12,
                actualBoundingBoxDescent: 4,
            }),
            clearRect() {},
            fillRect() {},
            beginPath() {},
            moveTo() {},
            lineTo() {},
            stroke() {},
            fill() {},
            save() {},
            restore() {},
            resetTransform() {},
            setTransform() {},
            transform() {},
            scale() {},
            translate() {},
            rotate() {},
            fillText() {},
            strokeText() {},
            drawImage() {},
            createPattern() { return null; },
            createLinearGradient() {
                return { addColorStop() {} };
            },
            getImageData(_x, _y, w, h) {
                const iw = Math.max(0, Number(w) | 0);
                const ih = Math.max(0, Number(h) | 0);
                return {
                    width: iw,
                    height: ih,
                    data: new Uint8ClampedArray(iw * ih * 4),
                };
            },
        };
    };
})();
"#;
