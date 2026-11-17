#!/usr/bin/env node
'use strict';

// Exercise the actual player script with controlled network reads and clock.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');

const root = path.resolve(__dirname, '../..');
const html = fs.readFileSync(path.join(root, 'src/r/services/audio_http/player.html'), 'utf8');
const script = html.match(/<script>([\s\S]*?)<\/script>/)[1];

function deferred() {
  const result = {};
  result.promise = new Promise((resolve, reject) => Object.assign(result, {resolve, reject}));
  return result;
}

async function until(predicate, description) {
  for (let attempt = 0; attempt < 100; ++attempt) {
    if (predicate()) return;
    await Promise.resolve();
  }
  assert.fail('async boundary not reached: ' + description);
}

function wavHeader() {
  const header = Buffer.alloc(44);
  header.write('RIFF', 0);
  header.writeUInt32LE(0xffffffff, 4);
  header.write('WAVEfmt ', 8);
  header.writeUInt32LE(16, 16);
  header.writeUInt16LE(1, 20);
  header.writeUInt16LE(2, 22);
  header.writeUInt32LE(48000, 24);
  header.writeUInt32LE(192000, 28);
  header.writeUInt16LE(4, 32);
  header.writeUInt16LE(16, 34);
  header.write('data', 36);
  header.writeUInt32LE(0xffffffff - 36, 40);
  return header;
}

function pcm(left, right, endLeft = left) {
  const block = Buffer.alloc(1920);
  for (let offset = 0; offset < block.length; offset += 4) {
    block.writeInt16LE(left, offset);
    block.writeInt16LE(right, offset + 2);
  }
  block.writeInt16LE(endLeft, block.length - 4);
  return block;
}

function fixture() {
  const state = {requests: [], timers: [], reports: []};
  const resume = deferred();
  class AudioContext {
    constructor(options) {
      assert.equal(options.latencyHint, 'interactive');
      this.currentTime = 1;
      this.destination = {};
      this.sources = [];
      state.context = this;
    }
    createGain() {
      return this.gain = {gain: {value: NaN}, connect() {}};
    }
    createConstantSource() {
      return this.silence = {
        offset: {value: 1}, connect() {}, starts: 0, stops: 0,
        start() { ++this.starts; }, stop() { ++this.stops; },
      };
    }
    createBuffer(channels, frames, rate) {
      assert.equal(channels, 2);
      assert.equal(frames, 480);
      assert.equal(rate, 48000);
      const data = Array.from({length: channels}, () => new Float32Array(frames));
      return {getChannelData: channel => data[channel]};
    }
    createBufferSource() {
      const source = {
        starts: [], stops: 0, disconnects: 0,
        connect(target) { this.target = target; },
        start(when) { this.starts.push(when); },
        stop() { ++this.stops; },
        disconnect() { ++this.disconnects; },
      };
      this.sources.push(source);
      return source;
    }
    resume() { return resume.promise; }
    close() { this.closed = true; return Promise.resolve(); }
  }
  const elements = new Map(['listen', 'status', 'volume'].map(id => [id, {}]));
  const sandbox = vm.createContext({
    window: {AudioContext, addEventListener() {}},
    document: {getElementById: id => elements.get(id)},
    AbortController,
    fetch(url, options) {
      assert.equal(url, '/audio/live.wav');
      assert.equal(options.cache, 'no-store');
      const request = Object.assign(deferred(), {signal: options.signal, claimed: false});
      const abort = () => request.reject(new Error('fetch aborted'));
      request.signal.addEventListener('abort', abort, {once: true});
      request.promise.then(
        () => request.signal.removeEventListener('abort', abort),
        () => request.signal.removeEventListener('abort', abort),
      );
      state.requests.push(request);
      return request.promise;
    },
    setTimeout(callback, ms) {
      const timer = {callback, ms, active: true};
      state.timers.push(timer);
      return timer;
    },
    clearTimeout(timer) { timer.active = false; },
  });
  vm.runInContext(script, sandbox, {filename: 'production-player.html'});
  const LivePlayer = vm.runInContext('LivePlayer', sandbox);
  state.player = new LivePlayer(message => state.reports.push(message), .75);
  state.completion = state.player.start();
  state.resume = () => resume.resolve();
  state.request = async () => {
    await until(() => state.requests.some(request => !request.claimed), 'live request');
    const request = state.requests.find(request => !request.claimed);
    request.claimed = true;
    return request;
  };
  state.open = async () => {
    const request = await state.request();
    const stream = {reads: [], cancels: 0, releases: 0, closed: false};
    const reader = {
      read() {
        const read = deferred();
        const abort = () => read.reject(new Error('read aborted'));
        request.signal.addEventListener('abort', abort, {once: true});
        read.promise.then(
          () => request.signal.removeEventListener('abort', abort),
          () => request.signal.removeEventListener('abort', abort),
        );
        if (request.signal.aborted) abort();
        stream.reads.push(read);
        return read.promise;
      },
      cancel() { ++stream.cancels; return Promise.resolve(); },
      releaseLock() { ++stream.releases; },
    };
    request.resolve({ok: true, body: {getReader: () => reader}});
    let cursor = 0;
    stream.feed = async value => {
      await until(() => cursor < stream.reads.length, 'pending network read');
      const done = value === undefined;
      stream.closed ||= done;
      stream.reads[cursor++].resolve({value, done});
    };
    return stream;
  };
  state.fire = async ms => {
    await until(() => state.timers.some(t => t.active && t.ms === ms), 'timer ' + ms);
    const timer = state.timers.find(t => t.active && t.ms === ms);
    timer.active = false;
    timer.callback();
  };
  state.stop = async () => {
    state.player.stop();
    resume.resolve();
    await state.completion;
  };
  return state;
}

const tests = [
  ['fragmented headers and stereo frames play before the response finishes', async () => {
    const f = fixture();
    assert.equal(f.requests.length, 0, 'resume precedes the live fetch');
    f.resume();
    const stream = await f.open();
    const header = wavHeader();
    const a = pcm(-32768, 8192, 16384);
    const b = pcm(4096, -4096);
    await stream.feed(header.subarray(0, 9));
    await stream.feed(header.subarray(9, 35));
    await stream.feed(Buffer.concat([header.subarray(35), a.subarray(0, 3)]));
    await stream.feed(a.subarray(3, 1919));
    assert.equal(f.context.sources.length, 0, 'partial block is not played');
    await stream.feed(Buffer.concat([a.subarray(1919), b]));
    await until(() => f.context.sources.length === 2, 'two scheduled PCM blocks');
    const [first, second] = f.context.sources;
    assert.equal(stream.closed, false);
    assert.equal(f.reports[0], 'Listening');
    assert.equal(first.starts[0], 1.04);
    assert.ok(Math.abs(second.starts[0] - 1.05) < 1e-8);
    assert.equal(first.buffer.getChannelData(0)[0], -1);
    assert.equal(first.buffer.getChannelData(0)[479], .5);
    assert.equal(first.buffer.getChannelData(1)[0], .25);
    assert.equal(second.buffer.getChannelData(0)[0], .125);
    assert.equal(second.buffer.getChannelData(1)[0], -.125);
    assert.equal(first.target, f.context.gain);
    assert.equal(first.target.gain.value, .75);

    first.onended();
    assert.equal(first.disconnects, 1);
    await f.stop();
    assert.equal(first.stops, 0, 'completed nodes are released');
    assert.equal(second.stops, 1);
    assert.equal(second.disconnects, 1);
    assert.equal(f.context.silence.stops, 1);
    assert.equal(stream.cancels, 1);
    assert.equal(stream.releases, 1);
    assert.equal(f.context.closed, true);
  }],
  ['idle PCM advances the clock without allocating silent playback nodes', async () => {
    const f = fixture();
    f.resume();
    const stream = await f.open();
    await stream.feed(Buffer.concat([wavHeader(), pcm(0, 0), pcm(8000, 8000)]));
    await until(() => f.context.sources.length === 1, 'first non-silent block');
    assert.ok(Math.abs(f.context.sources[0].starts[0] - 1.05) < 1e-8);
    assert.equal(f.context.silence.starts, 1);
    await f.stop();
  }],
  ['bursts cannot schedule far ahead or cause another unbounded read', async () => {
    const f = fixture();
    f.resume();
    const stream = await f.open();
    const block = pcm(8000, -8000);
    await stream.feed(Buffer.concat([wavHeader(), ...Array(40).fill(block)]));
    await until(() => f.timers.some(t => t.active && t.ms === 10), 'scheduler backpressure');
    assert.equal(stream.reads.length, 1);
    assert.ok(f.context.sources.length < 17);
    for (const source of f.context.sources) assert.ok(source.starts[0] <= 1.16 + 1e-8);
    const before = f.context.sources.length;
    f.context.currentTime += .1;
    await f.fire(10);
    await until(() => f.context.sources.length > before, 'clock progress releases backpressure');
    for (const source of f.context.sources) {
      assert.ok(source.starts[0] <= f.context.currentTime + .16 + 1e-8);
    }
    await f.stop();
    assert.equal(stream.reads.length, 1);
    assert.ok(f.context.sources.every(source => source.stops === 1));
    assert.ok(!f.timers.some(t => t.active));
  }],
  ['invalid header and EOF reconnect with fresh framing on the same output graph', async () => {
    const f = fixture();
    f.resume();
    const old = await f.open();
    const badHeader = wavHeader();
    badHeader.writeUInt32LE(44100, 24);
    await old.feed(Buffer.concat([badHeader, pcm(8000, 8000)]));
    await f.fire(500);
    assert.equal(old.releases, 1);
    assert.equal(f.context.sources.length, 0);
    const ended = await f.open();
    await ended.feed(wavHeader());
    await ended.feed();
    await f.fire(500);
    assert.equal(ended.releases, 1);
    const replacement = await f.open();
    await replacement.feed(Buffer.concat([wavHeader(), pcm(4096, -4096)]));
    await until(() => f.context.sources.length === 1, 'replacement session PCM');
    assert.equal(f.context.sources[0].buffer.getChannelData(0)[0], .125);
    assert.equal(f.context.silence.starts, 1);
    assert.deepEqual(f.reports, ['Reconnecting…', 'Listening', 'Reconnecting…', 'Listening']);
    await f.stop();
  }],
  ['stop safely cancels pending resume or fetch', async () => {
    const starting = fixture();
    await starting.stop();
    assert.equal(starting.requests.length, 0);
    const connecting = fixture();
    connecting.resume();
    const request = await connecting.request();
    await connecting.stop();
    assert.equal(request.signal.aborted, true);
    assert.equal(connecting.context.sources.length, 0);
    assert.equal(connecting.context.closed, true);
  }],
];

(async () => {
  for (const [name, test] of tests) {
    await test();
    console.log('PASS', name);
  }
})().catch(error => { console.error(error); process.exitCode = 1; });
