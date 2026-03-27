const INJECTED_FONT_BYTES_KEY = "__trueosOpentDemoFontBytes";

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

function makeParseOkImage() {
  const width = 48;
  const height = 48;
  const rgba = new Uint8Array(width * height * 4);
  rgba.fill(255);

  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      const base = (y * width + x) * 4;
      const border = x < 3 || y < 3 || x >= width - 3 || y >= height - 3;
      const diag = x === y || x + y === width - 1;
      if (border || diag) {
        rgba[base] = 0;
        rgba[base + 1] = 0;
        rgba[base + 2] = 0;
        rgba[base + 3] = 255;
      }
    }
  }

  return { width, height, rgba };
}

async function renderTextDemoAsync() {
  const bytes = injectedFontBytes();
  if (!(bytes instanceof ArrayBuffer) || bytes.byteLength === 0) {
    fail("injected font bytes are empty");
  }
  const image = makeParseOkImage();
  return {
    width: image.width,
    height: image.height,
    rgba: image.rgba,
    fontBytes: bytes.byteLength,
  };
}

export { renderTextDemoAsync };
