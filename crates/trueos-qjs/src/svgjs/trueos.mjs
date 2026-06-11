import * as cmdStream from 'trueos:cmd_stream';
import { SVG } from './index.mjs';

function utf8Bytes(text) {
  const value = String(text ?? '');
  const out = [];
  for (let i = 0; i < value.length; i += 1) {
    let code = value.charCodeAt(i);
    if (code >= 0xd800 && code <= 0xdbff && i + 1 < value.length) {
      const next = value.charCodeAt(i + 1);
      if (next >= 0xdc00 && next <= 0xdfff) {
        code = 0x10000 + ((code - 0xd800) << 10) + (next - 0xdc00);
        i += 1;
      }
    }
    if (code <= 0x7f) {
      out.push(code);
    } else if (code <= 0x7ff) {
      out.push(0xc0 | (code >> 6), 0x80 | (code & 0x3f));
    } else if (code <= 0xffff) {
      out.push(0xe0 | (code >> 12), 0x80 | ((code >> 6) & 0x3f), 0x80 | (code & 0x3f));
    } else {
      out.push(
        0xf0 | (code >> 18),
        0x80 | ((code >> 12) & 0x3f),
        0x80 | ((code >> 6) & 0x3f),
        0x80 | (code & 0x3f),
      );
    }
  }
  return new Uint8Array(out);
}

export function svgToBytes(draw) {
  const text = typeof draw === 'string' ? draw : draw.svg(true);
  return utf8Bytes(text);
}

export function createTextureSvg(draw) {
  return cmdStream.createTextureSvg(svgToBytes(draw));
}

export function createTextureSvgAsync(draw) {
  return cmdStream.createTextureSvgAsync(svgToBytes(draw));
}

export function updateTextureSvg(texId, draw) {
  return cmdStream.updateTextureSvg(texId, svgToBytes(draw));
}

export function makeKernelSmokeSvg() {
  const draw = SVG().size(192, 128).viewbox(0, 0, 192, 128);
  draw.rect(192, 128).fill('#101820');
  draw.circle(58).center(58, 64).fill('#4dd0e1').opacity(0.82);
  draw.path('M104 34 L156 64 L104 94 Z').fill('#f6c85f');
  draw.rect(154, 18).move(20, 102).rx(4).fill('#ffffff').opacity(0.22);
  return draw;
}

export function createKernelSmokeTexture(asyncUpload = true) {
  const draw = makeKernelSmokeSvg();
  return asyncUpload ? createTextureSvgAsync(draw) : createTextureSvg(draw);
}

export { SVG };
