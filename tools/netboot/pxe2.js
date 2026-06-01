#!/usr/bin/env node
const dgram = require("dgram");
const fs = require("fs");
const path = require("path");

const { spawn, spawnSync } = require("child_process");

const REPO_ROOT = path.resolve(__dirname, "../..");
const BOOTFILE = "EFI/BOOT/BOOTX64.EFI";
const TFTP_ROOT = path.join(REPO_ROOT, "bld");
const TFTP_PORT = 69;

function runJson(cmd, args, label) {
  const r = spawnSync(cmd, args, { encoding: "utf8" });
  if (r.error) throw new Error(`${label}: ${r.error.message}`);
  if (r.status !== 0) {
    const stderr = (r.stderr || "").trim();
    throw new Error(`${label} failed (${r.status})${stderr ? `\n${stderr}` : ""}`);
  }
  const s = (r.stdout || "").trim();
  if (!s) throw new Error(`${label}: empty output`);
  try {
    return JSON.parse(s);
  } catch (e) {
    throw new Error(`${label}: invalid JSON output`);
  }
}

function ipv4ToInt(ip) {
  const parts = ip.split(".").map((x) => Number(x));
  if (parts.length !== 4 || parts.some((n) => !Number.isInteger(n) || n < 0 || n > 255)) {
    throw new Error(`Invalid IPv4 address: ${ip}`);
  }
  return (
    ((parts[0] << 24) >>> 0) |
    ((parts[1] << 16) >>> 0) |
    ((parts[2] << 8) >>> 0) |
    (parts[3] >>> 0)
  ) >>> 0;
}

function intToIpv4(n) {
  return [
    (n >>> 24) & 255,
    (n >>> 16) & 255,
    (n >>> 8) & 255,
    n & 255,
  ].join(".");
}

function prefixToNetmask(prefix) {
  if (!Number.isInteger(prefix) || prefix < 0 || prefix > 32) {
    throw new Error(`Invalid IPv4 prefix: ${prefix}`);
  }
  if (prefix === 0) return "0.0.0.0";
  const mask = (0xffffffff << (32 - prefix)) >>> 0;
  return intToIpv4(mask);
}

function networkAddress(ip, prefix) {
  const ipInt = ipv4ToInt(ip);
  const maskInt = ipv4ToInt(prefixToNetmask(prefix));
  return intToIpv4((ipInt & maskInt) >>> 0);
}

function detectDefaultInterface() {
  const routes = runJson("ip", ["-j", "route", "show", "default"], "ip route");
  if (!Array.isArray(routes) || routes.length === 0) {
    throw new Error("Could not detect default route interface (no default route)");
  }
  const dev = routes[0] && routes[0].dev;
  if (!dev) throw new Error("Could not detect default route interface (missing dev)");
  return dev;
}

function detectInterfaceIPv4(iface) {
  const addrs = runJson("ip", ["-j", "-4", "addr", "show", "dev", iface], "ip addr");
  if (!Array.isArray(addrs) || addrs.length === 0) {
    throw new Error(`No IPv4 address info for interface ${iface}`);
  }

  const info = addrs[0];
  const addrInfo = Array.isArray(info.addr_info) ? info.addr_info : [];
  const candidate = addrInfo.find((a) => a.family === "inet" && a.scope === "global" && a.local);
  if (!candidate) {
    throw new Error(`Interface ${iface} has no global IPv4 address (is it connected / DHCP ok?)`);
  }
  const ip = candidate.local;
  const prefix = Number(candidate.prefixlen);
  if (!ip || !Number.isFinite(prefix)) {
    throw new Error(`Failed to parse IPv4/prefix for interface ${iface}`);
  }
  return { ip, prefix };
}

function cstrs(buf, start) {
  return buf.subarray(start).toString("utf8").split("\0").filter(Boolean);
}

function u16(n) {
  const b = Buffer.alloc(2);
  b.writeUInt16BE(n & 0xffff, 0);
  return b;
}

function tftpError(socket, port, address, code, message) {
  const body = Buffer.from(message + "\0");
  socket.send(Buffer.concat([u16(5), u16(code), body]), port, address);
}

function tftpPath(name) {
  const clean = path.posix.normalize(name.replace(/^\/+/, ""));
  if (clean === "." || clean.startsWith("../") || path.posix.isAbsolute(clean)) {
    return null;
  }
  return path.join(TFTP_ROOT, clean);
}

function optionPacket(opts) {
  const parts = [u16(6)];
  for (const [key, value] of opts) {
    parts.push(Buffer.from(key + "\0" + value + "\0"));
  }
  return Buffer.concat(parts);
}

function startTftpServer(serverIp) {
  const listen = dgram.createSocket("udp4");
  listen.on("error", (err) => {
    process.stderr.write(`TFTP failed: ${err.message}\n`);
    process.exit(1);
  });

  listen.on("message", (msg, rinfo) => {
    if (msg.length < 4 || msg.readUInt16BE(0) !== 1) {
      return;
    }

    const fields = cstrs(msg, 2);
    const fileName = fields[0];
    const mode = (fields[1] || "").toLowerCase();
    if (!fileName || mode !== "octet") {
      tftpError(listen, rinfo.port, rinfo.address, 4, "unsupported request");
      return;
    }

    const filePath = tftpPath(fileName);
    if (!filePath) {
      tftpError(listen, rinfo.port, rinfo.address, 2, "bad path");
      return;
    }

    let file;
    try {
      file = fs.readFileSync(filePath);
    } catch {
      tftpError(listen, rinfo.port, rinfo.address, 1, "file not found");
      return;
    }

    const options = new Map();
    for (let i = 2; i + 1 < fields.length; i += 2) {
      options.set(fields[i].toLowerCase(), fields[i + 1]);
    }

    const blksize = Math.max(8, Math.min(Number(options.get("blksize")) || 512, 1468));
    const windowsize = Math.max(1, Math.min(Number(options.get("windowsize")) || 1, 16));
    const accepted = [];
    if (options.has("blksize")) accepted.push(["blksize", String(blksize)]);
    if (options.has("tsize")) accepted.push(["tsize", String(file.length)]);
    if (options.has("windowsize")) accepted.push(["windowsize", String(windowsize)]);

    const transfer = dgram.createSocket("udp4");
    transfer.on("error", close);
    let lastAck = accepted.length ? 0 : -1;
    let timer = null;
    let closed = false;
    const totalBlocks = Math.floor(file.length / blksize) + 1;

    function packet(block) {
      const start = (block - 1) * blksize;
      return Buffer.concat([u16(3), u16(block), file.subarray(start, start + blksize)]);
    }

    function close() {
      if (closed) return;
      closed = true;
      clearTimeout(timer);
      transfer.close();
    }

    function sendWindow() {
      clearTimeout(timer);
      for (let block = lastAck + 1; block <= Math.min(lastAck + windowsize, totalBlocks); block++) {
        transfer.send(packet(block), rinfo.port, rinfo.address);
      }
      if (lastAck >= totalBlocks) {
        close();
        return;
      }
      timer = setTimeout(sendWindow, 1500);
    }

    transfer.on("message", (ack, peer) => {
      if (peer.address !== rinfo.address || peer.port !== rinfo.port || ack.length < 4) {
        return;
      }
      const op = ack.readUInt16BE(0);
      if (op === 5) {
        close();
        return;
      }
      if (op !== 4) {
        return;
      }
      const block = ack.readUInt16BE(2);
      if (block === (lastAck & 0xffff)) {
        sendWindow();
        return;
      }
      lastAck += (block - (lastAck & 0xffff) + 0x10000) & 0xffff;
      sendWindow();
    });

    transfer.bind(0, serverIp, () => {
      if (accepted.length) {
        transfer.send(optionPacket(accepted), rinfo.port, rinfo.address);
      } else {
        lastAck = 0;
        sendWindow();
      }
      process.stdout.write(`tftp ${rinfo.address}:${rinfo.port} ${fileName} ${file.length} bytes\n`);
    });
  });

  listen.bind(TFTP_PORT, serverIp, () => {
    process.stdout.write(`TRUEOS TFTP ${serverIp}:${TFTP_PORT} root=${TFTP_ROOT}\n`);
  });
  return listen;
}

function buildDnsmasqArgs({ iface, serverIp, lanNetwork, lanNetmask }) {
  return [
    "--no-daemon",
    `--interface=${iface}`,
    "--bind-interfaces",
    `--dhcp-range=${lanNetwork},proxy,${lanNetmask}`,
    "--dhcp-vendorclass=set:efi64,PXEClient:Arch:00007",
    "--dhcp-match=set:efi64,option:client-arch,7",
    `--pxe-service=tag:efi64,x86-64_EFI,TRUEOS (UEFI),${BOOTFILE},${serverIp}`,
    "--dhcp-ignore=tag:!efi64",
    "--log-dhcp",
  ];
}

(async () => {
  try {
    const iface = detectDefaultInterface();
    const { ip: serverIp, prefix } = detectInterfaceIPv4(iface);
    const lanNetmask = prefixToNetmask(prefix);
    const lanNetwork = networkAddress(serverIp, prefix);
    const args = buildDnsmasqArgs({
      iface,
      serverIp,
      lanNetwork,
      lanNetmask,
    });

    process.stdout.write(
      `TRUEOS PXE ProxyDHCP iface=${iface} ip=${serverIp}/${prefix} tftp=${TFTP_ROOT} boot=${BOOTFILE}\n`
    );
    startTftpServer(serverIp);
    const child = spawn("dnsmasq", args, { stdio: "inherit" });
    child.on("exit", (code, signal) => {
      process.exitCode = code ?? (signal ? 1 : 0);
    });
  } catch (err) {
    process.stderr.write(String(err && err.stack ? err.stack : err).trimEnd() + "\n");
    process.exit(1);
  }
})();
