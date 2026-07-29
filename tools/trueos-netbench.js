#!/usr/bin/env node
"use strict";

// Unpaced two-connection peer for TRUEOS's deferred kernel TCP benchmark.
// The kernel connects twice and identifies each connection with:
//   TRUEOS-BENCH/1 RX <duration-ms>  (Node -> TRUEOS)
//   TRUEOS-BENCH/1 TX <duration-ms>  (TRUEOS -> Node)

const net = require("node:net");

function parseArgs(argv) {
  const options = {
    host: "0.0.0.0",
    port: 9651,
    maxDurationMs: 120_000,
    chunkBytes: 1024 * 1024,
  };

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    const value = argv[index + 1];
    if (arg === "--host" && value) {
      options.host = value;
      index += 1;
    } else if (arg === "--port" && value) {
      options.port = Number.parseInt(value, 10);
      index += 1;
    } else if (arg === "--chunk-mib" && value) {
      options.chunkBytes = Number.parseInt(value, 10) * 1024 * 1024;
      index += 1;
    } else if (arg === "--help" || arg === "-h") {
      console.log(
        "usage: node tools/trueos-netbench.js [--host 0.0.0.0] [--port 9651] [--chunk-mib 1]",
      );
      process.exit(0);
    } else {
      throw new Error(`unknown or incomplete argument: ${arg}`);
    }
  }

  if (
    !Number.isInteger(options.port) ||
    options.port < 1 ||
    options.port > 65535 ||
    !Number.isInteger(options.chunkBytes) ||
    options.chunkBytes < 64 * 1024
  ) {
    throw new Error("invalid port or chunk size");
  }
  return options;
}

function formatRate(bytes, elapsedMs) {
  const bitsPerSecond = (bytes * 8 * 1000) / Math.max(elapsedMs, 1);
  if (bitsPerSecond >= 1e9) {
    return `${(bitsPerSecond / 1e9).toFixed(3)} Gbit/s`;
  }
  return `${(bitsPerSecond / 1e6).toFixed(2)} Mbit/s`;
}

const options = parseArgs(process.argv.slice(2));
const downloadChunk = Buffer.allocUnsafe(options.chunkBytes);
for (let index = 0; index < downloadChunk.length; index += 1) {
  downloadChunk[index] = (index * 31 + 17) & 0xff;
}

const sessions = new Map();
let nextSessionId = 1;

function startSession(socket, role, durationMs, initialPayload) {
  const existing = sessions.get(role);
  if (existing && !existing.socket.destroyed) {
    socket.destroy(new Error(`role ${role} is already connected`));
    return;
  }

  const session = {
    id: nextSessionId++,
    role,
    socket,
    startedNs: process.hrtime.bigint(),
    lastNs: process.hrtime.bigint(),
    bytes: 0,
    lastBytes: 0,
    durationMs,
    timer: null,
    reportTimer: null,
    ended: false,
  };
  sessions.set(role, session);

  socket.setNoDelay(true);
  socket.setKeepAlive(true, 10_000);

  const account = (bytes) => {
    session.bytes += bytes;
  };

  if (role === "TX") {
    // TRUEOS calls this TX; Node is the receiving side and therefore provides
    // the authoritative upload wire-rate.
    if (initialPayload.length !== 0) {
      account(initialPayload.length);
    }
    socket.on("data", (data) => account(data.length));
    socket.resume();
  } else {
    // Reuse one immutable buffer and honor only the kernel/socket backpressure.
    const pump = () => {
      if (session.ended || socket.destroyed) return;
      while (socket.write(downloadChunk)) {
        account(downloadChunk.length);
      }
      account(downloadChunk.length);
    };
    socket.on("drain", pump);
    pump();
  }

  session.reportTimer = setInterval(() => {
    const now = process.hrtime.bigint();
    const elapsedMs = Number(now - session.lastNs) / 1e6;
    const delta = session.bytes - session.lastBytes;
    const totalMs = Number(now - session.startedNs) / 1e6;
    const label = role === "RX" ? "node->trueos" : "trueos->node";
    console.log(
      `[${label}] bytes=${session.bytes} instant=${formatRate(delta, elapsedMs)} average=${formatRate(session.bytes, totalMs)}`,
    );
    session.lastNs = now;
    session.lastBytes = session.bytes;
  }, 1000);
  session.reportTimer.unref();

  session.timer = setTimeout(() => {
    session.ended = true;
    socket.end();
  }, Math.min(durationMs, options.maxDurationMs));
  session.timer.unref();

  console.log(
    `session=${session.id} role=${role} peer=${socket.remoteAddress}:${socket.remotePort} duration_ms=${durationMs}`,
  );
}

function finishSession(session, reason) {
  if (session.ended && !sessions.has(session.role)) return;
  session.ended = true;
  clearTimeout(session.timer);
  clearInterval(session.reportTimer);
  sessions.delete(session.role);
  const elapsedMs = Number(process.hrtime.bigint() - session.startedNs) / 1e6;
  const label = session.role === "RX" ? "node->trueos" : "trueos->node";
  console.log(
    `[${label}] complete reason=${reason} bytes=${session.bytes} elapsed_ms=${elapsedMs.toFixed(0)} average=${formatRate(session.bytes, elapsedMs)}`,
  );
}

const server = net.createServer({ allowHalfOpen: false }, (socket) => {
  let handshake = Buffer.alloc(0);
  const onHandshakeData = (data) => {
    handshake = Buffer.concat([handshake, data]);
    if (handshake.length > 512) {
      socket.destroy(new Error("benchmark handshake too large"));
      return;
    }

    const newline = handshake.indexOf(0x0a);
    if (newline < 0) return;

    socket.off("data", onHandshakeData);
    const line = handshake.subarray(0, newline).toString("ascii").trim();
    const initialPayload = handshake.subarray(newline + 1);
    const match = /^TRUEOS-BENCH\/1 (RX|TX) ([0-9]+)$/.exec(line);
    if (!match) {
      socket.destroy(new Error(`bad benchmark handshake: ${line}`));
      return;
    }

    const durationMs = Number.parseInt(match[2], 10);
    if (!Number.isInteger(durationMs) || durationMs < 1000) {
      socket.destroy(new Error("invalid benchmark duration"));
      return;
    }
    startSession(socket, match[1], durationMs, initialPayload);
  };

  socket.on("data", onHandshakeData);
  socket.on("error", (error) => {
    const session = [...sessions.values()].find((item) => item.socket === socket);
    if (session) finishSession(session, `error:${error.code || error.message}`);
    else console.error(`unidentified connection error: ${error.message}`);
  });
  socket.on("close", () => {
    const session = [...sessions.values()].find((item) => item.socket === socket);
    if (session) finishSession(session, "close");
  });
});

server.listen(options.port, options.host, () => {
  const address = server.address();
  console.log(
    `TRUEOS netbench listening on ${address.address}:${address.port}; start this before booting TRUEOS`,
  );
});

function shutdown(signal) {
  console.log(`received ${signal}, shutting down`);
  for (const session of sessions.values()) {
    session.ended = true;
    session.socket.destroy();
  }
  server.close(() => process.exit(0));
}

process.on("SIGINT", () => shutdown("SIGINT"));
process.on("SIGTERM", () => shutdown("SIGTERM"));
