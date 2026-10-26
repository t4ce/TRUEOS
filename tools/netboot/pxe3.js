#!/usr/bin/env node
"use strict";

// IPv4 ProxyDHCP + TFTP for the native UEFI artifacts. No ISO or HTTP server.
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { spawn, spawnSync } = require("node:child_process");

const FILES = ["EFI/BOOT/BOOTX64.EFI", "TRUEOS.elf", "EFI/BOOT/tgl_guc_70.bin"];
const ROOT = path.resolve(__dirname, "../../tgt/usb-release");

function json(command, args) {
  const result = spawnSync(command, args, { encoding: "utf8" });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`${command}: ${result.stderr.trim()}`);
  return JSON.parse(result.stdout);
}

async function stopExisting() {
  const script = fs.realpathSync(__filename);
  for (const entry of fs.readdirSync("/proc")) {
    if (!/^\d+$/.test(entry) || Number(entry) === process.pid) continue;
    let matches = false;
    try {
      const argv = fs.readFileSync(`/proc/${entry}/cmdline`, "utf8").split("\0");
      if (!/^node(?:js)?$/.test(path.basename(argv[0])) || !argv[1] || argv.includes("--stop")) continue;
      const cwd = fs.readlinkSync(`/proc/${entry}/cwd`);
      matches = fs.realpathSync(path.resolve(cwd, argv[1])) === script;
    } catch { continue; } // Process exited, or belongs to an inaccessible user.
    if (!matches) continue;
    const pid = Number(entry);
    console.log(`Stopping this repository's PXE server (PID ${pid})`);
    process.kill(pid, "SIGTERM");
    for (let attempt = 0; attempt < 100; attempt++) {
      try { process.kill(pid, 0); } catch (error) {
        if (error.code === "ESRCH") break;
        throw error;
      }
      if (attempt === 99) throw new Error(`PXE process ${pid} did not stop`);
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
  }
}

async function main() {
  const args = process.argv.slice(2);
  if (args.length === 1 && args[0] === "--stop") return stopExisting();
  let readyFile;
  if (args[0] === "--ready-file") {
    args.shift();
    readyFile = args.shift();
    if (!readyFile) throw new Error("--ready-file requires a path");
  }
  if (args.includes("--help")) {
    console.log("Usage: sudo node tools/netboot/pxe3.js [interface] [artifact-directory]");
    console.log("Defaults: default-route interface, tgt/usb-release. Requires ip and dnsmasq.");
    console.log("Existing LAN DHCP supplies addresses; this serves only IPv4 PXE boot information and TFTP.");
    console.log("The Rust EFI loader uses firmware PXE/TFTP to fetch the remaining files.");
    console.log("--stop stops only PXE servers running this repository's pxe3.js.");
    return;
  }
  if (args.length > 2) throw new Error("Use --help for usage");
  const iface = args[0] || json("ip", ["-j", "-4", "route", "show", "default"])[0]?.dev;
  if (!iface) throw new Error("No default IPv4 interface; specify an interface explicitly");
  const address = json("ip", ["-j", "-4", "addr", "show", "dev", iface])
    .flatMap((entry) => entry.addr_info || [])
    .find((entry) => entry.family === "inet" && entry.scope === "global");
  if (!address) throw new Error(`No global IPv4 address on ${iface}`);
  const source = path.resolve(args[1] || ROOT);
  for (const name of FILES) {
    if (!fs.statSync(path.join(source, name)).isFile()) throw new Error(`Not a file: ${name}`);
  }

  // A private snapshot exposes exactly these three files, even if the staging
  // directory contains other artifacts. It also survives dnsmasq's privilege drop.
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "trueos-pxe3-"));
  const cleanup = () => fs.rmSync(root, { recursive: true, force: true });
  process.once("exit", cleanup);
  fs.chmodSync(root, 0o755);
  fs.mkdirSync(path.join(root, "EFI/BOOT"), { recursive: true, mode: 0o755 });
  for (const name of FILES) {
    fs.copyFileSync(path.join(source, name), path.join(root, name));
    fs.chmodSync(path.join(root, name), 0o644);
  }
  const ip = address.local;
  const dnsmasqArgs = [
    "--conf-file=/dev/null", "--no-daemon", "--port=0",
    `--interface=${iface}`, "--bind-interfaces",
    `--listen-address=${ip}`, `--no-dhcpv6-interface=${iface}`,
    `--dhcp-range=${ip},proxy`, "--dhcp-no-override",
    "--dhcp-match=set:efi64,option:client-arch,7",
    "--dhcp-match=set:efi64,option:client-arch,9",
    "--dhcp-ignore=tag:!efi64",
    `--dhcp-boot=tag:efi64,${FILES[0]},,${ip}`,
    `--pxe-service=tag:efi64,7,TRUEOS native UEFI,${FILES[0]},${ip}`,
    `--pxe-service=tag:efi64,9,TRUEOS native UEFI,${FILES[0]},${ip}`,
    `--enable-tftp=${iface}`, `--tftp-root=${root}`, "--log-dhcp",
  ];
  // Validate configuration before opening network sockets.
  const check = spawnSync("dnsmasq", ["--test", ...dnsmasqArgs], { encoding: "utf8" });
  if (check.error) throw check.error;
  if (check.status !== 0) throw new Error(check.stderr.trim());
  console.log(`TRUEOS IPv4 PXE on ${iface} (${ip}); artifacts: ${source}`);
  console.log("The Rust EFI loader fetches the kernel and firmware over PXE/TFTP.");
  const child = spawn("dnsmasq", dnsmasqArgs, { stdio: ["ignore", "inherit", "pipe"] });
  // dnsmasq announces the TFTP root after successfully binding its sockets.
  let output = "";
  child.stderr.on("data", (chunk) => {
    process.stderr.write(chunk);
    output = (output + chunk.toString()).slice(-16384);
    if (readyFile && output.includes(`TFTP root is ${root}`)) {
      fs.writeFileSync(readyFile, `${process.pid}\n`);
      readyFile = undefined;
    }
  });
  for (const signal of ["SIGINT", "SIGTERM"]) {
    process.on(signal, () => child.kill(signal));
  }
  child.once("error", (error) => {
    console.error(error.message);
    process.exitCode = 1;
    cleanup();
  });
  child.once("exit", (code, signal) => {
    process.exitCode = code ?? (signal === "SIGINT" || signal === "SIGTERM" ? 0 : 1);
    cleanup();
  });
}

main().catch((error) => {
  console.error(`pxe3: ${error.message}`);
  process.exitCode = 1;
});
