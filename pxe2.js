#!/usr/bin/env node
/*
  TRUEOS PXE (Router LAN) - ProxyDHCP + TFTP

  - Keeps your router (e.g. FRITZ!Box) as the only DHCP server for IP leases
  - This script only provides PXE boot info (ProxyDHCP) + TFTP service via dnsmasq
  - Does NOT modify interface IP configuration

  Usage:
    sudo node pxe2.js
    sudo node pxe2.js --iface <dev>
    sudo node pxe2.js --tftp-root ./bld

  Notes:
    - Requires UEFI x86_64 PXE clients (arch=7)
    - Ensure firewall allows UDP 67, 69, 4011 (plus TFTP data UDP ports)
*/

const fs = require("fs");
const http = require("http");
const path = require("path");
const { spawn, spawnSync } = require("child_process");

const BOOTFILE = "EFI/BOOT/BOOTX64.EFI";
const LEASES = "/tmp/trueos-pxe2.leases";
const DEFAULT_HTTP_PORT = 8080;
const HARDCODED_HTTP_ASSETS = [
  {
    kind: "video",
    label: "demo-mp4",
    filePath: "tools/vid/demo_yelly.mp4",
    contentType: "video/mp4",
  },
  {
    kind: "audio",
    label: "demo-wav",
    filePath: "tools/aud/demo.wav",
    contentType: "audio/wav",
  },
  {
    kind: "model",
    label: "gemma-4-e4b-it-q4-k-m",
    filePath: "tools/gemma-4-E4B-it-Q4_K_M.gguf",
    contentType: "application/octet-stream",
  },
];
 
// (UEFI x86_64 client arch code is 7 per RFC 4578.)
const UEFI_X86_64_ARCH = 7;

function die(msg) {
  process.stderr.write(String(msg).trimEnd() + "\n");
  process.exit(1);
}

function parseArgs(argv) {
  const out = {
    iface: null,
    tftpRoot: path.resolve(__dirname, "bld"),
    httpPort: DEFAULT_HTTP_PORT,
    enableHttp: true,
    dryRun: false,
    verbose: false,
  };

  for (let i = 2; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--iface" && i + 1 < argv.length) {
      out.iface = argv[++i];
    } else if (a === "--tftp-root" && i + 1 < argv.length) {
      out.tftpRoot = path.resolve(process.cwd(), argv[++i]);
    } else if (a === "--http-port" && i + 1 < argv.length) {
      out.httpPort = Number(argv[++i]);
    } else if (a === "--no-http") {
      out.enableHttp = false;
    } else if (a === "--dry-run") {
      out.dryRun = true;
    } else if (a === "--verbose") {
      out.verbose = true;
    } else if (a === "-h" || a === "--help") {
      process.stdout.write(
        "Usage: sudo node pxe2.js [--iface <dev>] [--tftp-root <path>] [--http-port <port>] [--no-http] [--dry-run] [--verbose]\n"
      );
      process.exit(0);
    } else {
      die(`Unknown arg: ${a}\nTry: --help`);
    }
  }
  return out;
}

function resolveHttpAssets() {
  return HARDCODED_HTTP_ASSETS.map((asset) => ({
    ...asset,
    absPath: path.resolve(__dirname, asset.filePath),
    urlPath: `/${asset.filePath}`,
  }));
}

function ensureHttpAssets() {
  const assets = resolveHttpAssets();
  return assets.map((asset) => {
    if (!fs.existsSync(asset.absPath)) {
      process.stdout.write(`Warning: HTTP ${asset.kind} file missing: ${asset.absPath}\n`);
      return { ...asset, ready: false };
    }
    const stat = fs.statSync(asset.absPath);
    if (!stat.isFile()) {
      process.stdout.write(`Warning: HTTP ${asset.kind} path is not a file: ${asset.absPath}\n`);
      return { ...asset, ready: false };
    }
    return { ...asset, ready: true };
  });
}

function startHttpServer({ httpPort, serverIp }) {
  const assets = resolveHttpAssets();
  const assetByUrlPath = new Map(assets.map((asset) => [asset.urlPath, asset]));
  const server = http.createServer((req, res) => {
    const method = req.method || "GET";
    if (method !== "GET" && method !== "HEAD") {
      res.writeHead(405, { "Content-Type": "text/plain; charset=utf-8" });
      res.end("method not allowed\n");
      return;
    }

    const target = req.url || "/";
    const requestPath = target.split("?")[0].split("#")[0];
    const asset = assetByUrlPath.get(requestPath);
    if (!asset) {
      res.writeHead(404, { "Content-Type": "text/plain; charset=utf-8" });
      res.end("not found\n");
      return;
    }

    fs.stat(asset.absPath, (statErr, stat) => {
      if (statErr || !stat.isFile()) {
        res.writeHead(404, { "Content-Type": "text/plain; charset=utf-8" });
        res.end(`missing media file: ${asset.absPath}\n`);
        return;
      }

      res.writeHead(200, {
        "Content-Type": asset.contentType,
        "Content-Length": stat.size,
        "Cache-Control": "no-store",
      });
      if (method === "HEAD") {
        res.end();
        return;
      }

      const stream = fs.createReadStream(asset.absPath);
      stream.on("error", () => {
        if (!res.headersSent) {
          res.writeHead(500, { "Content-Type": "text/plain; charset=utf-8" });
        }
        res.end("stream error\n");
      });
      stream.pipe(res);
    });
  });

  server.listen(httpPort, serverIp, () => {
    for (const asset of assets) {
      process.stdout.write(
        `HTTP media host kind=${asset.kind} file=${asset.absPath} url=http://${serverIp}:${httpPort}${asset.urlPath}\n`
      );
    }
  });
  server.on("error", (err) => {
    die(`HTTP server failed: ${err.message}`);
  });
  return server;
}

function runJson(cmd, args, label) {
  const r = spawnSync(cmd, args, { encoding: "utf8" });
  if (r.error) die(`${label}: ${r.error.message}`);
  if (r.status !== 0) {
    const stderr = (r.stderr || "").trim();
    die(`${label} failed (${r.status})${stderr ? `\n${stderr}` : ""}`);
  }
  const s = (r.stdout || "").trim();
  if (!s) die(`${label}: empty output`);
  try {
    return JSON.parse(s);
  } catch (e) {
    die(`${label}: invalid JSON output`);
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
    die("Could not detect default route interface (no default route)");
  }
  const dev = routes[0] && routes[0].dev;
  if (!dev) die("Could not detect default route interface (missing dev)");
  return dev;
}

function detectInterfaceIPv4(iface) {
  const addrs = runJson("ip", ["-j", "-4", "addr", "show", "dev", iface], "ip addr");
  if (!Array.isArray(addrs) || addrs.length === 0) {
    die(`No IPv4 address info for interface ${iface}`);
  }

  const info = addrs[0];
  const addrInfo = Array.isArray(info.addr_info) ? info.addr_info : [];
  const candidate = addrInfo.find((a) => a.family === "inet" && a.scope === "global" && a.local);
  if (!candidate) {
    die(`Interface ${iface} has no global IPv4 address (is it connected / DHCP ok?)`);
  }
  const ip = candidate.local;
  const prefix = Number(candidate.prefixlen);
  if (!ip || !Number.isFinite(prefix)) {
    die(`Failed to parse IPv4/prefix for interface ${iface}`);
  }
  return { ip, prefix };
}

function ensureTftpFiles(tftpRoot) {
  const bootPath = path.join(tftpRoot, BOOTFILE);
  const kernelPath = path.join(tftpRoot, "TRUEOS.elf");

  for (const p of [bootPath, kernelPath]) {
    if (!fs.existsSync(p)) {
      die(
        `Missing required TFTP file: ${p}\n` +
          `Hint: run \`make iso\` first to stage UEFI netboot files into ${tftpRoot}.`
      );
    }
  }

  const pxeLimineConf = path.join(tftpRoot, "EFI/BOOT/limine.conf");
  const isoBootLimineConf = path.join(tftpRoot, "iso-bootroot/limine.conf");

  if (fs.existsSync(isoBootLimineConf) && !fs.existsSync(pxeLimineConf)) {
    fs.mkdirSync(path.dirname(pxeLimineConf), { recursive: true });
    fs.copyFileSync(isoBootLimineConf, pxeLimineConf);
    process.stdout.write(
      `Staged PXE limine.conf at ${pxeLimineConf} from ${isoBootLimineConf}.\n`
    );
  }

  if (!fs.existsSync(pxeLimineConf)) {
    process.stdout.write(
      `Warning: PXE limine.conf not found at ${pxeLimineConf}; continuing because ${BOOTFILE} and TRUEOS.elf exist.\n`
    );
  }
}

function buildDnsmasqArgs({ iface, tftpRoot, serverIp, lanNetwork, lanNetmask }) {
  // ProxyDHCP: do not hand out leases, only PXE boot info.
  // We restrict to the vendor-class prefix the firmware actually sends.
  // Example observed: "PXEClient:Arch:00007:UNDI:..." (UEFI x86_64).
  return [
    "--no-daemon",
    "--port=0",
    `--interface=${iface}`,
    "--bind-interfaces",
    "--enable-tftp",
    `--tftp-root=${tftpRoot}`,

    // ProxyDHCP range uses network+netmask; dnsmasq will respond to PXE clients without leasing addresses.
    `--dhcp-range=${lanNetwork},proxy,${lanNetmask}`,

    // Tag UEFI x86_64 PXE clients by vendor-class prefix.
    // (This avoids depending on DHCP option 93 client-arch, which some firmwares omit.)
    "--dhcp-vendorclass=set:efi64,PXEClient:Arch:00007",

    // Secondary match for firmwares that do send option 93.
    `--dhcp-match=set:efi64,option:client-arch,${UEFI_X86_64_ARCH}`,

    // In proxy-DHCP mode, advertise exactly one PXE service and let the client
    // fetch its boot filename from that service. Mixing pxe-service and
    // dhcp-boot causes some UEFI firmwares to bounce between discovery paths.
    `--pxe-service=tag:efi64,x86-64_EFI,TRUEOS (UEFI),${BOOTFILE},${serverIp}`,
    "--dhcp-ignore=tag:!efi64",

    // Logging
    "--log-dhcp",

    // Keep a lease file path (mostly irrelevant in proxy mode, but dnsmasq may want it present)
    `--dhcp-leasefile=${LEASES}`,
  ];
}

(async () => {
  try {
    if (process.getuid && process.getuid() !== 0) {
      die("Run as root (needs to bind DHCP/TFTP/ProxyDHCP ports)");
    }

    const opts = parseArgs(process.argv);
    const iface = opts.iface || detectDefaultInterface();
    const tftpRoot = opts.tftpRoot;

    ensureTftpFiles(tftpRoot);
    let httpAssets = [];
    if (opts.enableHttp) {
      httpAssets = ensureHttpAssets();
      if (!Number.isInteger(opts.httpPort) || opts.httpPort < 1 || opts.httpPort > 65535) {
        die(`Invalid --http-port: ${opts.httpPort}`);
      }
    }

    const { ip: serverIp, prefix } = detectInterfaceIPv4(iface);
    const lanNetmask = prefixToNetmask(prefix);
    const lanNetwork = networkAddress(serverIp, prefix);

    const args = buildDnsmasqArgs({
      iface,
      tftpRoot,
      serverIp,
      lanNetwork,
      lanNetmask,
    });

    process.stdout.write(
      `TRUEOS PXE ProxyDHCP iface=${iface} ip=${serverIp}/${prefix} tftp=${tftpRoot} boot=${BOOTFILE}\n`
    );
    if (opts.enableHttp) {
      for (const asset of httpAssets) {
        process.stdout.write(
          `TRUEOS HTTP media host kind=${asset.kind} file=${asset.absPath} media=http://${serverIp}:${opts.httpPort}${asset.urlPath} ready=${asset.ready ? 1 : 0}\n`
        );
      }
    }

    if (opts.verbose) {
      process.stdout.write(
        [
          `lan-network=${lanNetwork} netmask=${lanNetmask}`,
          "Firewall (typical): allow UDP 67, 69, 4011 (+ TFTP data high UDP ports)",
          opts.enableHttp ? `Firewall (HTTP): allow TCP ${opts.httpPort}` : "HTTP media host disabled.",
          "Note: ProxyDHCP usually requires same L2/VLAN broadcast domain.",
          "",
          "dnsmasq argv:",
          ...args.map((a) => "  " + a),
          "",
        ].join("\n")
      );
    }

    if (opts.dryRun) {
      process.stdout.write("Dry-run: dnsmasq argv:\n" + args.map((a) => "  " + a).join("\n") + "\n");
      if (opts.enableHttp) {
        for (const asset of httpAssets) {
          process.stdout.write(
            `Dry-run: HTTP kind=${asset.kind} file=${asset.absPath} url=http://${serverIp}:${opts.httpPort}${asset.urlPath} ready=${asset.ready ? 1 : 0}\n`
          );
        }
      }
      process.exit(0);
    }

    if (opts.enableHttp) {
      startHttpServer({
        httpPort: opts.httpPort,
        serverIp,
      });
    }

    const child = spawn("dnsmasq", args, { stdio: "inherit" });
    child.on("exit", (code, signal) => {
      process.exitCode = code ?? (signal ? 1 : 0);
    });
  } catch (err) {
    die(String(err && err.stack ? err.stack : err));
  }
})();
