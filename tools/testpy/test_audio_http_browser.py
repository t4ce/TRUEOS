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
STATE = {"id": "0", "started": {}, "tail_sent_ms": {}}


def wav(identifier):
    amplitude = int(identifier)*4096
    pcm = b''.join(struct.pack('<hh', amplitude if frame<48000 else -amplitude,
                              -amplitude if frame<48000 else amplitude) for frame in range(96000))
    return struct.pack('<4sI4s4sIHHIIHH4sI',b'RIFF',len(pcm)+36,b'WAVE',b'fmt ',16,1,2,48000,192000,4,16,b'data',len(pcm))+pcm


class Handler(BaseHTTPRequestHandler):
    def log_message(self,*args):
        pass

    def do_GET(self):
        target = urllib.parse.urlsplit(self.path)
        if target.path == '/':
            body = (ROOT/'src/r/services/audio_http/player.html').read_bytes()
            content_type = 'text/html'
        elif target.path == '/fixture/publish':
            STATE['id'] = urllib.parse.parse_qs(target.query)['id'][0]
            body = b'{}'
            content_type = 'application/json'
        elif target.path == '/fixture/state':
            body = json.dumps(STATE).encode()
            content_type = 'application/json'
        elif target.path == '/audio/status':
            body = json.dumps({'clip_id':STATE['id'],'playing':STATE['id']!='0'}).encode()
            content_type = 'application/json'
        elif target.path == '/audio/clip.wav':
            identifier = urllib.parse.parse_qs(target.query)['id'][0]
            body = wav(identifier)
            STATE['started'][identifier] = True
            self.send_response(200)
            self.send_header('Content-Type','audio/wav')
            self.send_header('Content-Length',str(len(body)))
            self.end_headers()
            try:
                split = len(body)//2
                self.wfile.write(body[:split]);self.wfile.flush()
                time.sleep(.35)
                STATE['tail_sent_ms'][identifier] = time.time()*1000
                self.wfile.write(body[split:]);self.wfile.flush()
            except (BrokenPipeError,ConnectionResetError):
                pass
            return
        else:
            self.send_error(404);return
        self.send_response(200)
        self.send_header('Content-Type',content_type)
        self.send_header('Content-Length',str(len(body)))
        self.end_headers();self.wfile.write(body)


SCRIPT = r'''
const assert=require('node:assert/strict');
const [wsURL,base]=process.argv.slice(2);
const ws=new WebSocket(wsURL);const pending=new Map();let serial=0;
ws.onmessage=event=>{const x=JSON.parse(event.data),p=pending.get(x.id);if(!p)return;pending.delete(x.id);x.type==='error'?p.reject(new Error(JSON.stringify(x))):p.resolve(x.result)};
const ready=new Promise((resolve,reject)=>{ws.onopen=resolve;ws.onerror=reject});
const command=(method,params={})=>new Promise((resolve,reject)=>{const id=++serial;pending.set(id,{resolve,reject});ws.send(JSON.stringify({id,method,params}))});
const sleep=ms=>new Promise(r=>setTimeout(r,ms));
let context;
(async()=>{
 await ready;
 await command('session.new',{capabilities:{alwaysMatch:{acceptInsecureCerts:true}}});
 ({context}=await command('browsingContext.create',{type:'tab'}));
 await command('browsingContext.navigate',{context,url:base,wait:'complete'});
 async function evaluate(expression){const x=await command('script.evaluate',{expression:'JSON.stringify('+expression+')',target:{context},awaitPromise:true});if(x.type!=='success')throw Error(JSON.stringify(x));return JSON.parse(x.result.value)}
 async function until(test){const end=Date.now()+8000;while(Date.now()<end){const value=await test();if(value)return value;await sleep(30)}throw Error('browser playback timeout')}
 await evaluate(`(()=>{window.starts=[];window.contexts=0;const Original=window.AudioContext;window.AudioContext=function(options){const ctx=new Original(options);window.contexts++;const create=ctx.createBufferSource.bind(ctx);ctx.createBufferSource=()=>{const node=create();const start=node.start.bind(node);node.start=(when,offset)=>{const data=node.buffer.getChannelData(0);window.starts.push({when,offset,at:Date.now(),state:ctx.state,rate:node.buffer.sampleRate,length:data.length,first:data[0],prefixMin:Math.min(...data.slice(0,Math.floor(node.buffer.sampleRate*.9))),prefixMax:Math.max(...data.slice(0,Math.floor(node.buffer.sampleRate*.9))),last:data[data.length-1]});return start(when,offset)};return node};return ctx};document.getElementById('volume').value='0';document.getElementById('listen').click();return true})()`);
 await until(()=>evaluate(`document.getElementById('status').textContent==='Listening'`));
 for(const id of ['1','2']){
   await fetch(base+'fixture/publish?id='+id);
   await until(()=>evaluate(`window.starts.length===${id}`));
   const starts=await evaluate('window.starts'),row=starts[Number(id)-1];
   const state=await (await fetch(base+'fixture/state')).json();
   assert.equal(row.offset,0);assert.equal(row.state,'running');
   assert.equal(row.length/row.rate,2);
   // Browser PCM-to-float normalization may use 32767 or 32768; tolerate one
   // s16 step while requiring the entire opening interval to be present.
   for(const sample of [row.first,row.prefixMin,row.prefixMax]) assert(Math.abs(sample-Number(id)*.125)<1/32767);
   assert(Math.abs(row.last+Number(id)*.125)<1/32767);
   assert(row.at>=state.tail_sent_ms[id],'played before the complete WAV arrived');
   await until(()=>evaluate(`document.getElementById('status').textContent==='Listening'`));
   await sleep(150);
 }
 assert.equal(await evaluate('window.contexts'),1,'recreated output context between clips');
 await fetch(base+'fixture/publish?id=3');
 await until(async()=> (await (await fetch(base+'fixture/state')).json()).started['3']);
 await evaluate(`(()=>{document.getElementById('listen').click();return true})()`);
 await sleep(500);
 assert.equal(await evaluate('window.starts.length'),2,'stopped download produced stale speech');
 const result={browser:'Firefox',clips:await evaluate('window.starts'),one_output_context:true,stop_during_transfer:'passed'};
 console.log(JSON.stringify(result,null,2));
 await command('browsingContext.close',{context});context=null;
 await command('session.end');ws.close();
})().catch(async error=>{console.error(error.stack);try{if(context)await command('browsingContext.close',{context});await command('session.end')}catch{}ws.close();process.exitCode=1});
'''


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
