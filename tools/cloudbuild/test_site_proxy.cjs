'use strict';
// Run with Express available from the existing TRUEOS_SITE dependencies:
// TRUEOS_SITE_DIR=/path/to/TRUEOS_SITE node --test tools/cloudbuild/test_site_proxy.cjs
const test = require('node:test');
const assert = require('node:assert/strict');
const http = require('node:http');
const { createRequire } = require('node:module');
const path = require('node:path');
const express = process.env.TRUEOS_SITE_DIR
  ? createRequire(path.resolve(process.env.TRUEOS_SITE_DIR, 'package.json'))('express')
  : require('express');
const { mountCloudbuild } = require('./deploy/site-proxy.cjs');

async function listen(server) {
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  return `http://127.0.0.1:${server.address().port}`;
}
async function close(server) {
  server.closeAllConnections();
  await new Promise(resolve => server.close(resolve));
}

test('site routes, prefix stripping, private headers, streaming and failure handling', async () => {
  const received = [];
  const upstream = http.createServer((req, res) => {
    const chunks = [];
    req.on('data', chunk => chunks.push(chunk));
    req.on('end', () => {
      received.push({ method: req.method, url: req.url, headers: req.headers, body: Buffer.concat(chunks).toString() });
      res.setHeader('Cache-Control', 'no-store');
      if (req.url === '/api/jobs/fixture/artifact') {
        res.setHeader('Content-Disposition', 'attachment; filename="fixture.bin"');
        res.setHeader('Content-Type', 'application/octet-stream');
        res.write(Buffer.from([0, 1, 255]));
        return res.end(Buffer.from([2, 3]));
      }
      res.setHeader('Content-Type', 'application/json');
      res.end(JSON.stringify({ fixture: true }));
    });
  });
  const target = await listen(upstream);
  const app = express();
  app.get('/', (_req, res) => res.send('home'));
  app.get('/apps', (_req, res) => res.send('apps'));
  mountCloudbuild(app, { upstream: target });
  const website = http.createServer(app);
  const base = await listen(website);
  try {
    assert.equal(await (await fetch(base + '/')).text(), 'home');
    assert.equal(await (await fetch(base + '/apps')).text(), 'apps');
    const redirect = await fetch(base + '/build', { redirect: 'manual' });
    assert.equal(redirect.status, 308);
    assert.equal(redirect.headers.get('location'), '/build/');
    const result = await fetch(base + '/build/api/plan?fixture=1', {
      method: 'POST', body: '{"preset":"kernel-only"}',
      headers: { Origin: base, 'Content-Type': 'application/json',
        Authorization: 'Bearer FIXTURE-NOT-A-REAL-TOKEN', Cookie: 'website=fixture',
        'X-Forwarded-For': 'fixture-spoofed-address' },
    });
    assert.equal(result.status, 200);
    await result.arrayBuffer();
    const request = received.at(-1);
    assert.equal(request.url, '/api/plan?fixture=1');
    assert.equal(request.headers.host, new URL(base).host);
    assert.equal(request.headers.origin, base);
    assert.equal(request.headers.authorization, 'Bearer FIXTURE-NOT-A-REAL-TOKEN');
    assert.equal(request.headers.cookie, undefined);
    assert.equal(request.headers['x-forwarded-for'], undefined);
    assert.equal(request.body, '{"preset":"kernel-only"}');
    const artifact = await fetch(base + '/build/api/jobs/fixture/artifact');
    assert.deepEqual(Buffer.from(await artifact.arrayBuffer()), Buffer.from([0, 1, 255, 2, 3]));
    assert.equal(artifact.headers.get('content-disposition'), 'attachment; filename="fixture.bin"');
    const before = received.length;
    const oversized = await fetch(base + '/build/api/plan', { method: 'POST', body: 'x'.repeat(32769) });
    assert.equal(oversized.status, 413);
    assert.equal(received.length, before);
    const forbiddenMethod = await fetch(base + '/build/api/builds', { method: 'DELETE' });
    assert.equal(forbiddenMethod.status, 405);
    await close(upstream);
    const unavailable = await fetch(base + '/build/api/catalog');
    assert.equal(unavailable.status, 503);
    assert.equal(unavailable.headers.get('cache-control'), 'no-store');
    assert.equal((await unavailable.json()).error.includes('127.0.0.1'), false);
  } finally {
    if (upstream.listening) await close(upstream);
    await close(website);
  }
});

test('operator upstream rejects credentials, paths and non-HTTP protocols', () => {
  for (const upstream of ['file:///tmp/fixture', 'http://person:fixture@localhost', 'http://localhost/api']) {
    assert.throws(() => mountCloudbuild(express(), { upstream }));
  }
});
