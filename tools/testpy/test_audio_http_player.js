#!/usr/bin/env node
'use strict';

// Exercise the real browser script with deterministic async boundaries. These
// stubs observe scheduling/lifecycle; actual codec/device behavior needs the
// separate real-browser test. No network or physical rig is contacted here.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const root = path.resolve(__dirname, '../..');
const html = fs.readFileSync(path.join(root, 'src/r/services/audio_http/player.html'), 'utf8');
const match = html.match(/<script>([\s\S]*?)<\/script>/);
assert.ok(match, 'production player has an inline script');

function deferred() {
  const result = { settled: false };
  result.promise = new Promise((resolve, reject) => {
    result.resolve = value => { if (!result.settled) { result.settled = true; resolve(value); } };
    result.reject = error => { if (!result.settled) { result.settled = true; reject(error); } };
  });
  return result;
}

async function until(predicate, description) {
  for (let attempt = 0; attempt < 100; ++attempt) {
    if (predicate()) return;
    await Promise.resolve();
  }
  assert.fail('async boundary not reached: ' + description);
}

async function settle() {
  for (let attempt = 0; attempt < 20; ++attempt) await Promise.resolve();
}

function fixture() {
  const state = { requests: [], timers: [], reports: [], bodies: [] };
  class AudioContext {
    constructor(options) {
      assert.equal(options.latencyHint, 'playback');
      this.currentTime = 1;
      this.destination = {};
      this.resumes = [];
      this.decodes = [];
      this.speech = [];
      this.constants = [];
      this.closed = false;
      state.context = this;
    }
    createGain() {
      const gain = { gain: { value: NaN }, connections: [], connect(target) { this.connections.push(target); } };
      this.gain = gain;
      return gain;
    }
    createConstantSource() {
      const source = {
        offset: { value: 1 }, connections: [], starts: 0, stops: 0,
        connect(target) { this.connections.push(target); },
        start() { ++this.starts; }, stop() { ++this.stops; },
      };
      this.constants.push(source);
      return source;
    }
    createBufferSource() {
      const context = this;
      const source = {
        connections: [], starts: [], disconnects: 0, stops: 0,
        connect(target) { this.connections.push(target); },
        start(when, offset) { this.starts.push({ when, offset, clock: context.currentTime }); },
        end() { if (this.onended) this.onended(); },
        stop() { ++this.stops; this.end(); },
        disconnect() { ++this.disconnects; },
      };
      this.speech.push(source);
      return source;
    }
    resume() {
      const result = deferred();
      this.resumes.push(result);
      return result.promise;
    }
    decodeAudioData(bytes) {
      const result = Object.assign(deferred(), { bytes });
      this.decodes.push(result);
      return result.promise;
    }
    close() { this.closed = true; return Promise.resolve(); }
  }

  const elements = new Map(['listen', 'status', 'volume'].map(id => [id, { value: '1', textContent: '' }]));
  const sandbox = vm.createContext({
    window: { AudioContext, addEventListener() {} },
    document: { getElementById: id => elements.get(id) },
    AbortController,
    fetch(url, options) {
      assert.equal(options.cache, 'no-store');
      const request = Object.assign(deferred(), { url, signal: options.signal, claimed: false });
      const abort = () => request.reject(new Error('fetch aborted'));
      request.signal.addEventListener('abort', abort, { once: true });
      request.promise.then(
        () => request.signal.removeEventListener('abort', abort),
        () => request.signal.removeEventListener('abort', abort),
      );
      if (request.signal.aborted) abort();
      state.requests.push(request);
      return request.promise;
    },
    setTimeout(callback, ms) {
      const timer = { callback, ms, active: true };
      state.timers.push(timer);
      return timer;
    },
    clearTimeout(timer) { timer.active = false; },
  });
  vm.runInContext(match[1], sandbox, { filename: 'production-player.html' });
  const ClipPlayer = vm.runInContext('ClipPlayer', sandbox);
  state.player = new ClipPlayer(message => state.reports.push(message), .75);
  state.completion = state.player.start().then(
    () => { state.finished = true; },
    error => { state.finished = true; state.error = error; },
  );
  state.request = async url => {
    await until(() => state.requests.some(r => r.url === url && !r.claimed), 'GET ' + url);
    const request = state.requests.find(r => r.url === url && !r.claimed);
    request.claimed = true;
    return request;
  };
  state.status = async value => {
    const request = await state.request('/audio/status');
    request.resolve({ ok: true, status: 200, json: async () => value });
  };
  state.fullBody = async id => {
    const request = await state.request('/audio/clip.wav?id=' + encodeURIComponent(id));
    const body = deferred();
    state.bodies.push(body);
    request.resolve({ ok: true, status: 200, arrayBuffer: () => body.promise });
    return body;
  };
  state.fire = async ms => {
    await until(() => state.timers.some(t => t.active && t.ms === ms), 'timer ' + ms);
    const timer = state.timers.find(t => t.active && t.ms === ms);
    timer.active = false;
    timer.callback();
  };
  state.stop = async () => {
    if (!state.player.abort.signal.aborted) state.player.stop();
    // Decoding/resume need not be cancellable in a browser. Resolve outstanding
    // callbacks after Stop to verify the production cancellation guards.
    for (let attempt = 0; attempt < 100 && !state.finished; ++attempt) {
      for (const body of state.bodies) body.resolve(new ArrayBuffer(8));
      for (const decode of state.context.decodes) decode.resolve({ abandoned: true });
      for (const resume of state.context.resumes) resume.resolve();
      await Promise.resolve();
    }
    assert.ok(state.finished, 'stopped player completion');
    await state.completion;
  };
  return state;
}

async function firstRequest(f, id) {
  assert.equal(f.context.resumes.length, 1);
  f.context.resumes[0].resolve();
  await f.status({ clip_id: id, playing: true });
  await f.status({ clip_id: id, playing: true });
  return f.request('/audio/clip.wav?id=' + encodeURIComponent(id));
}

async function firstBody(f, id) {
  f.context.resumes[0].resolve();
  await f.status({ clip_id: id, playing: true });
  await f.status({ clip_id: id, playing: true });
  return f.fullBody(id);
}

async function decodeAndPlay(f, body, expectedSpeechCount) {
  const bytes = new ArrayBuffer(84);
  body.resolve(bytes);
  await until(() => f.context.decodes.length === expectedSpeechCount, 'decode finite WAV');
  const decoded = { id: expectedSpeechCount };
  const decode = f.context.decodes.at(-1);
  assert.equal(decode.bytes, bytes);
  decode.resolve(decoded);
  await until(() => f.context.resumes.length === expectedSpeechCount + 1, 'resume before scheduling');
  f.context.resumes.at(-1).resolve();
  await until(() => f.context.speech.length === expectedSpeechCount, 'speech source');
  const source = f.context.speech.at(-1);
  assert.equal(source.buffer, decoded);
  assert.equal(source.starts.length, 1);
  assert.equal(source.starts[0].offset, 0);
  assert.equal(source.connections[0], f.context.gain);
  return source;
}

const tests = [
  ['speech waits for initial resume, full body, decode and final resume', async () => {
    const f = fixture();
    assert.equal(f.context.speech.length, 0);
    assert.equal(f.requests.length, 0, 'fetch cannot outrun audio readiness');
    const body = await firstBody(f, '12');
    await settle();
    assert.equal(f.context.decodes.length, 0, 'partial HTTP body cannot decode');
    assert.equal(f.context.speech.length, 0);
    const bytes = new ArrayBuffer(128);
    f.context.currentTime = 50;
    body.resolve(bytes);
    await until(() => f.context.decodes.length === 1, 'pending decode');
    assert.equal(f.context.decodes[0].bytes, bytes);
    assert.equal(f.context.speech.length, 0, 'unfinished decoding cannot schedule');
    f.context.currentTime = 400;
    const decoded = {};
    f.context.decodes[0].resolve(decoded);
    await until(() => f.context.resumes.length === 2, 'pre-play resume');
    assert.equal(f.context.speech.length, 0, 'second resume is also awaited');
    f.context.currentTime = 900;
    f.context.resumes[1].resolve();
    await until(() => f.context.speech.length === 1, 'scheduled source');
    const source = f.context.speech[0];
    assert.equal(source.buffer, decoded);
    assert.equal(source.starts[0].offset, 0);
    assert.equal(source.starts[0].when, 900.1, 'lead is based on current time after all delays');
    assert.ok(source.starts[0].when > source.starts[0].clock);
    assert.equal(f.player.lastId, '12');
    await f.stop();
    assert.equal(f.error, undefined);
  }],
  ['Stop during initial resume prevents fetching or speech', async () => {
    const f = fixture();
    await f.stop();
    assert.equal(f.requests.length, 0);
    assert.equal(f.context.speech.length, 0);
    assert.equal(f.context.closed, true);
  }],
  ['Stop aborts an in-flight clip fetch without scheduling stale audio', async () => {
    const f = fixture();
    const request = await firstRequest(f, '21');
    assert.equal(request.settled, false);
    await f.stop();
    assert.equal(request.signal.aborted, true);
    assert.equal(f.context.decodes.length, 0);
    assert.equal(f.context.speech.length, 0);
    assert.equal(f.player.lastId, '0');
  }],
  ['Stop while the body is downloading never starts a late response', async () => {
    const f = fixture();
    const body = await firstBody(f, '22');
    assert.equal(body.settled, false);
    await f.stop();
    assert.equal(f.context.speech.length, 0);
    assert.equal(f.player.lastId, '0');
  }],
  ['Stop during decode rejects the stale decoded result', async () => {
    const f = fixture();
    const body = await firstBody(f, '23');
    body.resolve(new ArrayBuffer(72));
    await until(() => f.context.decodes.length === 1, 'decode pending at Stop');
    assert.equal(f.context.decodes[0].settled, false);
    await f.stop();
    assert.equal(f.context.speech.length, 0);
    assert.equal(f.context.resumes.length, 1, 'stale decoding never resumes the closed output');
    assert.equal(f.player.lastId, '0');
  }],
  ['409 refreshes the latest clip and preserves exact u64 string IDs', async () => {
    const f = fixture();
    const missing = '18446744073709551614';
    const latest = '18446744073709551615';
    const request = await firstRequest(f, missing);
    request.resolve({ ok: false, status: 409, arrayBuffer() { assert.fail('409 is not PCM'); } });
    await f.fire(100);
    assert.equal(f.player.lastId, '0', 'missing clip was not acknowledged as played');
    await f.status({ clip_id: latest, playing: true });
    const body = await f.fullBody(latest);
    const source = await decodeAndPlay(f, body, 1);
    assert.equal(f.player.lastId, latest);
    assert.equal(source.starts[0].offset, 0);
    assert.equal(f.context.speech.length, 1);
    await f.stop();
  }],
  ['one zero ConstantSource keeps the graph connected between utterances', async () => {
    const f = fixture();
    const body = await firstBody(f, '31');
    const first = await decodeAndPlay(f, body, 1);
    const silence = f.context.constants[0];
    assert.equal(f.context.constants.length, 1);
    assert.equal(silence.offset.value, 0);
    assert.equal(silence.starts, 1);
    assert.equal(silence.connections[0], f.context.gain);
    assert.equal(f.context.gain.connections[0], f.context.destination);
    assert.equal(f.context.gain.gain.value, .75);
    first.end();
    await f.status({ clip_id: '31', playing: false });
    await f.fire(200);
    assert.equal(first.disconnects, 1);
    assert.equal(silence.stops, 0, 'silence survives the inter-utterance gap');
    assert.equal(f.context.constants.length, 1);
    await f.status({ clip_id: '32', playing: true });
    const nextBody = await f.fullBody('32');
    const second = await decodeAndPlay(f, nextBody, 2);
    assert.equal(second.starts[0].offset, 0);
    assert.equal(silence.starts, 1);
    assert.equal(silence.stops, 0);
    await f.stop();
    assert.equal(silence.stops, 1);
    assert.equal(second.stops, 1);
    assert.equal(second.disconnects, 1);
  }],
  ['starting after an old clip ended waits for a new ID', async () => {
    const f = fixture();
    f.context.resumes[0].resolve();
    await f.status({ clip_id: '40', playing: false });
    await f.status({ clip_id: '40', playing: false });
    await f.fire(200);
    assert.equal(f.requests.filter(r => r.url.includes('/audio/clip.wav')).length, 0);
    assert.equal(f.player.lastId, '40');
    await f.status({ clip_id: '41', playing: true });
    const body = await f.fullBody('41');
    await decodeAndPlay(f, body, 1);
    assert.equal(f.player.lastId, '41');
    await f.stop();
  }],
];

const watchdog = setTimeout(() => {
  console.error('FAIL: deterministic player test did not finish');
  process.exit(1);
}, 5_000);
(async () => {
  for (const [name, run] of tests) {
    await run();
    console.log('ok - ' + name);
  }
  console.log(`${tests.length} production player regressions passed`);
})().catch(error => {
  console.error(error);
  process.exitCode = 1;
}).finally(() => clearTimeout(watchdog));
