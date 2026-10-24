#!/usr/bin/env python3
"""Check the embedded snapshot and full-bin downloads, without a sync tool."""
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[2]
HTML = ROOT / "vendor/microfont/docs/lattice-glyph-studio.html"
TEST = r'''
const fs=require('fs'),vm=require('vm'),assert=require('assert/strict');
const html=fs.readFileSync(process.argv[1],'utf8');
const blocks=Object.fromEntries([...html.matchAll(/<script id="([^"]+)"[^>]*>([\s\S]*?)<\/script>/g)].map(m=>[m[1],m[2]]));
function core(data=blocks){
    const context={document:{getElementById:id=>({textContent:data[id]})}};
    vm.createContext(context);vm.runInContext(data['glyph-core'],context);
    return context.LatticeCore;
}
const c=core(),presets=JSON.parse(blocks['glyph-presets']),bins=JSON.parse(blocks['glyph-binaries']);
const stencils=JSON.parse(blocks['glyph-stencils']),quads=JSON.parse(blocks['glyph-quads']);
new vm.Script(blocks['glyph-ui']);
assert.equal(c.CP850_SLOTS.length,256);
assert.equal(c.CHARS.length,new Set(c.CHARS).size);
assert.ok(c.CHARS.length>=308);
assert.ok(Object.values(presets).filter(s=>s.length).length>=248);
const model=new c.GlyphModel(presets);
for(const ch of c.CHARS){
    const original=Buffer.from(bins[ch],'hex');
    assert.ok(original.length>=20);
    assert.equal(Buffer.from(c.binaryExport(ch,model.glyphs[ch])).toString('hex'),bins[ch],ch);
    const mask=BigInt(c.CATALOG.glyphs[ch].mask);
    for(let i=0;i<16;i++)assert.equal(original[i],Number(mask>>BigInt(i*8)&255n),ch);
    for(let y=0;y<11;y++)for(let x=0;x<6;x++)assert.equal(stencils[ch][y][x],String(Number(mask>>BigInt(65-y*6-x)&1n)),ch);
}
for(const ch of ['á','ı','←','ũ','🯰'])assert.ok(presets[ch].length,ch+' seeded recipe');
for(const ch of ['╭','■','█'])assert.ok(quads[ch].length,ch+' quad preview');
model.switchGlyph('A');model.clear();
const blank=c.binaryExport('A',model.strips);
assert.equal(blank.length,20);assert.equal(blank[17],0);
model.addPoint([1,1]);model.addPoint([4,8]);model.finish();model.setBoundary('start','Junction');
const edited=Buffer.from(c.binaryExport('A',model.strips));
assert.ok(edited.subarray(0,17).equals(Buffer.from(bins.A,'hex').subarray(0,17)));
assert.equal(edited[17],1);assert.equal(edited[18],0);assert.equal(edited[19],0);
assert.equal(edited.subarray(20).toString('hex'),'800944');
const fullBlock=c.binaryExport('█',[{points:[[0,0],[0,0]],start:'Junction',end:'End'}]);
assert.equal(fullBlock[8],3); // Preserve the two mask bits above the legacy u64.
assert.equal(fullBlock[17],1);assert.equal(fullBlock.length,23);
assert.throws(()=>c.binaryExport('A',[{points:[[6,0],[1,1]],start:'End',end:'End'}]));
const old={version:1,atlasVersion:3,grid:{columns:6,rows:11},glyphs:Object.fromEntries(c.CATALOG.legacyAtlasV3.map(ch=>[ch,presets[ch]]))};
const restored=new c.GlyphModel(presets);restored.loadSession(old);
assert.equal(restored.glyphs['ũ'].length,presets['ũ'].length);
assert.equal(restored.session().atlasVersion,4);
// Exercise the actual Save standalone HTML handler. A reopened copy must
// export edited lines, rather than mistake its new presets for old recipes.
const nodes=Object.fromEntries(Object.entries(blocks).map(([id,textContent])=>['#'+id,{textContent}]));
const clone={querySelector:id=>nodes[id]||(nodes[id]={}),querySelectorAll:()=>[],
    get outerHTML(){return Object.entries(nodes).filter(([id])=>id!=='#toast').map(([id,node])=>'<script id="'+id.slice(1)+'">'+node.textContent+'<\/script>').join('');}};
let saved;
const body=blocks['glyph-ui'].match(/\$\('download-html'\)\.addEventListener\('click',\(\)=>\{([\s\S]*?)\n    \}\);/)[1];
const context={document:{documentElement:{cloneNode:()=>clone}},ensureFinished:()=>true,model,
    binarySnapshot:c.binarySnapshot,quadPresets:quads,download:(_,data)=>saved=data,toast:()=>{}};
vm.createContext(context);vm.runInContext('(function(){'+body+'})()',context);
const savedBlocks=Object.fromEntries([...saved.matchAll(/<script id="([^"]+)"[^>]*>([\s\S]*?)<\/script>/g)].map(m=>[m[1],m[2]]));
const reopened=core(savedBlocks),savedPresets=JSON.parse(savedBlocks['glyph-presets']);
assert.equal(Buffer.from(reopened.binaryExport('A',savedPresets.A)).toString('hex'),edited.toString('hex'));
assert.ok(Object.hasOwn(JSON.parse(savedBlocks['glyph-binaries']),'<'));
assert.ok(Object.hasOwn(savedPresets,'<'));
assert.ok(!html.includes('/_sites/dispatch-assets/')&&!html.includes('/cdn-cgi/'));
// Run the actual UI initialization and download handler with a minimal DOM.
const elements=new Map();let blob,filename;
function element(id){
    if(!elements.has(id))elements.set(id,{textContent:blocks[id]||'',innerHTML:'',value:id==='specimen-input'?'Aa Bb 0123':'',
        style:{},dataset:{},events:{},classList:{toggle(){},add(){},remove(){}},
        addEventListener(name,fn){this.events[name]=fn;},setAttribute(){},querySelectorAll(){return[];},
        remove(){},click(){filename=this.download;}});
    return elements.get(id);
}
const doc={getElementById:element,querySelectorAll:()=>[],querySelector:()=>null,
    addEventListener(){},createElement:()=>element('anchor'),body:{append(){}}};
const app={document:doc,localStorage:{getItem:()=>null,setItem(){}},setTimeout:()=>0,clearTimeout(){},Blob,
    URL:{createObjectURL(value){blob=value;return'blob:test';},revokeObjectURL(){}}};
vm.createContext(app);vm.runInContext(blocks['glyph-core'],app);vm.runInContext(blocks['glyph-ui'],app);
assert.equal(app.lattice.snapshot().glyphs.A.length,presets.A.length);
assert.match(element('preset-status').textContent,/loaded/);
element('download-current-bin').events.click();
assert.equal(filename,'u0041.bin');assert.equal(blob.size,Buffer.from(bins.A,'hex').length);
console.log('Lattice: seeded atlas, original/edited bins, high mask bits, old sessions, saved HTML and UI download pass.');
'''


if __name__ == "__main__":
    subprocess.run(["node", "-e", TEST, str(HTML)], check=True)
