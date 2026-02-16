export function createTriangleScene(PIXI) {
  // Triangle in pixel coords.
  const positions = new Float32Array([
    40, 40,
    280, 60,
    160, 170,
  ]);

  // Per-vertex RGBA (normalized u8): R, G, B corners.
  const colors = new Uint8Array([
    255, 0, 0, 255,
    0, 255, 0, 255,
    0, 0, 255, 255,
  ]);

  const indices = new Uint16Array([0, 1, 2]);

  const geometry = new PIXI.Geometry()
    .addAttribute('aVertexPosition', positions, 2, false, PIXI.TYPES.FLOAT)
    .addAttribute('aColor', colors, 4, true, PIXI.TYPES.UNSIGNED_BYTE)
    .addIndex(indices);

  const vs = `precision mediump float;
attribute vec2 aVertexPosition;
attribute vec4 aColor;
uniform mat3 translationMatrix;
uniform mat3 projectionMatrix;
varying vec4 vColor;
void main(){
    vColor = aColor;
    vec3 pos = projectionMatrix * translationMatrix * vec3(aVertexPosition, 1.0);
    gl_Position = vec4(pos.xy, 0.0, 1.0);
}`;

  const fs = `precision mediump float;
varying vec4 vColor;
void main(){
    gl_FragColor = vColor;
}`;

  const program = PIXI.Program.from(vs, fs);
  const shader = new PIXI.Shader(program, {});
  const mesh = new PIXI.Mesh(geometry, shader);
  mesh.position.set(160, 100);
  mesh.pivot.set(160, 100);

  const stage = new PIXI.Container();
  stage.addChild(mesh);
  return { stage, mesh };
}
