const fs = require('fs');
const path = require('path');
const { spawn } = require('child_process');
const assert = require('node:assert/strict');
const { JSDOM, VirtualConsole } = require('jsdom');
const root = path.resolve(process.env.TRUEOS_REPOSITORY || process.cwd());
const qa = path.resolve(process.env.TRUEOS_QA_OUTPUT_DIR || __dirname);
const run = fs.mkdtempSync(path.join(qa, 'size-dom-run-'));
const origin = 'http://127.0.0.1:8898';
const server = spawn('python3', [
  'tools/cloudbuild/cloudbuild.py', '--state-dir', path.join(run, 'state'),
  'serve', '--port', '8898', '--allow-anonymous',
], { cwd: root, stdio: ['ignore', 'pipe', 'pipe'] });
let logs = '';
server.stdout.on('data', value => { logs += value; });
server.stderr.on('data', value => { logs += value; });
const pause = ms => new Promise(resolve => setTimeout(resolve, ms));
const wait = async (predicate, label) => {
  for (let i = 0; i < 200; i++) {
    if (predicate()) return;
    await pause(25);
  }
  throw new Error('Timed out: ' + label);
};
const errors = [], doms = [], network = [], checks = [];
const kB = bytes => `${Math.round(bytes / 1000).toLocaleString('en-US')} kB`;

async function openPage(transformCatalog) {
  const vc = new VirtualConsole();
  vc.on('jsdomError', error => errors.push(error.message));
  const downloads = [], blobs = new Map();
  let serial = 0;
  const dom = await JSDOM.fromURL(origin, {
    resources: 'usable', runScripts: 'dangerously', pretendToBeVisual: true,
    virtualConsole: vc,
    beforeParse(w) {
      w.fetch = async (url, opts = {}) => {
        const target = new URL(url, w.location.href).href;
        network.push({ url: target, method: opts.method || 'GET', body: opts.body ? JSON.parse(opts.body) : undefined });
        const response = await fetch(target, { ...opts, headers: { ...opts.headers, Origin: origin } });
        if (transformCatalog && target.endsWith('/api/catalog')) {
          const data = await response.json();
          transformCatalog(data.catalog, data);
          return new Response(JSON.stringify(data), { status: response.status, headers: { 'Content-Type': 'application/json' } });
        }
        return response;
      };
      w.AbortController = AbortController;
      w.Blob = Blob;
      w.HTMLElement.prototype.scrollIntoView = function () {};
      w.URL.createObjectURL = blob => {
        const url = `blob:${origin}/qa-${serial++}`;
        blobs.set(url, blob);
        return url;
      };
      w.URL.revokeObjectURL = url => blobs.delete(url);
      w.HTMLAnchorElement.prototype.click = function () {
        if (this.download) downloads.push({ filename: this.download, blob: blobs.get(this.href) });
      };
    },
  });
  doms.push(dom);
  const w = dom.window, d = w.document;
  const by = id => d.getElementById(id);
  const resolved = () => wait(() => by('plan-state')?.textContent === 'VALID', 'resolved plan');
  const manifest = () => JSON.parse(by('manifest-json').textContent);
  const click = (name, value) => d.querySelector(`input[name="${name}"][value="${value}"]`).click();
  const file = (id, content) => {
    const bytes = Buffer.from(content);
    Object.defineProperty(by(id), 'files', { configurable: true, value: [{ name: 'access.json', size: bytes.length, text: async () => bytes.toString() }] });
    by(id).dispatchEvent(new w.Event('change', { bubbles: true }));
  };
  await resolved();
  return { dom, by, d, w, resolved, manifest, click, file, downloads };
}

(async () => {
  for (let i = 0; i < 60; i++) {
    try { if ((await fetch(origin + '/api/catalog')).ok) break; } catch {}
    if (server.exitCode !== null || i === 59) throw new Error('QA server unavailable: ' + logs);
    await pause(100);
  }
  const { catalog } = await (await fetch(origin + '/api/catalog')).json();
  const sizes = catalog.size_estimates;
  assert.equal(sizes.schema, 1);
  assert.equal(sizes.reference.source_commit, null);
  const appIds = catalog.buildins.map(({ id }) => id).sort();
  const bytes = ids => ids.reduce((sum, id) => sum + sizes.buildins[id].packed_bp_bytes, 0);
  const allBytes = bytes(appIds);
  const kernelBytes = sizes.kernel.base_bytes;
  assert.equal(allBytes, 6115201);
  assert.equal(kernelBytes, 20388475);
  const p = await openPage();
  const original = p.manifest();

  for (const id of appIds) {
    const badge = p.by(`buildin-size-${id}`);
    assert.equal(badge.textContent, kB(sizes.buildins[id].packed_bp_bytes));
    assert(badge.title.includes(sizes.buildins[id].packed_bp_bytes.toLocaleString('en-US')));
    const checkbox = p.d.querySelector(`input[name=buildin][value="${id}"]`);
    assert(checkbox.getAttribute('aria-describedby').split(' ').includes(badge.id));
  }
  assert.equal(p.by('buildin-size-ssh').textContent, '648 kB');
  assert.equal(p.d.querySelectorAll('.buildin-size').length, 12);
  assert(p.by('buildin-size-basis').textContent.includes(sizes.reference.label));
  checks.push('All 12 badges use current packed BP bytes, decimal kB, and accessible descriptions; SSH is 648 kB.');

  assert.equal(p.by('size-apps').textContent, '6,115 kB');
  const roundedBadgeSum = appIds.reduce((sum, id) => sum + Math.round(sizes.buildins[id].packed_bp_bytes / 1000), 0);
  assert.equal(roundedBadgeSum, 6116);
  assert.notEqual(p.by('size-apps').textContent, `${roundedBadgeSum.toLocaleString('en-US')} kB`);
  assert.match(p.by('size-apps').title, /^6,115,201 bytes/);
  assert.equal(p.by('size-kernel').textContent, '≈ 20,388 kB');
  assert.equal(p.by('size-total').textContent, '≈ 26,504 kB');
  assert.equal(p.by('size-saved').textContent, '0 kB');
  assert.equal(p.by('size-reference-kernel').textContent, '26,214 kB');
  assert.equal(p.by('size-reference-download').textContent, '19,447 kB');
  assert.equal(p.by('size-kernel-label').textContent, sizes.kernel.label + ' (estimated)');
  assert.equal(p.by('size-method-note').textContent, sizes.kernel.note);
  assert.equal(p.by('size-meter').getAttribute('aria-valuenow'), '100.0');
  assert.match(p.by('size-context').textContent, /no verified source revision; rebuilt sizes may vary/);
  checks.push('Exact byte accumulation rounds once (6,115 kB vs 6,116 summed badges), with distinct derived baseline, estimated total, measured ELF, and download.');

  p.by('buildins-none').click();
  assert.equal(p.by('size-apps').textContent, '0 kB');
  assert.equal(p.by('size-saved').textContent, '6,115 kB');
  assert.equal(p.by('size-total').textContent, '≈ 20,388 kB');
  assert.equal(p.by('size-meter').getAttribute('aria-valuenow'), '0.0');
  assert.equal(p.by('size-meter-selected').style.width, '0%');
  await p.resolved();
  assert.deepEqual(p.manifest().buildins, []);
  assert.deepEqual(p.manifest().features, original.features.filter(id => id !== 'buildins'));
  assert(!p.by('buildins-note').hidden);
  checks.push('None updates immediately to zero app bytes, all app payload omitted, derived baseline only, and valid no-app manifest.');

  p.click('buildin', 'edit');
  p.click('buildin', 'termdir');
  const subset = ['edit', 'termdir'];
  const subsetBytes = bytes(subset);
  assert.equal(p.by('size-apps').textContent, '812 kB');
  assert.equal(p.by('size-total').textContent, kB(kernelBytes + subsetBytes).replace(/^/, '≈ '));
  assert.equal(p.by('size-saved').textContent, kB(allBytes - subsetBytes));
  assert.match(p.by('size-apps').title, /^811,802 bytes/);
  await p.resolved();
  assert.deepEqual(p.manifest().buildins, subset);
  const subsetStats = ['size-kernel', 'size-apps', 'size-total', 'size-saved'].map(id => p.by(id).textContent);
  for (const id of ['usb', 'igpu', 'virgl']) {
    p.click('feature', id);
    assert.deepEqual(['size-kernel', 'size-apps', 'size-total', 'size-saved'].map(name => p.by(name).textContent), subsetStats);
    assert.equal(p.by('size-subsystems').textContent, 'Not measured');
    await p.resolved();
    assert.deepEqual(p.manifest().buildins, subset);
  }
  assert.equal(p.by('size-subsystems-note').textContent, sizes.subsystems.note);
  checks.push('Edit + Termdir gives 811,802 exact bytes; USB/iGPU/Virgl switches preserve the app estimate and deduct no unmeasured savings.');

  p.click('profile', 'dev');
  assert.equal(p.by('size-heading').textContent, 'Release size reference');
  assert.equal(p.by('size-total-label').textContent, 'Release comparison total');
  assert.match(p.by('size-context').textContent, /Development selected.*comparison reference/);
  assert.deepEqual(['size-kernel', 'size-apps', 'size-total', 'size-saved'].map(name => p.by(name).textContent), subsetStats);
  await p.resolved();
  assert.equal(p.manifest().profile, 'dev');
  p.click('profile', 'release');
  await p.resolved();
  assert.equal(p.by('size-total-label').textContent, 'Estimated kernel + apps');
  checks.push('Development explicitly displays a release comparison, never a measured development total.');

  p.by('build-button').click();
  await wait(() => !p.by('job-panel').hidden && p.by('job-heading').textContent === 'Build queued' && !p.by('buildins-none').disabled, 'queued subset');
  const queuedSize = p.by('job-size-estimate').textContent;
  assert(!p.by('job-size-estimate').hidden);
  assert(queuedSize.includes(kB(subsetBytes)));
  assert(queuedSize.includes(kB(allBytes - subsetBytes)));
  assert(queuedSize.includes(sizes.reference.label));
  p.by('save-access').click();
  const access = JSON.parse(await p.downloads.pop().blob.text());
  p.by('buildins-all').click();
  await p.resolved();
  assert.equal(p.by('size-apps').textContent, kB(allBytes));
  assert.equal(p.by('job-size-estimate').textContent, queuedSize);
  const stored = await (await fetch(origin + '/api/jobs/' + access.job_id, { headers: { Authorization: 'Bearer ' + access.access_token } })).json();
  assert.deepEqual(stored.manifest.buildins, subset);
  const resumed = await openPage();
  resumed.file('resume-file', JSON.stringify(access));
  await wait(() => !resumed.by('job-panel').hidden, 'resumed subset job');
  assert.deepEqual(resumed.manifest().buildins, appIds);
  assert.equal(resumed.by('job-size-estimate').textContent, queuedSize);
  checks.push('Queued and resumed build estimates stay bound to their own saved app selection while the editable selector changes.');

  const missing = await openPage(c => { delete c.size_estimates; });
  assert.equal(missing.by('size-apps').textContent, 'Unmeasured');
  assert.equal(missing.by('size-total').textContent, 'Unmeasured');
  assert.equal(missing.by('buildin-size-ssh').textContent, 'Unmeasured');
  assert(missing.by('size-meter').hidden);
  assert.equal(missing.manifest().config_hash, original.config_hash);
  missing.by('buildins-none').click();
  await missing.resolved();
  assert.equal(missing.by('size-apps').textContent, '0 kB');
  assert.equal(missing.by('size-saved').textContent, 'Unmeasured');
  assert.equal(missing.by('size-total').textContent, 'Unmeasured');
  checks.push('Missing optional metadata leaves selection and authoritative hash intact; zero selected is zero, missing measurements are unknown.');

  const invalid = await openPage(c => { c.size_estimates.buildins.edit.packed_bp_bytes = -1; });
  assert.equal(invalid.by('buildin-size-edit').textContent, 'Unmeasured');
  assert.equal(invalid.by('size-apps').textContent, 'Unmeasured');
  assert.equal(invalid.by('size-total').textContent, 'Unmeasured');
  assert(invalid.by('size-meter').hidden);
  assert.equal(invalid.manifest().config_hash, original.config_hash);
  invalid.by('buildins-none').click();
  invalid.click('buildin', 'termdir');
  await invalid.resolved();
  assert.equal(invalid.by('size-apps').textContent, kB(sizes.buildins.termdir.packed_bp_bytes));
  assert.equal(invalid.by('size-saved').textContent, 'Unmeasured');
  assert.equal(invalid.by('size-total').textContent, '≈ ' + kB(kernelBytes + sizes.buildins.termdir.packed_bp_bytes));
  checks.push('Invalid individual byte counts propagate unknown only to affected totals, without breaking valid selections.');

  const changedReference = await openPage(c => {
    c.size_estimates.reference.label = 'Different snapshot';
    c.size_estimates.reference.source_commit = '0'.repeat(40);
    c.size_estimates.kernel.label = 'Different base label';
  });
  assert(changedReference.by('size-reference').textContent.includes('Different snapshot'));
  assert(changedReference.by('buildin-size-basis').textContent.includes('Different snapshot'));
  assert.equal(changedReference.by('size-kernel-label').textContent, 'Different base label (estimated)');
  assert.match(changedReference.by('size-context').textContent, /selected source differs/);
  assert.equal(changedReference.manifest().config_hash, original.config_hash);
  checks.push('Reference labels come from metadata; a known different source is identified without changing the manifest hash.');

  const allowedRequestKeys = ['buildins', 'features', 'preset', 'profile'];
  for (const request of network.filter(item => item.url.endsWith('/api/plan') || item.url.endsWith('/api/builds'))) {
    const spec = request.url.endsWith('/api/builds') ? request.body.request : request.body;
    if (request.url.endsWith('/api/builds')) assert.deepEqual(Object.keys(request.body), ['request']);
    assert.deepEqual(Object.keys(spec).sort(), allowedRequestKeys);
    assert(Array.isArray(spec.buildins));
    assert(!Object.hasOwn(spec.features, 'buildins'));
  }
  assert(!JSON.stringify(stored.manifest).includes('size_estimates'));
  assert(!JSON.stringify(stored.manifest).includes('packed_bp_bytes'));
  assert.deepEqual(errors, []);
  checks.push('All plan/build requests preserve the original request shape; no size metadata enters persisted manifests, and JavaScript raises no errors.');

  const result = {
    passed: true,
    kind: 'jsdom DOM and real Python HTTP integration; no visual browser assertion',
    reference: sizes.reference.label,
    exact_bytes: { all_apps: allBytes, kernel_baseline: kernelBytes, all_estimated: allBytes + kernelBytes, edit_and_termdir: subsetBytes },
    checks,
    javascriptErrors: errors,
  };
  fs.writeFileSync(path.join(qa, 'size-estimates-dom-results.json'), JSON.stringify(result, null, 2) + '\n');
  console.log(JSON.stringify(result, null, 2));
})().catch(error => {
  console.error(error.stack);
  console.error(logs);
  process.exitCode = 1;
}).finally(() => {
  for (const dom of doms) dom.window.close();
  server.kill('SIGTERM');
  fs.rmSync(run, { recursive: true, force: true });
});
