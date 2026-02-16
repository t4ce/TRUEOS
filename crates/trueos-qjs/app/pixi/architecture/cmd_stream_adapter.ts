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

  drawTrianglesPacked(vertices: Uint8Array): void {
    if (!vertices || vertices.length === 0) return;
    this.native.drawTrianglesU8(vertices);
  }

  endFrame(): void {
    this.native.endFrame();
  }
}
