'use strict';

// Mount the Cloudbuild service inside the existing TRUEOS Express site.
// The upstream is operator configuration, never a browser-supplied URL.
const http = require('node:http');
const https = require('node:https');
const REQUEST_HEADERS = ['host', 'origin', 'accept', 'content-type', 'content-length', 'authorization'];
const RESPONSE_HEADERS = ['content-type', 'content-length', 'content-disposition', 'cache-control',
  'content-security-policy', 'x-content-type-options', 'referrer-policy'];

function mountCloudbuild(app, options = {}) {
  const configured = options.upstream || process.env.TRUEOS_CLOUDBUILD_URL;
  const upstream = configured || `http://127.0.0.1:${process.env.TRUEOS_CLOUDBUILD_PORT || '8787'}`;
  const ensureSelector = configured ? null : require('./cloudbuild-host.cjs').ensureSelector;
  const target = new URL(upstream);
  if (!['http:', 'https:'].includes(target.protocol) || target.username || target.password ||
      target.pathname !== '/' || target.search || target.hash) {
    throw new Error('TRUEOS_CLOUDBUILD_URL must be an HTTP(S) origin without credentials or a path');
  }
  const transport = target.protocol === 'https:' ? https : http;
  app.get(/^\/build$/, (_req, res) => res.redirect(308, '/build/'));
  app.use('/build', async (req, res) => {
    res.setHeader('Cache-Control', 'no-store');
    if (!['GET', 'POST'].includes(req.method)) {
      res.setHeader('Allow', 'GET, POST');
      return res.status(405).json({ error: 'Use GET or POST for Cloudbuild.' });
    }
    if (req.headers['transfer-encoding']) {
      return res.status(400).json({ error: 'Use a request with Content-Length.' });
    }
    const length = req.headers['content-length'];
    if (length !== undefined && (!/^\d+$/.test(length) || Number(length) > 32768)) {
      return res.status(413).json({ error: 'Cloudbuild request is too large.' });
    }
    if (ensureSelector) {
      try { await ensureSelector(); }
      catch {
        return res.status(503).json({ error: 'The selector could not start. The operator should check Python 3.11+ and the website Cloudbuild startup log.' });
      }
    }
    // Express removes /build from req.url. Host and Origin retain the public
    // trueos.eu identity so the Python service can enforce its configured origin.
    const headers = {};
    for (const name of REQUEST_HEADERS) {
      if (req.headers[name] !== undefined) headers[name] = req.headers[name];
    }
    const outgoing = transport.request({
      protocol: target.protocol, hostname: target.hostname, port: target.port || undefined,
      method: req.method, path: req.url, headers,
    }, (incoming) => {
      res.statusCode = incoming.statusCode;
      for (const name of RESPONSE_HEADERS) {
        if (incoming.headers[name] !== undefined) res.setHeader(name, incoming.headers[name]);
      }
      incoming.on('error', () => res.destroy());
      incoming.pipe(res);
    });
    outgoing.setTimeout(30000, () => outgoing.destroy(new Error('Cloudbuild upstream timed out')));
    outgoing.on('error', () => {
      if (res.headersSent) return res.destroy();
      res.status(503).json({ error: 'The build service is unavailable. Please try again later.' });
    });
    req.on('aborted', () => outgoing.destroy());
    res.on('close', () => outgoing.destroy());
    req.pipe(outgoing);
  });
}

module.exports = { mountCloudbuild };
