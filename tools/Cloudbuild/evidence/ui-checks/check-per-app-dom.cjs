const fs = require('fs');
const path = require('path');
const { spawn, execFileSync } = require('child_process');
const assert = require('node:assert/strict');
const { JSDOM, VirtualConsole } = require('jsdom');
const root = path.resolve(process.env.TRUEOS_REPOSITORY || process.cwd());
const qa = path.resolve(process.env.TRUEOS_QA_OUTPUT_DIR || __dirname);
const run=fs.mkdtempSync(path.join(qa,'per-app-dom-run-'));
process.env.TMPDIR=path.join(run,'tmp');fs.mkdirSync(process.env.TMPDIR);
const origin='http://127.0.0.1:8897';
const state=path.join(run,'state');
const issuer=path.join(run,'issuer'), claimant=path.join(run,'claimant');
for(const key of [issuer,claimant])execFileSync('ssh-keygen',['-q','-t','ed25519','-N','','-f',key]);
const server=spawn('python3',['tools/cloudbuild/cloudbuild.py','--state-dir',state,'serve','--port','8897','--issuer-key',issuer,'--allow-anonymous'],{cwd:root,stdio:['ignore','pipe','pipe']});
let logs='';server.stdout.on('data',x=>logs+=x);server.stderr.on('data',x=>logs+=x);
const pause=ms=>new Promise(r=>setTimeout(r,ms));
const wait=async(fn,label)=>{for(let i=0;i<160;i++){if(fn())return;await pause(25);}throw new Error('Timed out: '+label);};
const errors=[],doms=[],network=[];
async function openPage(){
  const downloads=[],blobs=new Map();let serial=0;
  const vc=new VirtualConsole();vc.on('jsdomError',e=>errors.push(e.message));
  const dom=await JSDOM.fromURL(origin,{resources:'usable',runScripts:'dangerously',pretendToBeVisual:true,virtualConsole:vc,beforeParse(w){
    w.fetch=(url,opts={})=>{const target=new URL(url,w.location.href).href;network.push({url:target,method:opts.method||'GET',body:opts.body?JSON.parse(opts.body):undefined});return fetch(target,{...opts,headers:{...opts.headers,Origin:origin}});};
    w.AbortController=AbortController;w.Blob=Blob;
    w.HTMLElement.prototype.scrollIntoView=function(){};
    w.URL.createObjectURL=blob=>{const url=`blob:${origin}/qa-${serial++}`;blobs.set(url,blob);return url;};
    w.URL.revokeObjectURL=url=>blobs.delete(url);
    w.HTMLAnchorElement.prototype.click=function(){if(this.download)downloads.push({filename:this.download,blob:blobs.get(this.href)});};
  }});
  doms.push(dom);const w=dom.window,d=w.document;
  await wait(()=>d.getElementById('plan-state')?.textContent==='VALID','initial plan');
  const by=id=>d.getElementById(id);
  const input=(id,value)=>{by(id).value=value;by(id).dispatchEvent(new w.Event('input',{bubbles:true}));};
  const file=(id,name,content)=>{const bytes=Buffer.from(content);Object.defineProperty(by(id),'files',{configurable:true,value:[{name,size:bytes.length,text:async()=>bytes.toString()}]});by(id).dispatchEvent(new w.Event('change',{bubbles:true}));};
  const manifest=()=>JSON.parse(by('manifest-json').textContent);
  const resolved=()=>wait(()=>by('plan-state').textContent==='VALID','resolved plan');
  return{w,d,by,input,file,manifest,resolved,downloads,dom};
}
(async()=>{
  for(let i=0;i<60;i++){try{if((await fetch(origin+'/api/catalog')).ok)break;}catch{}if(server.exitCode!==null||i===59)throw Error('QA server unavailable '+logs);await pause(100);}
  execFileSync('python3',['tools/cloudbuild/cloudbuild.py','--state-dir',state,'grant','--public-key',claimant+'.pub','--license-id','PER-APP-UI-TEST'],{cwd:root,stdio:'pipe'});
  const catalog=(await (await fetch(origin+'/api/catalog')).json()).catalog;
  const allIds=catalog.buildins.map(({id})=>id).sort();
  const labels=Object.fromEntries(catalog.buildins.map(({id,label})=>[id,label]));
  assert.equal(allIds.length,12);
  assert.deepEqual(allIds,['apps','backup','commander','edit','hull','img','os','paper','share','ssh','termdir','trauma']);
  const p=await openPage();
  const feature=id=>p.d.querySelector(`input[name=feature][value="${id}"]`);
  const buildin=id=>p.d.querySelector(`input[name=buildin][value="${id}"]`);
  const preset=id=>p.d.querySelector(`input[name=preset][value="${id}"]`);
  assert.equal(p.d.querySelectorAll('input[name=buildin]').length,allIds.length);
  assert.equal(feature('buildins'),null);
  assert.equal(p.d.querySelectorAll('input[name=feature]').length,catalog.features.length-1);
  assert.deepEqual(p.manifest().buildins,allIds);
  assert.equal(p.manifest().schema,2);
  assert.equal(p.by('summary-buildins').textContent,`${allIds.length} of ${allIds.length} selected`);
  for(const label of Object.values(labels))assert(p.by('selected-buildins').textContent.includes(label));
  assert(p.by('buildins-all').disabled);
  const original=p.manifest();
  const kernelFeatures=original.features.filter(id=>id!=='buildins');
  buildin('edit').click();await p.resolved();
  assert.deepEqual(p.manifest().buildins,allIds.filter(id=>id!=='edit'));
  assert.deepEqual(p.manifest().features,original.features);
  assert.deepEqual(p.manifest().cargo_features,original.cargo_features);
  assert.notEqual(p.manifest().config_hash,original.config_hash);
  assert(!p.by('selected-buildins').textContent.split(': ')[1].split(', ').includes(labels.edit));
  p.by('buildins-none').click();await p.resolved();
  assert.deepEqual(p.manifest().buildins,[]);
  assert.deepEqual(p.manifest().features,kernelFeatures);
  assert(!p.manifest().cargo_features.includes('buildin-apps'));
  assert.equal(p.by('summary-buildins').textContent,'None · Kernel only');
  assert(!p.by('buildins-note').hidden);
  assert(p.by('buildins-none').disabled);
  for(const id of allIds){
    buildin(id).click();await p.resolved();
    assert.deepEqual(p.manifest().buildins,[id]);
    assert.deepEqual(p.manifest().features,original.features);
    assert.equal(p.d.querySelectorAll('input[name=buildin]:checked').length,1);
    assert.match(p.by('summary-buildins').textContent,/^1 of 12 selected$/);
    assert.equal(p.by('selected-buildins').textContent,`Selected apps: ${labels[id]}`);
    p.by('buildins-none').click();
  }
  await p.resolved();
  feature('usb').click();await p.resolved();
  const withoutUsb=p.manifest().features;
  p.by('buildins-all').click();await p.resolved();
  assert.deepEqual(p.manifest().buildins,allIds);
  assert.deepEqual(p.manifest().features,[...withoutUsb,'buildins'].sort());
  assert(!feature('usb').checked);
  p.by('buildins-none').click();await p.resolved();
  assert.deepEqual(p.manifest().features,withoutUsb);
  assert(!feature('usb').checked);
  preset('qemu-virgl').click();await p.resolved();
  assert.deepEqual(p.manifest().buildins,allIds);
  assert.deepEqual(p.manifest().features,[...catalog.presets.find(p=>p.id==='qemu-virgl').features].sort());
  preset('kernel-only').click();await p.resolved();
  assert.deepEqual(p.manifest().buildins,[]);
  assert.deepEqual(p.manifest().features,kernelFeatures);
  buildin('commander').click();buildin('edit').click();await p.resolved();
  const beforeProof=p.manifest();
  p.input('public-key',fs.readFileSync(claimant+'.pub','utf8').trim());
  p.by('build-button').click();await wait(()=>!p.by('challenge-panel').hidden,'first challenge');
  p.by('download-challenge').click();const firstProof=JSON.parse(await p.downloads.pop().blob.text());
  assert.equal(firstProof.config_hash,beforeProof.config_hash);
  buildin('apps').click();
  assert(p.by('challenge-panel').hidden);
  assert.equal(p.by('signature').value,'');
  assert(p.by('verify-button').disabled);
  await p.resolved();
  assert.notEqual(p.manifest().config_hash,beforeProof.config_hash);
  p.by('build-button').click();await wait(()=>!p.by('challenge-panel').hidden,'fresh challenge after app edit');
  p.by('download-challenge').click();const proofText=await p.downloads.pop().blob.text();
  const newProof=JSON.parse(proofText);
  assert.notEqual(newProof.challenge_id,firstProof.challenge_id);
  assert.equal(newProof.config_hash,p.manifest().config_hash);
  const challengePath=path.join(run,'trueos-challenge.json');fs.writeFileSync(challengePath,proofText);
  execFileSync('ssh-keygen',['-Y','sign','-f',claimant,'-n','build-request@trueos.eu',challengePath],{stdio:'pipe'});
  p.file('signature-file','trueos-challenge.json.sig',fs.readFileSync(challengePath+'.sig'));
  await wait(()=>!p.by('verify-button').disabled,'signature loaded');
  p.by('verify-button').click();await wait(()=>!p.by('job-panel').hidden&&p.by('job-heading').textContent==='Build queued','subset queued');
  const queuedApps=['apps','commander','edit'];
  const jobText=`Bundled apps: 3 selected — ${queuedApps.map(id=>labels[id]).join(', ')}`;
  assert.equal(p.by('job-buildins').textContent,jobText);
  p.by('save-access').click();const access=JSON.parse(await p.downloads.pop().blob.text());
  p.by('buildins-none').click();await p.resolved();
  assert.equal(p.by('summary-buildins').textContent,'None · Kernel only');
  assert.equal(p.by('job-buildins').textContent,jobText);
  const stored=await(await fetch(origin+'/api/jobs/'+access.job_id,{headers:{Authorization:'Bearer '+access.access_token}})).json();
  assert.deepEqual(stored.manifest.buildins,queuedApps);
  const resumed=await openPage();
  resumed.file('resume-file','access.json',JSON.stringify(access));
  await wait(()=>!resumed.by('job-panel').hidden,'resumed subset job');
  assert.equal(resumed.by('job-buildins').textContent,jobText);
  assert.deepEqual(resumed.manifest().buildins,allIds);
  execFileSync('python3',['-c','import sys; from pathlib import Path; sys.path.insert(0, "tools/cloudbuild"); from service import Store; Store(Path(sys.argv[1])).fail(sys.argv[2], "UI test failure; no OS build was run")',state,access.job_id],{cwd:root});
  await wait(()=>resumed.by('job-heading').textContent==='Build failed','failed subset');
  resumed.by('retry-job').click();await resumed.resolved();
  assert.deepEqual(resumed.manifest().buildins,queuedApps);
  assert(resumed.by('build-button').disabled);
  assert.match(resumed.by('build-button-label').textContent,/public key/);
  resumed.by('download-manifest').click();const downloaded=JSON.parse(await resumed.downloads.pop().blob.text());
  assert.deepEqual(downloaded.buildins,queuedApps);
  for(const req of network.filter(req=>req.url.endsWith('/api/plan'))){
    assert(Array.isArray(req.body.buildins));
    assert(!Object.hasOwn(req.body.features,'buildins'));
  }
  assert.deepEqual(errors,[]);
  const result={passed:true,kind:'jsdom DOM and real Python HTTP/OpenSSH integration; no visual browser assertion',checks:[
    '12 real buildin checkboxes, separate from kernel switches',
    'default selects all and summary lists every app',
    'one app deselection changes hash while preserving kernel flags',
    'None yields Kernel only with no app Cargo feature',
    'each of the 12 apps is independently selectable alone',
    'All and None preserve USB deselection',
    'QEMU and Kernel only presets populate app and kernel choices',
    'individual app edit invalidates an existing signing challenge',
    'new challenge binds updated app-selection hash',
    'real SSH signature queues exactly the selected subset',
    'queued app names stay frozen after selector edits',
    'persisted job records exact app IDs',
    'saved access resumes subset independently of current selector',
    'failed keyed retry restores exact apps and needs new proof',
    'manifest export preserves exact app IDs',
    'requests send app arrays without conflicting master override',
    'no JavaScript exceptions'
  ],javascriptErrors:errors};
  fs.writeFileSync(path.join(qa,'per-app-dom-results.json'),JSON.stringify(result,null,2)+'\n');console.log(JSON.stringify(result,null,2));
})().catch(e=>{console.error(e.stack);console.error(logs);process.exitCode=1;}).finally(()=>{for(const d of doms)d.window.close();server.kill('SIGTERM');fs.rmSync(run,{recursive:true,force:true});});
