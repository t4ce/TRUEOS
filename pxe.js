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

// (UEFI x86_64 client arch code is 7 per RFC 4578.)
const UEFI_X86_64_ARCH = 7;

function run(cmd, args, label) {
  return new Promise((resolve, reject) => {
    const child = spawn(cmd, args, { stdio: "inherit" });
    child.on("exit", (code, signal) => {
      if (code === 0) resolve();
      else reject(new Error(`${label} failed (${signal || code})`));
    });
  });
}

(async () => {
  try {
    if (process.getuid && process.getuid() !== 0) {
      throw new Error("Run as root (needs ip link/ip addr and binds DHCP/TFTP ports)");
    }

    const bootPath = path.join(TFTP_ROOT, BOOTFILE);
    const limineConfPath = path.join(TFTP_ROOT, "EFI/BOOT/limine.conf");
    const kernelPath = path.join(TFTP_ROOT, "TRUEOS.elf");

    for (const p of [bootPath, limineConfPath, kernelPath]) {
      if (!fs.existsSync(p)) {
        throw new Error(
          `Missing required TFTP file: ${p}\n` +
            `Hint: run \`make iso\` first to stage UEFI netboot files into ${TFTP_ROOT}.`
        );
      }
    }

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
      `--dhcp-match=set:efi64,option:client-arch,${UEFI_X86_64_ARCH}`,
      `--dhcp-boot=tag:efi64,${BOOTFILE},,${SERVER_IP}`,
      "--dhcp-ignore=tag:!efi64",
      "--log-dhcp",
      `--dhcp-leasefile=${LEASES}`,
    ];
    const child = spawn("dnsmasq", args, { stdio: "inherit" });
    child.on("exit", (code, signal) => {
      process.exitCode = code ?? (signal ? 1 : 0);
    });
  } catch (err) {
    console.error(String(err && err.stack ? err.stack : err));
    process.exitCode = 1;
  }
})();
