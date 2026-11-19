#!/usr/bin/env python3
"""Exercise the production player in Firefox through its local WebDriver BiDi."""
import argparse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import shutil
import socket
import struct
import subprocess
import tempfile
import threading
import time
import urllib.parse
import urllib.request

ROOT = Path(__file__).resolve().parents[2]
STATE = {"id": "0", "prefix_sent_ms": {}, "tail_sent_ms": {}, "opens": 0}
BLOCK_FRAMES = 480
STEREO_SILENCE = bytes(BLOCK_FRAMES * 4)
SILENCE = bytes(4) + STEREO_SILENCE * 2


def pcm(left, right, second=None):
    slot_b = STEREO_SILENCE if second is None else struct.pack('<hh', *second) * BLOCK_FRAMES
    return (
        struct.pack('<HH', BLOCK_FRAMES, 0 if second is None else BLOCK_FRAMES)
        + struct.pack('<hh', left, right) * BLOCK_FRAMES
        + slot_b
    )


class Handler(BaseHTTPRequestHandler):
    def log_message(self, *args):
        pass

    def do_GET(self):
        target = urllib.parse.urlsplit(self.path)
        if target.path == '/':
            body = (ROOT / 'src/r/services/audio_http/player.html').read_bytes()
            content_type = 'text/html'
        elif target.path == '/fixture/publish':
            STATE['id'] = urllib.parse.parse_qs(target.query)['id'][0]
            body = b'{}'
            content_type = 'application/json'
        elif target.path == '/fixture/state':
            body = json.dumps(STATE).encode()
            content_type = 'application/json'
        elif target.path == '/audio/live.pcm':
            self.live_pcm()
            return
        else:
            self.send_error(404)
            return
        self.send_response(200)
        self.send_header('Content-Type', content_type)
        self.send_header('Content-Length', str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def live_pcm(self):
        STATE['opens'] += 1
        self.send_response(200)
        self.send_header('Content-Type', 'application/octet-stream')
        self.send_header('Cache-Control', 'no-store')
        self.send_header('Connection', 'close')
        self.end_headers()
        try:
            previous = '0'
            while True:
                spoken = STATE['id']
                if spoken == previous:
                    self.wfile.write(SILENCE)
                    time.sleep(.01)
                    continue

                previous = spoken
                amplitude = int(spoken) * 4096
                block = pcm(amplitude, -amplitude, (-4096, 4096) if spoken == '4' else None)
                # Hold the rest of the utterance back while the browser
                # schedules its prefix. For the stop case hold part of the
                # first block too, so a complete block arrives after Stop.
                split = 111 if spoken == '3' else len(block)
                self.wfile.write(block[:split])
                self.wfile.flush()
                STATE['prefix_sent_ms'][spoken] = time.time() * 1000
                time.sleep(.35)
                STATE['tail_sent_ms'][spoken] = time.time() * 1000
                self.wfile.write(block[split:])
                self.wfile.write(pcm(-amplitude, amplitude))
                self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError):
            pass


SCRIPT = r"""
const assert = require('node:assert/strict');
const [wsURL, base] = process.argv.slice(2);
const ws = new WebSocket(wsURL);
const pending = new Map();
let serial = 0;
ws.onmessage = event => {
  const x = JSON.parse(event.data);
  const p = pending.get(x.id);
  if (!p) return;
  pending.delete(x.id);
  x.type === 'error' ? p.reject(new Error(JSON.stringify(x))) : p.resolve(x.result);
};
const ready = new Promise((resolve, reject) => { ws.onopen = resolve; ws.onerror = reject; });
const command = (method, params = {}) => new Promise((resolve, reject) => {
  const id = ++serial;
  pending.set(id, {resolve, reject});
  ws.send(JSON.stringify({id, method, params}));
});
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
let context;

(async () => {
  await ready;
  await command('session.new', {capabilities: {alwaysMatch: {acceptInsecureCerts: true}}});
  ({context} = await command('browsingContext.create', {type:'tab'}));
  await command('browsingContext.navigate', {context, url: base, wait: 'complete'});
  async function evaluate(expression) {
    const x = await command('script.evaluate', {
      expression: 'JSON.stringify(' + expression + ')',
      target: {context},
      awaitPromise: true,
    });
    if (x.type !== 'success') throw Error(JSON.stringify(x));
    return JSON.parse(x.result.value);
  }
  async function until(test) {
    const end = Date.now() + 8000;
    while (Date.now() < end) {
      const result = await test();
      if (result) return result;
      await sleep(20);
    }
    throw Error('browser playback timeout');
  }
  const serverState = async () => (await fetch(base + 'fixture/state')).json();
  await evaluate(`(() => {
    window.starts = [];
    window.contexts = 0;
    const Original = window.AudioContext;
    window.AudioContext = function(options) {
      window.latencyHint = options.latencyHint;
      const ctx = new Original(options);
      window.contexts++;
      const gains = window.gains = [];
      const createGain = ctx.createGain.bind(ctx);
      ctx.createGain = () => {
        const gain = createGain();
        gains.push(gain);
        return gain;
      };
      const create = ctx.createBufferSource.bind(ctx);
      ctx.createBufferSource = () => {
        const node = create();
        const start = node.start.bind(node);
        const connect = node.connect.bind(node);
        let lane;
        node.connect = target => {
          lane = gains.indexOf(target);
          return connect(target);
        };
        node.start = when => {
          const left = node.buffer.getChannelData(0);
          const right = node.buffer.getChannelData(1);
          window.starts.push({
            when, lane, clock: ctx.currentTime, at: Date.now(),
            state: ctx.state, rate: node.buffer.sampleRate,
            length: left.length, left: left[0], right: right[0], last: left.at(-1),
          });
          return start(when);
        };
        return node;
      };
      return ctx;
    };
    document.getElementById('volume').value = '0';
    document.getElementById('listen').click();
    return true;
  })()`);
  await until(() => evaluate("document.getElementById('status').textContent === 'Listening'"));

  const prefixes = [];
  for (const id of ['1', '2']) {
    const amplitude = Number(id) * .125;
    await fetch(base + 'fixture/publish?id=' + id);
    const row = await until(() => evaluate(`window.starts.find(row => row.left === ${amplitude}) ?? null`));
    assert.equal(row.state, 'running');
    assert.equal(row.length / row.rate, .01);
    assert.equal(row.left, amplitude);
    assert.equal(row.last, amplitude);
    assert.equal(row.right, -amplitude);
    assert.equal(row.lane, 0, 'solo routes through master volume');
    const state = await until(async () => {
      const state = await serverState();
      return state.tail_sent_ms[id] && state;
    });
    const startMs = row.at + (row.when - row.clock) * 1000;
    assert(startMs < state.tail_sent_ms[id], 'speech prefix was not scheduled for playback before tail');
    prefixes.push({
      id, schedule_ms_after_prefix: Math.round(row.at - state.prefix_sent_ms[id]),
      planned_start_ms_after_prefix: Math.round(startMs - state.prefix_sent_ms[id]),
    });
    const tail = await until(() => evaluate(`window.starts.find(row => row.left === ${-amplitude}) ?? null`));
    assert.equal(tail.right, amplitude);
  }
  assert.equal(await evaluate('window.contexts'), 1);
  assert.equal(await evaluate('window.latencyHint'), 'interactive');
  assert.equal((await serverState()).opens, 1, 'listener reopened between replies');

  await fetch(base + 'fixture/publish?id=4');
  const pair = await until(() => evaluate(`(() => {
    const a = window.starts.find(row => row.lane === 1 && row.left === .5);
    const b = window.starts.find(row => row.lane === 2 && row.left === -.125);
    return a && b ? [a, b] : null;
  })()`));
  assert.equal(pair[0].when, pair[1].when, 'A and B start on the same audio clock');
  assert.deepEqual(await evaluate("window.gains.map(node => node.gain.value)"), [0, .5, .5]);
  await evaluate(`(() => {
    const slider = document.getElementById('crossfade');
    slider.value = '1';
    slider.dispatchEvent(new Event('input'));
    return true;
  })()`);
  assert.deepEqual(await evaluate("window.gains.map(node => node.gain.value)"), [0, 0, 1]);
  await until(() => evaluate("window.starts.find(row => row.left === -.5) ?? null"));

  const beforeStop = await evaluate('window.starts.length');
  await fetch(base + 'fixture/publish?id=3');
  await until(async () => (await serverState()).prefix_sent_ms['3']);
  await evaluate("(() => { document.getElementById('listen').click(); return true; })()");
  await sleep(500);
  assert.equal(await evaluate('window.starts.length'), beforeStop, 'stopped stream produced stale speech');
  console.log(JSON.stringify({
    browser: 'Firefox', prefixes, crossfade: 'passed', one_live_connection: true, stop_during_transfer: 'passed',
  }, null, 2));
  await command('browsingContext.close', {context});
  context = null;
  await command('session.end');
  ws.close();
})().catch(async error => {
  console.error(error.stack);
  try {
    if (context) await command('browsingContext.close', {context});
    await command('session.end');
  } catch {}
  ws.close();
  process.exitCode = 1;
});
"""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--ws',help='use an existing fresh Firefox BiDi endpoint instead of launching a private profile')
    args = parser.parse_args()
    if not shutil.which('node') or (not args.ws and not shutil.which('firefox')):
        print('SKIP: Firefox and Node are required for the real-browser test')
        return
    server = ThreadingHTTPServer(('127.0.0.1',0),Handler)
    thread = threading.Thread(target=server.serve_forever,daemon=True);thread.start()
    browser = None
    work = ROOT/'target/audio-http-prefix-tests'
    work.mkdir(parents=True,exist_ok=True)
    profile = tempfile.TemporaryDirectory(prefix='firefox-private-',dir=work)
    log = (work/'firefox-browser-test.log').open('w')
    try:
        endpoint = args.ws
        if not endpoint:
            with socket.socket() as probe:
                probe.bind(('127.0.0.1',0));port=probe.getsockname()[1]
            Path(profile.name,'user.js').write_text('user_pref("media.autoplay.default",0);\nuser_pref("media.autoplay.block-webaudio",false);\nuser_pref("browser.shell.checkDefaultBrowser",false);\n')
            browser = subprocess.Popen(['firefox','--headless','--no-remote','--profile',profile.name,
                                        '--remote-debugging-port',str(port),'about:blank'],stdout=log,stderr=subprocess.STDOUT)
            deadline = time.monotonic()+15
            while True:
                try:
                    with urllib.request.urlopen(f'http://127.0.0.1:{port}/',timeout=.5):
                        break
                except OSError:
                    if browser.poll() is not None or time.monotonic()>=deadline:
                        raise RuntimeError('Private Firefox failed to start; see firefox-browser-test.log')
                    time.sleep(.1)
            endpoint = f'ws://127.0.0.1:{port}/session'
        subprocess.run(['node','-',endpoint,f'http://127.0.0.1:{server.server_port}/'],input=SCRIPT,text=True,check=True,timeout=35)
    finally:
        server.shutdown();server.server_close()
        if browser is not None:
            browser.terminate()
            try:
                browser.wait(timeout=10)
            except subprocess.TimeoutExpired:
                browser.kill();browser.wait()
        log.close();profile.cleanup()


if __name__=='__main__':
    main()
