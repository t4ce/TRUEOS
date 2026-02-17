export interface NativeCmdStream {
  beginFrame(): void;
  endFrame(): void;
  setClearRgb(rgb: number): void;
  setViewport(w: number, h: number): void;
  setBlendEnabled(enabled: boolean): void;
  setBlendFunc(srcRgb: number, dstRgb: number, srcAlpha: number, dstAlpha: number): void;
  setBlendEquation(rgb: number, alpha: number): void;
  drawTrianglesU8(bytes: Uint8Array): void;
}

export interface HexagonFrameOptions {
  width: number;
  height: number;
  cx?: number;
  cy?: number;
  radius: number;
  angleRad?: number;
  clearRgb?: number;
  fillRgb?: number;
}

export async function loadNativeCmdStream(): Promise<NativeCmdStream | null> {
  try {
    const mod = await import("cmd_stream");
    return mod as NativeCmdStream;
  } catch {
    return null;
  }
}

export class DirectCmdUiRenderer {
  constructor(private readonly native: NativeCmdStream) {}

  beginFrame(clearRgb: number): void {
    this.native.setClearRgb(clearRgb >>> 0);
    this.native.beginFrame();
  }

  setViewport(width: number, height: number): void {
    this.native.setViewport(width | 0, height | 0);
  }

  drawTrianglesPacked(vertices: Uint8Array): void {
    if (!vertices || vertices.length === 0) return;
    this.native.drawTrianglesU8(vertices);
  }

  endFrame(): void {
    this.native.endFrame();
  }
}

function writeVertex(
  out: Uint8Array,
  offset: number,
  x: number,
  y: number,
  rgb: number,
): number {
  const dv = new DataView(out.buffer, out.byteOffset, out.byteLength);
  dv.setFloat32(offset, x, true);
  dv.setFloat32(offset + 4, y, true);
  out[offset + 8] = (rgb >>> 16) & 0xff;
  out[offset + 9] = (rgb >>> 8) & 0xff;
  out[offset + 10] = rgb & 0xff;
  out[offset + 11] = 0;
  return offset + 12;
}

export function buildHexagonTriangleBytes(options: HexagonFrameOptions): Uint8Array {
  const cx = options.cx ?? options.width * 0.5;
  const cy = options.cy ?? options.height * 0.5;
  const angle0 = options.angleRad ?? 0;
  const color = (options.fillRgb ?? 0x3ddc97) >>> 0;

  const pts = new Array<{ x: number; y: number }>(6);
  for (let i = 0; i < 6; i += 1) {
    const a = angle0 + (i * Math.PI) / 3;
    pts[i] = {
      x: cx + Math.cos(a) * options.radius,
      y: cy + Math.sin(a) * options.radius,
    };
  }

  const out = new Uint8Array(6 * 3 * 12);
  let off = 0;
  for (let i = 0; i < 6; i += 1) {
    const p0 = pts[i];
    const p1 = pts[(i + 1) % 6];
    off = writeVertex(out, off, cx, cy, color);
    off = writeVertex(out, off, p0.x, p0.y, color);
    off = writeVertex(out, off, p1.x, p1.y, color);
  }
  return out;
}

export function renderHexagonFrame(renderer: DirectCmdUiRenderer, options: HexagonFrameOptions): void {
  renderer.setViewport(options.width, options.height);
  renderer.beginFrame((options.clearRgb ?? 0x081830) >>> 0);
  renderer.drawTrianglesPacked(buildHexagonTriangleBytes(options));
  renderer.endFrame();
}
