/**
 * driverdev — TRUEOS xHCI driver-development helpers
 *
 * Import path:  /qjs/dd/driverdev.mjs
 *
 * Bridges the four kernel globals injected by src/host_api.rs into a clean
 * typed API with helpers for descriptor decoding and completion checking.
 *
 * Usage:
 *   import { listDevices, getDescriptor, DESC } from "/qjs/dd/driverdev.mjs";
 *   const devs = listDevices();
 *   const raw  = getDescriptor(devs[0].handle, DESC.DEVICE, 0, 18);
 */

// ---------------------------------------------------------------------------
// Descriptor type constants (USB 2.0 §9.4, USB 3.x §9.4)
// ---------------------------------------------------------------------------
export const DESC = Object.freeze({
  DEVICE:            0x01,
  CONFIGURATION:     0x02,
  STRING:            0x03,
  INTERFACE:         0x04,
  ENDPOINT:          0x05,
  DEVICE_QUALIFIER:  0x06,
  OTHER_SPEED:       0x07,
  INTERFACE_POWER:   0x08,
  BOS:               0x0F,
  DEVICE_CAPABILITY: 0x10,
  HID:               0x21,
  HID_REPORT:        0x22,
  HID_PHYSICAL:      0x23,
  HUB:               0x29,
  SUPERSPEED_HUB:    0x2A,
  SS_ENDPOINT_COMP:  0x30,
});

// ---------------------------------------------------------------------------
// xHCI transfer completion codes (xHCI Rev 1.2 §6.4.5)
// ---------------------------------------------------------------------------
export const CC = Object.freeze({
  SUCCESS:             1,
  DATA_BUFFER_ERROR:   2,
  BABBLE:              3,
  USB_TRANSACTION:     4,
  TRB_ERROR:           5,
  STALL:               6,
  RESOURCE_ERROR:      7,
  BANDWIDTH_ERROR:     8,
  NO_SLOTS:            9,
  INVALID_STREAM_TYPE: 10,
  SLOT_NOT_ENABLED:    11,
  ENDPOINT_NOT_ENABLED:12,
  SHORT_PACKET:        13,
  RING_UNDERRUN:       14,
  RING_OVERRUN:        15,
  VF_RING_FULL:        16,
  PARAMETER_ERROR:     17,
  BANDWIDTH_OVERRUN:   18,
  CONTEXT_STATE_ERROR: 19,
  NO_PING_RESPONSE:    20,
  EVENT_RING_FULL:     21,
  INCOMPATIBLE_DEVICE: 22,
  MISSED_SERVICE:      23,
  COMMAND_RING_STOPPED:24,
  COMMAND_ABORTED:     25,
  STOPPED:             26,
  STOPPED_LENGTH_INVALID:27,
  STOPPED_SHORT_PACKET:28,
  MAX_EXIT_LATENCY:    29,
  ISOCH_BUFFER_OVERRUN:31,
  EVENT_LOST:          32,
  UNDEFINED:           33,
  INVALID_STREAM_ID:   34,
  SECONDARY_BANDWIDTH: 35,
  SPLIT_TRANSACTION:   36,
});

export const HID_REPORT_TYPE = Object.freeze({
  INPUT:   1,
  OUTPUT:  2,
  FEATURE: 3,
});

// ---------------------------------------------------------------------------
// handle encoding helpers
// ---------------------------------------------------------------------------
export function makeHandle(controllerId, slotId) {
  return ((controllerId & 0xFF) << 24) | (slotId & 0xFFFFFF);
}

export function handleControllerId(handle) {
  return (handle >>> 24) & 0xFF;
}

export function handleSlotId(handle) {
  return handle & 0xFFFFFF;
}

// ---------------------------------------------------------------------------
// listDevices()
// Returns an array of device summary objects:
//   { handle, controller_id, slot_id, port, kind, vid, pid }
// vid and pid are already lowercase hex strings (e.g. "046d").
// Returns [] if the kernel global is absent.
// ---------------------------------------------------------------------------
export function listDevices() {
  const raw = typeof __trueosXhciListDevices === "function"
    ? __trueosXhciListDevices()
    : null;
  if (typeof raw !== "string" || raw.length === 0) return [];
  try {
    return JSON.parse(raw);
  } catch (_) {
    return [];
  }
}

export function listControllers() {
  const raw = typeof __trueosXhciListControllers === "function"
    ? __trueosXhciListControllers()
    : null;
  if (typeof raw !== "string" || raw.length === 0) return [];
  try {
    return JSON.parse(raw);
  } catch (_) {
    return [];
  }
}

export function getControllerSnapshot(controllerId) {
  if (typeof __trueosXhciGetControllerSnapshot !== "function") return null;
  const raw = __trueosXhciGetControllerSnapshot(controllerId | 0);
  if (typeof raw !== "string" || raw.length === 0) return null;
  try {
    return JSON.parse(raw);
  } catch (_) {
    return null;
  }
}

export function requestProbe(controllerId) {
  if (typeof __trueosXhciRequestProbe !== "function") return -1;
  return __trueosXhciRequestProbe(controllerId | 0);
}

export function requestRebind(controllerId) {
  if (typeof __trueosXhciRequestRebind !== "function") return -1;
  return __trueosXhciRequestRebind(controllerId | 0);
}

// ---------------------------------------------------------------------------
// portReset(controllerId, portIdx)  → 0 | -1
// ---------------------------------------------------------------------------
export function portReset(controllerId, portIdx) {
  if (typeof __trueosXhciPortReset !== "function") return -1;
  return __trueosXhciPortReset(controllerId | 0, portIdx | 0);
}

// ---------------------------------------------------------------------------
// getDescriptor(handle, descType, descIndex = 0, length = 255)
// Returns a Uint8Array of the raw descriptor bytes, or null on failure.
// ---------------------------------------------------------------------------
export function getDescriptor(handle, descType, descIndex = 0, length = 255) {
  if (typeof __trueosXhciGetDescriptor !== "function") return null;
  const hex = __trueosXhciGetDescriptor(handle | 0, descType | 0, descIndex | 0, length | 0);
  return hexToBytes(hex);
}

// ---------------------------------------------------------------------------
// readTransferEvent(handle, epTarget)
// Returns { cc, residual } or null if no matching event is buffered.
// ---------------------------------------------------------------------------
export function readTransferEvent(handle, epTarget) {
  if (typeof __trueosXhciReadTransferEvent !== "function") return null;
  return __trueosXhciReadTransferEvent(handle | 0, epTarget | 0);
}

export function getHidDescriptor(handle, interfaceNumber = 0, length = 64) {
  if (typeof __trueosXhciGetHidDescriptor !== "function") return null;
  const hex = __trueosXhciGetHidDescriptor(
    handle | 0,
    interfaceNumber | 0,
    length | 0,
  );
  return hexToBytes(hex);
}

export function getHidReportDescriptor(handle, interfaceNumber = 0, length = 512) {
  if (typeof __trueosXhciGetHidReportDescriptor !== "function") return null;
  const hex = __trueosXhciGetHidReportDescriptor(
    handle | 0,
    interfaceNumber | 0,
    length | 0,
  );
  return hexToBytes(hex);
}

export function getHidProtocol(handle, interfaceNumber = 0) {
  if (typeof __trueosXhciHidGetProtocol !== "function") return -1;
  return __trueosXhciHidGetProtocol(handle | 0, interfaceNumber | 0);
}

export function setHidProtocol(handle, interfaceNumber = 0, protocol = 0) {
  if (typeof __trueosXhciHidSetProtocol !== "function") return -1;
  return __trueosXhciHidSetProtocol(handle | 0, interfaceNumber | 0, protocol | 0);
}

export function getHidIdle(handle, interfaceNumber = 0, reportId = 0) {
  if (typeof __trueosXhciHidGetIdle !== "function") return -1;
  return __trueosXhciHidGetIdle(handle | 0, interfaceNumber | 0, reportId | 0);
}

export function setHidIdle(handle, interfaceNumber = 0, reportId = 0, duration4ms = 0) {
  if (typeof __trueosXhciHidSetIdle !== "function") return -1;
  return __trueosXhciHidSetIdle(handle | 0, interfaceNumber | 0, reportId | 0, duration4ms | 0);
}

export function getHidReport(handle, interfaceNumber = 0, reportType = HID_REPORT_TYPE.INPUT, reportId = 0, length = 64) {
  if (typeof __trueosXhciHidGetReport !== "function") return null;
  const hex = __trueosXhciHidGetReport(
    handle | 0,
    interfaceNumber | 0,
    reportType | 0,
    reportId | 0,
    length | 0,
  );
  return hexToBytes(hex);
}

// HID class SET_REPORT request.
// reportType: 1=input, 2=output, 3=feature.
// payload can be a Uint8Array-like object or a lower-case hex string.
// Returns xHCI completion code (typically 1) or -1 on failure.
export function setHidReport(handle, interfaceNumber = 0, reportType = 2, reportId = 0, payload = "") {
  if (typeof __trueosXhciHidSetReport !== "function") return -1;
  const payloadHex = typeof payload === "string" ? payload : bytesToHex(payload);
  return __trueosXhciHidSetReport(
    handle | 0,
    interfaceNumber | 0,
    reportType | 0,
    reportId | 0,
    String(payloadHex || ""),
  );
}

export function sendLedOutputReport(handle, reportId = 0, payload = "") {
  if (typeof __trueosLedsSendOutputReport !== "function") return -1;
  const payloadHex = typeof payload === "string" ? payload : bytesToHex(payload);
  return __trueosLedsSendOutputReport(
    handle | 0,
    reportId | 0,
    String(payloadHex || ""),
  );
}

export function sendLedPreferredOutputReport(handle, payload = "") {
  if (typeof __trueosLedsSendPreferredOutputReport !== "function") return -1;
  const payloadHex = typeof payload === "string" ? payload : bytesToHex(payload);
  return __trueosLedsSendPreferredOutputReport(
    handle | 0,
    String(payloadHex || ""),
  );
}

// ---------------------------------------------------------------------------
// Higher-level helpers
// ---------------------------------------------------------------------------

/**
 * Fetch the 18-byte device descriptor and parse it into a structured object.
 * Returns parsed fields or null if the transfer fails.
 */
export function getDeviceDescriptor(handle) {
  const b = getDescriptor(handle, DESC.DEVICE, 0, 18);
  if (!b || b.length < 8) return null;
  return {
    bLength:            b[0],
    bDescriptorType:    b[1],
    bcdUSB:             b[2] | (b[3] << 8),
    bDeviceClass:       b[4],
    bDeviceSubClass:    b[5],
    bDeviceProtocol:    b[6],
    bMaxPacketSize0:    b[7],
    idVendor:           b.length >= 10 ? (b[8] | (b[9] << 8)) : 0,
    idProduct:          b.length >= 12 ? (b[10] | (b[11] << 8)) : 0,
    bcdDevice:          b.length >= 14 ? (b[12] | (b[13] << 8)) : 0,
    iManufacturer:      b.length >= 15 ? b[14] : 0,
    iProduct:           b.length >= 16 ? b[15] : 0,
    iSerialNumber:      b.length >= 17 ? b[16] : 0,
    bNumConfigurations: b.length >= 18 ? b[17] : 0,
  };
}

/**
 * Fetch a string descriptor (type 0x03) and decode it as UTF-16LE.
 * index = 0 returns the language-ID list as a Uint8Array instead.
 */
export function getString(handle, index, langId = 0x0409) {
  if (index === 0) {
    return getDescriptor(handle, DESC.STRING, 0, 4);
  }
  // First fetch with a small buffer to get bLength.
  const probe = getDescriptor(handle, DESC.STRING, index, 4);
  if (!probe || probe.length < 2) return null;
  const full = getDescriptor(handle, DESC.STRING, index, probe[0]);
  if (!full || full.length < 4) return null;
  // Decode UTF-16LE starting at offset 2.
  let out = "";
  for (let i = 2; i + 1 < full.length; i += 2) {
    out += String.fromCharCode(full[i] | (full[i + 1] << 8));
  }
  return out;
}

/**
 * Check whether a completion code indicates success.
 */
export function ccOk(cc) {
  return cc === CC.SUCCESS || cc === CC.SHORT_PACKET;
}

export function identifyHidDevice(handle, interfaceNumber = 0) {
  const report = getHidReportDescriptor(handle, interfaceNumber, 512);
  if (!report || report.length === 0) {
    return {
      kind: "unknown",
      reason: "hid_report_unavailable",
    };
  }

  const usage = findFirstApplicationUsage(report);
  const usagePage = usage ? usage.usagePage : null;
  const usageId = usage ? usage.usage : null;
  let kind = "unknown";

  if (usagePage === 0x01 && usageId === 0x02) {
    kind = "mouse";
  } else if (usagePage === 0x01 && usageId === 0x06) {
    kind = "keyboard";
  } else if (usagePage === 0x01 && usageId === 0x04) {
    kind = "joystick";
  } else if (usagePage === 0x01 && usageId === 0x05) {
    kind = "gamepad";
  } else if (usagePage === 0x0d) {
    kind = "digitizer";
  } else if (usagePage === 0x0c) {
    kind = "consumer_control";
  }

  return {
    kind,
    usagePage,
    usageId,
    interfaceNumber,
    reportLength: report.length,
  };
}

// ---------------------------------------------------------------------------
// Internal utilities
// ---------------------------------------------------------------------------
function hexToBytes(hex) {
  if (typeof hex !== "string" || hex.length === 0) return null;
  const len = hex.length >>> 1;
  const out = new Uint8Array(len);
  for (let i = 0; i < len; i++) {
    out[i] = parseInt(hex.slice(i * 2, i * 2 + 2), 16);
  }
  return out;
}

function findFirstApplicationUsage(report) {
  let i = 0;
  let usagePage = null;
  let usage = null;

  while (i < report.length) {
    const prefix = report[i++];
    if (prefix === 0xfe) {
      if (i + 1 >= report.length) break;
      const size = report[i++] | 0;
      i += 1;
      i += size;
      continue;
    }

    const sizeCode = prefix & 0x03;
    const size = sizeCode === 3 ? 4 : sizeCode;
    const itemType = (prefix >> 2) & 0x03;
    const itemTag = (prefix >> 4) & 0x0f;

    let value = 0;
    for (let n = 0; n < size && i < report.length; n += 1) {
      value |= (report[i++] & 0xff) << (8 * n);
    }

    if (itemType === 1 && itemTag === 0x0) {
      usagePage = value;
    } else if (itemType === 2 && itemTag === 0x0) {
      usage = value;
    } else if (itemType === 0 && itemTag === 0xa) {
      const collectionType = value & 0xff;
      if (collectionType === 0x01) {
        return {
          usagePage,
          usage,
        };
      }
    }
  }

  return null;
}

export default {
  DESC,
  CC,
  makeHandle,
  handleControllerId,
  handleSlotId,
  listControllers,
  listDevices,
  getControllerSnapshot,
  requestProbe,
  requestRebind,
  portReset,
  getDescriptor,
  readTransferEvent,
  getHidDescriptor,
  getHidReportDescriptor,
  getHidProtocol,
  setHidProtocol,
  getHidIdle,
  setHidIdle,
  getHidReport,
  setHidReport,
  sendLedOutputReport,
  sendLedPreferredOutputReport,
  getDeviceDescriptor,
  getString,
  identifyHidDevice,
  ccOk,
};
