#!/usr/bin/env node
const fs = require("fs");
const path = require("path");
const { spawn } = require("child_process");
const IFACE = "enx047bcb669593";
const SERVER_CIDR = "192.168.55.1/24";
const SERVER_IP = "192.168.55.1";
const DHCP_RANGE = "192.168.55.50,192.168.55.150,255.255.255.0,12h";
const TFTP_ROOT = "/home/t4ce/Repos/TRUEOS/bld";
const BOOTFILE = "EFI/BOOT/BOOTX64.EFI";
const LEASES = "/tmp/trueos-pxe.leases";

function run(cmd, args, label) {
  return new Promise((resolve, reject) => {
    const child = spawn(cmd, args, { stdio: "inherit" });
    child.on("exit", (code, signal) => {
      if (code === 0) resolve();
      else reject(new Error(`${label} failed (${signal || code})`));
    });
  });
}

function mustExist(filePath, what) {
  if (!fs.existsSync(filePath)) {
    throw new Error(`${what} not found: ${filePath}`);
  }
}

(async () => {
  try {
    if (typeof process.getuid === "function" && process.getuid() !== 0) {
      throw new Error("run this as root (sudo), required for ip + dnsmasq");
    }

    const bootPath = path.join(TFTP_ROOT, BOOTFILE);
    mustExist(bootPath, "PXE bootfile");

    await run("ip", ["link", "set", IFACE, "up"], "ip link set up");
    await run("ip", ["addr", "replace", SERVER_CIDR, "dev", IFACE], "ip addr replace");

    const args = [
      "--no-daemon",
      "--port=0",
      `--interface=${IFACE}`,
      "--bind-interfaces",
      "--dhcp-authoritative",
      `--dhcp-range=${DHCP_RANGE}`,
      `--dhcp-option=option:router,${SERVER_IP}`,
      `--dhcp-option=option:dns-server,${SERVER_IP}`,
      "--enable-tftp",
      `--tftp-root=${TFTP_ROOT}`,
      `--dhcp-boot=${BOOTFILE},,${SERVER_IP}`,
      "--log-dhcp",
      `--dhcp-leasefile=${LEASES}`,
    ];

    console.log("PXE ready:");
    console.log(`- iface: ${IFACE}`);
    console.log(`- server: ${SERVER_CIDR}`);
    console.log(`- tftp root: ${TFTP_ROOT}`);
    console.log(`- bootfile: ${BOOTFILE}`);
    console.log(`Starting: dnsmasq ${args.join(" ")}`);

    const child = spawn("dnsmasq", args, { stdio: "inherit" });
    child.on("exit", (code, signal) => {
      console.log(`dnsmasq exited (${signal || code})`);
    });
  } catch (err) {
    console.error(err && err.message ? err.message : String(err));
    process.exit(1);
  }
})();
