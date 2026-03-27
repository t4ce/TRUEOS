import * as opentypeNs from "../vendor/opentype.mjs";

const opentype = opentypeNs.default ?? opentypeNs;
const INJECTED_FONT_BYTES_KEY = "__trueosOpentDemoFontBytes";
const DEMO_TEXT = "RTO";
const DEMO_FONT_SIZE = 64;
const DEMO_PADDING = 8;

function fail(message) {
  throw new Error(`opent.mjs: ${message}`);
}

function injectedFontBytes() {
  const injected = globalThis[INJECTED_FONT_BYTES_KEY];
  if (injected instanceof ArrayBuffer) {
    return injected.slice(0);
  }
  if (injected instanceof Uint8Array) {
    return injected.buffer.slice(
      injected.byteOffset,
      injected.byteOffset + injected.byteLength,
    );
  }
  fail(`missing injected font bytes on globalThis.${INJECTED_FONT_BYTES_KEY}`);
}

function makeRgba(width, height) {
  const rgba = new Uint8Array(width * height * 4);
  for (let i = 0; i < rgba.length; i += 4) {
    rgba[i] = 255;
    rgba[i + 1] = 255;
    rgba[i + 2] = 255;
    rgba[i + 3] = 255;
  }
  return rgba;
}

function flattenQuadratic(out, from, ctrl, to, segments) {
  for (let step = 1; step <= segments; step += 1) {
    const t = step / segments;
    const mt = 1 - t;
    out.push({
      x: mt * mt * from.x + 2 * mt * t * ctrl.x + t * t * to.x,
      y: mt * mt * from.y + 2 * mt * t * ctrl.y + t * t * to.y,
    });
  }
}

function flattenCubic(out, from, ctrl1, ctrl2, to, segments) {
  for (let step = 1; step <= segments; step += 1) {
    const t = step / segments;
    const mt = 1 - t;
    out.push({
      x:
        mt * mt * mt * from.x +
        3 * mt * mt * t * ctrl1.x +
        3 * mt * t * t * ctrl2.x +
        t * t * t * to.x,
      y:
        mt * mt * mt * from.y +
        3 * mt * mt * t * ctrl1.y +
        3 * mt * t * t * ctrl2.y +
        t * t * t * to.y,
    });
  }
}

function pathToContours(path) {
  const contours = [];
  let contour = [];
  let current = null;
  let start = null;

  for (const cmd of path.commands ?? []) {
    switch (cmd.type) {
      case "M":
        if (contour.length >= 3) {
          contours.push(contour);
        }
        contour = [{ x: cmd.x, y: cmd.y }];
        current = { x: cmd.x, y: cmd.y };
        start = { x: cmd.x, y: cmd.y };
        break;
      case "L":
        if (!current) {
          break;
        }
        contour.push({ x: cmd.x, y: cmd.y });
        current = { x: cmd.x, y: cmd.y };
        break;
      case "Q":
        if (!current) {
          break;
        }
        flattenQuadratic(
          contour,
          current,
          { x: cmd.x1, y: cmd.y1 },
          { x: cmd.x, y: cmd.y },
          12,
        );
        current = { x: cmd.x, y: cmd.y };
        break;
      case "C":
        if (!current) {
          break;
        }
        flattenCubic(
          contour,
          current,
          { x: cmd.x1, y: cmd.y1 },
          { x: cmd.x2, y: cmd.y2 },
          { x: cmd.x, y: cmd.y },
          16,
        );
        current = { x: cmd.x, y: cmd.y };
        break;
      case "Z":
        if (start && contour.length > 0) {
          const last = contour[contour.length - 1];
          if (last.x !== start.x || last.y !== start.y) {
            contour.push({ x: start.x, y: start.y });
          }
        }
        current = start ? { x: start.x, y: start.y } : current;
        break;
      default:
        break;
    }
  }

  if (contour.length >= 3) {
    contours.push(contour);
  }
  return contours;
}

function pointInContour(x, y, contour) {
  let inside = false;
  for (let i = 0, j = contour.length - 1; i < contour.length; j = i, i += 1) {
    const a = contour[i];
    const b = contour[j];
    const edgeCrosses = (a.y > y) !== (b.y > y);
    const edgeX = ((b.x - a.x) * (y - a.y)) / ((b.y - a.y) || 1e-6) + a.x;
    if (edgeCrosses && x < edgeX) {
      inside = !inside;
    }
  }
  return inside;
}

function drawFilledContours(width, height, contours) {
  const rgba = makeRgba(width, height);
  for (let y = 0; y < height; y += 1) {
    const py = y + 0.5;
    for (let x = 0; x < width; x += 1) {
      const px = x + 0.5;
      let inside = false;
      for (const contour of contours) {
        if (pointInContour(px, py, contour)) {
          inside = !inside;
        }
      }
      if (inside) {
        const base = (y * width + x) * 4;
        rgba[base] = 0;
        rgba[base + 1] = 0;
        rgba[base + 2] = 0;
        rgba[base + 3] = 255;
      }
    }
  }
  return rgba;
}

function buildTextImage(font, text) {
  const options = {
    kerning: true,
    hinting: false,
  };
  const initialPath = font.getPath(
    text,
    DEMO_PADDING,
    DEMO_FONT_SIZE,
    DEMO_FONT_SIZE,
    options,
  );
  const initialBox = initialPath.getBoundingBox();
  const originX = DEMO_PADDING - Math.floor(initialBox.x1 || 0);
  const originY = DEMO_PADDING - Math.floor(initialBox.y1 || 0);
  const path = font.getPath(text, originX, originY, DEMO_FONT_SIZE, options);
  const box = path.getBoundingBox();
  const width = Math.max(32, Math.ceil((box.x2 || 0) + DEMO_PADDING));
  const height = Math.max(32, Math.ceil((box.y2 || 0) + DEMO_PADDING));
  const contours = pathToContours(path);
  return {
    width,
    height,
    rgba: drawFilledContours(width, height, contours),
    commandCount: path.commands?.length ?? 0,
    contourCount: contours.length,
  };
}

async function renderTextDemoAsync() {
  const bytes = injectedFontBytes();
  if (!(bytes instanceof ArrayBuffer) || bytes.byteLength === 0) {
    fail("injected font bytes are empty");
  }
  if (!opentype || typeof opentype.parse !== "function") {
    fail("opentype.parse is not available");
  }
  const font = opentype.parse(bytes);
  const image = buildTextImage(font, DEMO_TEXT);
  return {
    width: image.width,
    height: image.height,
    rgba: image.rgba,
    text: DEMO_TEXT,
    fontBytes: bytes.byteLength,
    unitsPerEm: font.unitsPerEm ?? null,
    glyphCount: font.glyphs?.length ?? null,
    commandCount: image.commandCount,
    contourCount: image.contourCount,
  };
}

export { renderTextDemoAsync };
