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

const BLOCK_BYTES = 1920;
const PACKET_BYTES = 4 + 2 * BLOCK_BYTES;
function stereo(left, right, endLeft = left) {
  const block = Buffer.alloc(BLOCK_BYTES);
  for (let offset = 0; offset < block.length; offset += 4) {
    block.writeInt16LE(left, offset);
    block.writeInt16LE(right, offset + 2);
  }
  block.writeInt16LE(endLeft, block.length - 4);
  return block;
}

function packet(a, b, framesA = a ? 480 : 0, framesB = b ? 480 : 0) {
  const block = Buffer.alloc(PACKET_BYTES);
  block.writeUInt16LE(framesA, 0);
  block.writeUInt16LE(framesB, 2);
  a?.copy(block, 4);
  b?.copy(block, 4 + BLOCK_BYTES);
  return block;
}

function pcm(left, right, endLeft = left) {
  return packet(stereo(left, right, endLeft), null);
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
      const gain = {
        gain: {value: NaN},
        connect(target) { this.target = target; },
        disconnect(target) {
          assert.equal(this.target, target);
          this.target = null;
        },
      };
      this.gain ??= gain;
      return gain;
    }
    createConstantSource() {
      return this.silence = {
        offset: {value: 1}, connect() {}, starts: 0, stops: 0,
        start() { ++this.starts; }, stop() { ++this.stops; },
      };
    }
    createMediaStreamDestination() {
      const track = {stopped: false, stop() { this.stopped = true; }};
      return {stream: {getTracks: () => [track]}};
    }
    createBuffer(channels, frames, rate) {
      assert.equal(channels, 2);
      assert.ok(frames > 0 && frames <= 480);
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
  const elements = new Map([
    'listen', 'status', 'volume', 'crossfade', 'low-gain-compat', 'compat-status',
  ].map(id => [id, {}]));
  state.compatOutput = {
    srcObject: null,
    plays: [],
    play() {
      const play = deferred();
      this.plays.push(play);
      return play.promise;
    },
    pause() {},
  };
  elements.set('compat-output', state.compatOutput);
  const sandbox = vm.createContext({
    window: {AudioContext, addEventListener() {}},
    document: {getElementById: id => elements.get(id)},
    AbortController,
    fetch(url, options) {
      assert.equal(url, '/audio/live.pcm');
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
  state.player = new LivePlayer(message => state.reports.push(message), .75, .5);
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
  ['fragmented packet counts and stereo frames play before the response finishes', async () => {
    const f = fixture();
    assert.equal(f.requests.length, 0, 'resume precedes the live fetch');
    f.resume();
    const stream = await f.open();
    const a = pcm(-32768, 8192, 16384);
    const b = pcm(4096, -4096);
    await stream.feed(a.subarray(0, 1));
    await stream.feed(a.subarray(1, 7));
    await stream.feed(a.subarray(7, PACKET_BYTES - 1));
    assert.equal(f.context.sources.length, 0, 'partial block is not played');
    await stream.feed(Buffer.concat([a.subarray(PACKET_BYTES - 1), b]));
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
    await stream.feed(Buffer.concat([packet(null, null), pcm(8000, 8000)]));
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
    await stream.feed(Buffer.concat(Array(40).fill(block)));
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
  ['invalid frame count and EOF reconnect with fresh framing on the same output graph', async () => {
    const f = fixture();
    f.resume();
    const old = await f.open();
    await old.feed(packet(null, null, 481, 0));
    await f.fire(500);
    assert.equal(old.releases, 1);
    assert.equal(f.context.sources.length, 0);
    const ended = await f.open();
    await ended.feed(packet(null, null));
    await ended.feed();
    await f.fire(500);
    assert.equal(ended.releases, 1);
    const replacement = await f.open();
    await replacement.feed(pcm(4096, -4096));
    await until(() => f.context.sources.length === 1, 'replacement session PCM');
    assert.equal(f.context.sources[0].buffer.getChannelData(0)[0], .125);
    assert.equal(f.context.silence.starts, 1);
    assert.deepEqual(f.reports, ['Reconnecting…', 'Listening', 'Reconnecting…', 'Listening']);
    await f.stop();
  }],
  ['crossfade is listener-local, keeps volume, and bypasses the fade for solo frames', async () => {
    const f = fixture();
    f.resume();
    const stream = await f.open();
    f.player.setCrossfade(1);
    await stream.feed(packet(stereo(8192, -8192), null));
    await until(() => f.context.sources.length === 1, 'slot A alone at the B endpoint');
    assert.equal(f.context.sources[0].target, f.player.gain);
    assert.equal(f.context.sources[0].buffer.getChannelData(0)[0], .25);

    f.player.setCrossfade(0);
    await stream.feed(packet(null, stereo(16384, -16384)));
    await until(() => f.context.sources.length === 2, 'slot B alone at the A endpoint');
    assert.equal(f.context.sources[1].target, f.player.gain);
    assert.equal(f.context.sources[1].buffer.getChannelData(0)[0], .5);

    await stream.feed(packet(stereo(8192, -8192), stereo(16384, -16384), 240, 480));
    await until(() => f.context.sources.length === 5, 'overlap and solo tail');
    const [a, b, tail] = f.context.sources.slice(2);
    assert.equal(a.target, f.player.lanes[0]);
    assert.equal(b.target, f.player.lanes[1]);
    assert.equal(a.target.target, f.player.gain);
    assert.equal(b.target.target, f.player.gain);
    assert.equal(a.starts[0], b.starts[0]);
    assert.equal(tail.target, f.player.gain);
    assert.ok(Math.abs(tail.starts[0] - b.starts[0] - .005) < 1e-8);
    assert.equal(tail.buffer.getChannelData(0).length, 240);
    f.player.setCrossfade(.3);
    assert.equal(a.target.gain.value, .7);
    assert.equal(b.target.gain.value, .3);
    assert.equal(f.player.gain.gain.value, .75);
    f.player.setCrossfade(1);
    assert.equal(a.target.gain.value, 0);
    assert.equal(b.target.gain.value, 1);
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
  ['native playback failure restores direct output and superseded requests leave the current route in place', async () => {
    const f = fixture();
    const blocked = f.player.setCompat(true);
    const blockedTrack = f.compatOutput.srcObject.getTracks()[0];
    f.compatOutput.plays[0].reject(new Error('playback refused'));
    await assert.rejects(blocked, /playback refused/);
    assert.equal(f.player.gain.target, f.context.destination);
    assert.equal(f.compatOutput.srcObject, null);
    assert.equal(blockedTrack.stopped, true);

    const superseded = f.player.setCompat(true);
    await f.player.setCompat(false);
    const current = f.player.setCompat(true);
    const destination = f.player.gain.target;
    f.compatOutput.plays[1].reject(new Error('superseded'));
    await superseded;
    assert.equal(f.player.gain.target, destination);
    assert.equal(f.compatOutput.srcObject, destination.stream);
    f.compatOutput.plays[2].resolve();
    await current;
    await f.stop();
    assert.equal(destination.stream.getTracks()[0].stopped, true);
    assert.equal(f.compatOutput.srcObject, null);
  }],
];

(async () => {
  for (const [name, test] of tests) {
    await test();
    console.log('PASS', name);
  }
})().catch(error => { console.error(error); process.exitCode = 1; });
