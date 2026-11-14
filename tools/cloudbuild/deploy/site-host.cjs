'use strict';

// Optional selector-only backend for a website without a provisioned API.
// All build submission stays disabled. Production supplies TRUEOS_CLOUDBUILD_URL.
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const http = require('node:http');
const { spawn } = require('node:child_process');
let startup;
let child;

function health(origin, port) {
  return new Promise(resolve => {
    const request = http.get(`http://127.0.0.1:${port}/api/catalog`, { headers: { Host: origin.host } }, response => {
      response.resume();
      resolve(response.statusCode === 200);
    });
    request.setTimeout(1000, () => request.destroy());
    request.on('error', () => resolve(false));
  });
}

function ensureSelector() {
  if (startup) return startup;
  startup = (async () => {
    const origin = new URL(process.env.TRUEOS_CLOUDBUILD_ORIGIN || 'https://trueos.eu');
    const port = Number(process.env.TRUEOS_CLOUDBUILD_PORT || '8787');
    if (!Number.isInteger(port) || port < 1 || port > 65535) throw new Error('Invalid selector port');
    if (await health(origin, port)) return;
    const runtime = path.join(__dirname, 'cloudbuild');
    const metadata = JSON.parse(fs.readFileSync(path.join(runtime, 'runtime.json'), 'utf8'));
    if (!/^[a-f0-9]{40}$/.test(metadata.source_commit)) throw new Error('Invalid packaged source revision');
    const state = process.env.TRUEOS_CLOUDBUILD_STATE_DIR || path.join(
      process.env.XDG_STATE_HOME || path.join(os.homedir(), '.local', 'state'), 'trueos-cloudbuild');
    let failure;
    child = spawn(process.env.TRUEOS_PYTHON || 'python3', [
      path.join(runtime, 'cloudbuild.py'), '--state-dir', state, 'serve',
      '--origin', origin.origin, '--source-commit', metadata.source_commit, '--port', String(port),
    ], { stdio: ['ignore', 'inherit', 'inherit'], windowsHide: true });
    child.once('error', error => { failure = error; });
    child.once('exit', code => { failure = new Error(`Selector backend exited (${code})`); startup = undefined; });
    for (let i = 0; i < 50; i++) {
      await new Promise(resolve => setTimeout(resolve, 100));
      if (await health(origin, port)) {
        console.log('[cloudbuild] Selector ready; cloud build submission remains disabled.');
        return;
      }
      if (failure) throw failure;
    }
    throw new Error('Selector backend did not become ready');
  })().catch(error => {
    console.error('[cloudbuild] Could not start selector:', error.message);
    if (child && child.exitCode === null) child.kill();
    startup = undefined;
    throw error;
  });
  return startup;
}

// A child exit also invalidates the cached health result for the next request.
process.once('exit', () => { if (child && child.exitCode === null) child.kill(); });
for (const signal of ['SIGINT', 'SIGTERM']) {
  process.once(signal, () => {
    if (child && child.exitCode === null) child.kill();
    process.exit(0);
  });
}
module.exports = { ensureSelector };
