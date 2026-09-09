const fs = require('fs');
const vm = require('vm');
const path = require('path');

const html = fs.readFileSync(path.join(__dirname, '..', 'Cube', 'AssetShowcase.html'), 'utf8');
const start = html.indexOf('function mulberry32(a)');
const end = html.indexOf('function build(){', start);
if (start < 0 || end < 0) throw new Error('showcase builder section not found');

class Object3D {
  constructor() { this.children = []; this.position = { set() {} }; this.scale = { setScalar() {} }; }
  add(x) { this.children.push(x); }
  remove(x) { this.children = this.children.filter(y => y !== x); }
}
class Color {
  constructor(value) { this.value = value >>> 0; }
  lerp(other, amount) {
    const c = n => [(n >> 16) & 255, (n >> 8) & 255, n & 255];
    const a = c(this.value), b = c(other.value);
    this.value = (Math.round(a[0] + (b[0] - a[0]) * amount) << 16) |
      (Math.round(a[1] + (b[1] - a[1]) * amount) << 8) |
      Math.round(a[2] + (b[2] - a[2]) * amount);
    return this;
  }
  getHex() { return this.value >>> 0; }
}
const THREE = {
  Mesh: class extends Object3D { constructor() { super(); this.material = { dispose() {} }; } },
  MeshStandardMaterial: class { dispose() {} },
  InstancedMesh: class extends Object3D { constructor() { super(); this.instanceMatrix = { needsUpdate: false }; } },
  Vector3: class { constructor() {} },
  Quaternion: class {}, Matrix4: class {}, Color
};
const treeGroup = new Object3D();
const document = { getElementById: id => id === 'portalMix' ? { value: '1,3' } : { value: 'pine' } };
const window = { dispatchEvent() {} };
const sandbox = { THREE, treeGroup, document, window, console, atob: s => Buffer.from(s, 'base64').toString('latin1') };
vm.createContext(sandbox);
const prelude = `
const GAP=0.01, GRID_UNIT=0.20, GAP_WORLD=GRID_UNIT*GAP, MAX_CUBE_STEP=4, STEM_TIER=2;
const PART_IDS=new Map([
 ['cube',0],['trunk',1],['branch',2],['foliage',3],['foliageSmall',3],['crown',3],['sideCrown',3],['tip',3],['upperFoliage',3],['lowerFoliage',3],
 ['rock',4],['ore',5],['bush',6],['berry',7],['grass',8],['grassBase',8],['portal',9],['portalAccent',10],['chest',11],['chestTrim',12],['gem',13],
 ['veggie',14],['veggieLeaf',15],['toolHandle',16],['toolHead',17],['toolDetail',18],['packedLite',27]
]);
let cubeGeometry={}, occupied=new Set(), placed=[], seed=1;
`;
const postlude = `
globalThis.api={
  builders:{pine,broad,orchard,tall,stonePile,ironPile,copperPile,goldPile,roundBush,berryBush,grassTuft,wildBush,compactBush,lowMoundBush,forkedBush,smallBerryBush,travelerChest,gemChest,ironboundChest,royalChest,pickaxe,hatchet,hammer,spade,hoe,rake,sickle,wateringCan,chilliPepper,banana,carrot,eggplant,cornCob,pumpkin,radish,cucumber,dynamicPortalStanding,dynamicPortalCircle,skyPortal,undergroundPortal,blackHolePortal,whiteHolePortal,islandPortal,cityPortal,voidPortal,leavePortal},
  reset(){ occupied.clear(); placed=[]; treeGroup.children=[]; seed=1; rnd=mulberry32(seed); },
  records(){ return placed; },
  encode(records){
    const colors=[], indices=new Map();
    for(const q of records){ const k=q.color>>>0; if(!indices.has(k)){indices.set(k,colors.length);colors.push(k);} }
    if(colors.length>255 || records.length>65535) throw new Error('CUBES v1 limits exceeded');
    const out=new ArrayBuffer(16+colors.length*4+records.length*8), dv=new DataView(out); let o=0;
    for(const c of [67,85,66,69]) dv.setUint8(o++,c); dv.setUint8(o++,1); dv.setUint8(o++,0); dv.setUint8(o++,1); dv.setUint8(o++,8);
    dv.setUint16(o,records.length,true); o+=2; dv.setUint8(o++,colors.length); dv.setUint8(o++,4); dv.setFloat32(o,0.20,true); o+=4;
    for(const c of colors){dv.setUint8(o++,(c>>16)&255);dv.setUint8(o++,(c>>8)&255);dv.setUint8(o++,c&255);dv.setUint8(o++,255);}
    for(const q of records){dv.setInt8(o++,q.gx);dv.setInt8(o++,q.gy);dv.setInt8(o++,q.gz);dv.setUint8(o++,q.tier);dv.setUint8(o++,indices.get(q.color>>>0));dv.setUint8(o++,PART_IDS.get(q.kind)??0);dv.setUint8(o++,0);dv.setUint8(o++,0);}
    return Buffer.from(out);
  }
};`;
vm.runInContext(prelude + fs.readFileSync(path.join(__dirname, '..', 'Cube', 'AssetShowcase.html'), 'utf8').slice(start, end) + postlude, sandbox);

const presets = [
  'pine','broad','orchard','tall','stonePile','ironPile','copperPile','goldPile','roundBush','berryBush','grassTuft','wildBush',
  'compactBush','lowMoundBush','forkedBush','smallBerryBush','travelerChest','gemChest','ironboundChest','royalChest','chilliPepper','banana',
  'carrot','eggplant','cornCob','pumpkin','radish','cucumber','pickaxe','hatchet','hammer','spade','hoe','rake','sickle','wateringCan',
  'packedLiteBush','packedLiteWildflower','packedLiteRose','packedLiteSunflower','packedLiteDaisy','packedLiteGrass','packedLitePalmTree',
  'packedLiteOakTree','packedLiteBirchTree','packedLiteTree','packedLiteSpruceTree','dynamicPortalStanding','dynamicPortalCircle'
];
const outDir = path.join(__dirname, '..', 'Cube', 'Assets');
fs.mkdirSync(outDir, { recursive: true });
for (const preset of presets) {
  sandbox.api.reset();
  if (preset.startsWith('packedLite')) sandbox.buildPackedLite(preset);
  else sandbox.api.builders[preset]();
  const records = sandbox.api.records();
  if (!records.length) throw new Error(`${preset}: no records`);
  fs.writeFileSync(path.join(outDir, `${preset}.cubes`), sandbox.api.encode(records));
  console.log(`${preset}: ${records.length} cubes`);
}
console.log(`Generated ${presets.length} assets in ${outDir}`);
