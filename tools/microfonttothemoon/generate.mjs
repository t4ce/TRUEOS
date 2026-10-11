// Run the studio's pure geometry functions only, without its UI script.
import fs from 'node:fs';
import vm from 'node:vm';
import {performance} from 'node:perf_hooks';
import crypto from 'node:crypto';
const han=process.argv[4]==='han';
const html=fs.readFileSync('src/intel/gpu_font/continuum-cell-studio.html','utf8');
const detail=Number(process.argv[2]||6);
if(![6,12,24,40].includes(detail)) throw new Error('detail must be 6 (Draft), 12, 24, or 40');
const context=vm.createContext({performance});
const start=html.indexOf('function buildOrganic(input)');
const end=html.indexOf('</script>',start);
vm.runInContext(html.slice(start,end),context);
const cs=html.indexOf('const FPIXELS ='),ce=html.indexOf('const byId=',cs);
const catalog=vm.runInContext(html.slice(cs,ce)+';catalog',context);
let glyphs=[...catalog.filter(g=>g.kind!=='custom'),...catalog.filter(g=>g.kind==='custom')];
let hanSource='';
if(han) {
 const records=fs.readFileSync('bld/microfonttothemoon/han-masks.txt','utf8').trim().split('\n');
 glyphs=records.map(line=>{const [cp,bits]=line.split(' ');return {char:String.fromCodePoint(parseInt(cp,16)),bits:BigInt('0x'+bits),kind:'han',full66:true,width:6};});
 if(glyphs.length!==512)throw new Error('incomplete GUI Han subset');
 hanSource=fs.readFileSync('vendor/microfont/src/gui_han.rs','utf8')+fs.readFileSync('vendor/microfont/src/gui_han_data.rs','utf8');
}
const cell={stroke:.64,doubleGap:.70,edgeGhosts:true,continuations:true,boxSource:'ports',dotSize:1,dotStyle:'round',barLength:1.7,dotGap:.24,brailleLayout:'microfont',joinDots:false,blockCoverage:'ideal',blockInset:0,shadeRows:8};
const settings={bias:0,smartBias:true,cell,model:'membrane',reach:1.85,sigma:.44,level:.48,balance:.82,stretch:1.4,detail,prune:true,cross:false,contours:false};
function sites(g) {
 const points=[]; const cells=g.cells||[g.bits]; const count=g.kind==='custom'||g.full66?66:64;
 cells.forEach((bits,k)=>{for(let i=0;i<count;i++)if((bits>>BigInt(count-1-i))&1n)points.push({x:k*6+i%6+.5,y:Math.floor(i/6)+.5,group:0});});
 return points;
}
const vertices=[],indices=[],stats=[];
for(let slot=0;slot<glyphs.length;slot++) {
 const g=glyphs[slot],cp=g.char.codePointAt(0);
 const family=cp>=0x2500&&cp<=0x257f?'box':cp>=0x2580&&cp<=0x259f?'block':cp>=0x2800&&cp<=0x28ff?'braille':g.char===' '||g.char==='\u00a0'?'space':'field';
 const points=sites(g),tile={char:g.char,family,x:0,y:0,width:g.width,height:11,points,mask:family==='braille'?cp-0x2800:0};
 if(family==='block')tile.blockPoints=sites(catalog.find(g=>g.id==='block-'+(cp-0x2580)));
 const input={id:slot,width:g.width,height:11,points,tiles:[tile],settings:{...settings,route:family==='field'?'field':'cells'}};
 context.input=input;
 const mesh=vm.runInContext('buildShape(input)',context);
 if(mesh.stats.openLoops||mesh.stats.nonmanifold)throw new Error(`invalid mesh slot ${slot}`);
 const base=vertices.length/3;
 const ox=(slot%32)*80+4,oy=Math.floor(slot/32)*64+10;
 // Render0 accepts clip-position vertices. Positive source Y points down.
 for(let i=0;i<mesh.positions.length;i+=2)vertices.push(Math.fround((ox+mesh.positions[i]*4)/2560*2-1),Math.fround(1-(oy+mesh.positions[i+1]*4)/1088*2),.5);
 let gpuTriangles=0,collapsedFaces=0,float32FlippedFaces=0;
 for(let i=0;i<mesh.indices.length;i+=3) {
  const ids=[base+mesh.indices[i],base+mesh.indices[i+2],base+mesh.indices[i+1]];
  const [a,b,c]=ids.map(v=>vertices.slice(v*3,v*3+2));
  const area=(b[0]-a[0])*(c[1]-a[1])-(b[1]-a[1])*(c[0]-a[0]);
  // Float32 clip-position conversion can collapse a microscopic threshold
  // sliver. Record collapses and normalize winding after quantization.
  if(area===0){collapsedFaces++;continue;}
  if(area<0){[ids[1],ids[2]]=[ids[2],ids[1]];float32FlippedFaces++;}
  indices.push(...ids);gpuTriangles++;
 }
 stats.push({slot,char:g.char,kind:g.kind,width:g.width,byte:g.byte,route:mesh.route,spacing:mesh.stats.spacing,vertices:mesh.stats.vertices,triangles:mesh.stats.triangles,gpuTriangles,collapsedFaces,float32FlippedFaces});
}
const bytes=Buffer.alloc(16+vertices.length*4+indices.length*4);
bytes.write('MOONMSH1',0);bytes.writeUInt32LE(vertices.length/3,8);bytes.writeUInt32LE(indices.length,12);
vertices.forEach((x,i)=>bytes.writeFloatLE(x,16+i*4));
indices.forEach((x,i)=>bytes.writeUInt32LE(x,16+vertices.length*4+i*4));
const root=(process.argv[3]||'src/intel/gpu_font/moon').replace(/\/$/,'')+'/';
fs.mkdirSync(root,{recursive:true});
fs.writeFileSync(root+'draft.mesh',bytes);
fs.writeFileSync(root+'config.rs',`// Generated geometry configuration.\nconst WIDTH: u32 = 2560;\nconst HEIGHT: u32 = 1088;\nconst DETAIL: u32 = ${detail};\n`);
fs.writeFileSync(root+'catalog.json',JSON.stringify({detail,scale:4,width:2560,height:1088,model:'membrane',route:'auto',source_sha256:crypto.createHash('sha256').update(html).digest('hex'),set:han?'gui-han':'studio',glyph_count:glyphs.length,han_source_sha256:han?crypto.createHash('sha256').update(hanSource).digest('hex'):undefined,mesh_sha256:crypto.createHash('sha256').update(bytes).digest('hex'),glyphs:stats},null,2)+'\n');
console.log(`Detail ${detail}: ${glyphs.length} glyphs, ${vertices.length/3} vertices, ${indices.length/3} triangles, ${bytes.length} bytes`);
