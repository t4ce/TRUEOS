import * as driverdev from '/qjs/dd/driverdev.mjs';
import { normalizeJsonFileTree } from './ai_file_adapter.mjs';

const SUPPORTED_SHELL_COMMANDS = Object.freeze(new Set([
  'acpi',
  'email',
  'etc',
  'file',
  'inteldev',
  'net',
  'probe',
  'set',
  'smp',
  'tlb',
  'turbo',
]));

const SHELL_COMMAND_DESCRIPTION_OVERRIDES = Object.freeze({
  acpi: 'Run the shell1 acpi command for power actions like shutdown, reboot, or sleep.',
  email: 'Run the shell1 email command.',
  etc: 'Run the shell1 etc command.',
  file: 'Run the shell1 file command for filesystem listing and file-related tasks.',
  inteldev: 'Run the shell1 inteldev command for live Intel GPU debug and bring-up control.',
  net: 'Run the shell1 net command for network status and probes.',
  probe: 'Run the shell1 probe command for hardware and bus inspection.',
  set: 'Run the shell1 set command for terminal sizing.',
  smp: 'Run the shell1 smp command.',
  tlb: 'Run the shell1 tlb table inspection command.',
  turbo: 'Run the shell1 turbo command.',
});

const DRIVERDEV_TOOLS = Object.freeze([
  Object.freeze({
    toolName: 'driverdev_list_xhci_controllers',
    description: 'List visible TRUEOS xHCI controllers with live runtime status.',
    parameters: Object.freeze({
      type: 'object',
      properties: Object.freeze({}),
      required: Object.freeze([]),
      additionalProperties: false,
    }),
  }),
  Object.freeze({
    toolName: 'driverdev_get_xhci_controller_snapshot',
    description: 'Inspect one xHCI controller: runtime state, MMIO registers, per-port state, cached devices, and topology.',
    parameters: Object.freeze({
      type: 'object',
      properties: Object.freeze({
        controller_id: Object.freeze({ type: 'integer', description: 'xHCI controller id.' }),
      }),
      required: Object.freeze(['controller_id']),
      additionalProperties: false,
    }),
  }),
  Object.freeze({
    toolName: 'driverdev_request_xhci_probe',
    description: 'Ask the running crabusb service to reprobe one xHCI controller.',
    parameters: Object.freeze({
      type: 'object',
      properties: Object.freeze({
        controller_id: Object.freeze({ type: 'integer', description: 'xHCI controller id.' }),
      }),
      required: Object.freeze(['controller_id']),
      additionalProperties: false,
    }),
  }),
  Object.freeze({
    toolName: 'driverdev_request_xhci_rebind',
    description: 'Force the running crabusb service to rebind one xHCI controller.',
    parameters: Object.freeze({
      type: 'object',
      properties: Object.freeze({
        controller_id: Object.freeze({ type: 'integer', description: 'xHCI controller id.' }),
      }),
      required: Object.freeze(['controller_id']),
      additionalProperties: false,
    }),
  }),
  Object.freeze({
    toolName: 'driverdev_list_devices',
    description: 'List visible TRUEOS xHCI devices.',
    parameters: Object.freeze({
      type: 'object',
      properties: Object.freeze({}),
      required: Object.freeze([]),
      additionalProperties: false,
    }),
  }),
  Object.freeze({
    toolName: 'driverdev_get_device_descriptor',
    description: 'Read and decode the USB device descriptor for a device handle.',
    parameters: Object.freeze({
      type: 'object',
      properties: Object.freeze({
        handle: Object.freeze({
          type: 'integer',
          description: 'Encoded TRUEOS xHCI device handle.',
        }),
      }),
      required: Object.freeze(['handle']),
      additionalProperties: false,
    }),
  }),
  Object.freeze({
    toolName: 'driverdev_get_descriptor',
    description: 'Read a raw USB descriptor from a device handle.',
    parameters: Object.freeze({
      type: 'object',
      properties: Object.freeze({
        handle: Object.freeze({ type: 'integer', description: 'Encoded TRUEOS xHCI device handle.' }),
        desc_type: Object.freeze({ type: 'integer', description: 'USB descriptor type number.' }),
        desc_index: Object.freeze({ type: 'integer', description: 'Descriptor index. Defaults to 0.' }),
        length: Object.freeze({ type: 'integer', description: 'Maximum read length. Defaults to 255.' }),
      }),
      required: Object.freeze(['handle', 'desc_type']),
      additionalProperties: false,
    }),
  }),
  Object.freeze({
    toolName: 'driverdev_get_hid_report_descriptor',
    description: 'Read the HID report descriptor for a device handle.',
    parameters: Object.freeze({
      type: 'object',
      properties: Object.freeze({
        handle: Object.freeze({ type: 'integer', description: 'Encoded TRUEOS xHCI device handle.' }),
        interface_number: Object.freeze({ type: 'integer', description: 'HID interface number. Defaults to 0.' }),
        length: Object.freeze({ type: 'integer', description: 'Maximum read length. Defaults to 512.' }),
      }),
      required: Object.freeze(['handle']),
      additionalProperties: false,
    }),
  }),
  Object.freeze({
    toolName: 'driverdev_port_reset',
    description: 'Request an xHCI port reset.',
    parameters: Object.freeze({
      type: 'object',
      properties: Object.freeze({
        controller_id: Object.freeze({ type: 'integer', description: 'xHCI controller id.' }),
        port_idx: Object.freeze({ type: 'integer', description: 'Port index to reset.' }),
      }),
      required: Object.freeze(['controller_id', 'port_idx']),
      additionalProperties: false,
    }),
  }),
  Object.freeze({
    toolName: 'driverdev_read_transfer_event',
    description: 'Read one buffered xHCI transfer completion event for a device handle and endpoint target.',
    parameters: Object.freeze({
      type: 'object',
      properties: Object.freeze({
        handle: Object.freeze({ type: 'integer', description: 'Encoded TRUEOS xHCI device handle.' }),
        ep_target: Object.freeze({ type: 'integer', description: 'xHCI endpoint target / DCI.' }),
      }),
      required: Object.freeze(['handle', 'ep_target']),
      additionalProperties: false,
    }),
  }),
]);

const FILE_ADAPTER_TOOLS = Object.freeze([
  Object.freeze({
    toolName: 'file_adapter_get_primary_tree',
    description: 'Read the custom TRUEOS primary filesystem tree adapter as compact JSON for live file inspection.',
    parameters: Object.freeze({
      type: 'object',
      properties: Object.freeze({
        max_entries: Object.freeze({
          type: 'integer',
          description: 'Maximum number of filesystem entries to include. Defaults to 100.',
        }),
      }),
      required: Object.freeze([]),
      additionalProperties: false,
    }),
  }),
]);

export const AI_TOOL_PROFILE_ALL = 'all';
export const AI_TOOL_PROFILE_NORMAL = 'normal';
export const AI_TOOL_PROFILE_INTELDEV = 'inteldev';
export const AI_TOOL_PROFILE_DRIVERDEV = 'driverdev';

function cloneToolParameters(parameters) {
  if (!parameters || typeof parameters !== 'object') {
    return null;
  }
  try {
    return Object.freeze(JSON.parse(JSON.stringify(parameters)));
  } catch {
    return null;
  }
}

function quoteArg(value) {
  const text = String(value ?? '');
  return `"${text.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`;
}

function appendToken(tokens, value) {
  const text = String(value ?? '').trim();
  if (text) {
    tokens.push(text);
  }
}

function appendKeyValue(tokens, key, value) {
  if (value === null || value === undefined) {
    return;
  }
  const text = String(value).trim();
  if (!text) {
    return;
  }
  tokens.push(`${key}=${text}`);
}

function readShellRuntimeCommands() {
  const runtime = globalThis.__trueosShell1Runtime;
  if (!runtime || typeof runtime !== 'object' || !Array.isArray(runtime.commands)) {
    return [];
  }

  const seen = new Set();
  const out = [];
  for (const entry of runtime.commands) {
    const command = String(entry?.command || entry?.name || '').trim();
    if (!command || seen.has(command) || !SUPPORTED_SHELL_COMMANDS.has(command)) {
      continue;
    }
    seen.add(command);
    const tool = entry?.tool && typeof entry.tool === 'object'
      ? Object.freeze({
        description: String(entry.tool.description || '').trim(),
        parameters: cloneToolParameters(entry.tool.parameters),
      })
      : null;
    if (!tool?.parameters) {
      continue;
    }
    out.push(Object.freeze({
      command,
      toolName: `shell_${command.replace(/[^a-z0-9]+/gi, '_').replace(/^_+|_+$/g, '').toLowerCase() || 'command'}`,
      mode: String(entry?.mode || 'cmd').trim() || 'cmd',
      description: tool?.description
        || SHELL_COMMAND_DESCRIPTION_OVERRIDES[command]
        || `Run the shell1 ${command} command. Prefer the structured tool parameters when available.`,
      parameters: tool.parameters,
    }));
  }
  return out;
}

function getShellCommands() {
  return Object.freeze(readShellRuntimeCommands());
}

function getInteldevShellCommand() {
  return getShellCommands().find((entry) => entry.command === 'inteldev') || null;
}

function shellToolBundle() {
  return getShellCommands()
    .filter((entry) => entry.command !== 'inteldev')
    .map((entry) => ({
    type: 'function',
    name: entry.toolName,
    description: entry.description,
    parameters: entry.parameters,
  }));
}

function intelToolBundle() {
  const inteldev = getInteldevShellCommand();
  if (!inteldev) {
    return [];
  }
  return [{
    type: 'function',
    name: 'intel_adapter',
    description: 'Live Intel GPU adapter for MMIO, ring, GuC, HuC, render, and media bring-up actions. Prefer this for Intel hardware work.',
    parameters: inteldev.parameters,
  }];
}

export function buildAiPcShellToolBundle() {
  return Object.freeze(shellToolBundle());
}

export function buildAiPcIntelToolBundle() {
  return Object.freeze(intelToolBundle());
}

export function buildAiPcDriverdevToolBundle() {
  return Object.freeze(
    DRIVERDEV_TOOLS.map((entry) => ({
      type: 'function',
      name: entry.toolName,
      description: entry.description,
      parameters: entry.parameters,
    })),
  );
}

export function buildAiPcFileToolBundle() {
  return Object.freeze(
    FILE_ADAPTER_TOOLS.map((entry) => ({
      type: 'function',
      name: entry.toolName,
      description: entry.description,
      parameters: entry.parameters,
    })),
  );
}

export function buildAiPcToolBundle(profile = AI_TOOL_PROFILE_ALL) {
  switch (String(profile || AI_TOOL_PROFILE_ALL)) {
    case AI_TOOL_PROFILE_NORMAL:
      return Object.freeze([
        ...buildAiPcFileToolBundle(),
        ...buildAiPcShellToolBundle(),
      ]);
    case AI_TOOL_PROFILE_INTELDEV:
      return Object.freeze([
        ...buildAiPcIntelToolBundle(),
        ...buildAiPcShellToolBundle(),
      ]);
    case AI_TOOL_PROFILE_DRIVERDEV:
      return Object.freeze([
        ...buildAiPcShellToolBundle(),
        ...buildAiPcDriverdevToolBundle(),
      ]);
    default:
      return Object.freeze([
        ...buildAiPcIntelToolBundle(),
        ...buildAiPcFileToolBundle(),
        ...buildAiPcShellToolBundle(),
        ...buildAiPcDriverdevToolBundle(),
      ]);
  }
}

export function findAiPcShellCommandByToolName(toolName) {
  return getShellCommands().find((entry) => entry.toolName === toolName) || null;
}

export function isAiPcIntelToolName(toolName) {
  return toolName === 'intel_adapter';
}

export function isAiPcFileToolName(toolName) {
  return toolName === 'file_adapter_get_primary_tree';
}

function pushArg(argv, value) {
  if (value === null || value === undefined) {
    return;
  }
  const text = String(value).trim();
  if (text) {
    argv.push(text);
  }
}

function pushKeyValueArg(argv, key, value) {
  if (value === null || value === undefined) {
    return;
  }
  const text = String(value).trim();
  if (text) {
    argv.push(`${key}=${text}`);
  }
}

function quoteDisplayArg(value) {
  const text = String(value ?? '');
  if (!/[\s"\\]/.test(text)) {
    return text;
  }
  return `"${text.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`;
}

export function buildAiPcShellCommandArgs(toolName, args = {}) {
  const entry = findAiPcShellCommandByToolName(toolName);
  if (!entry) {
    throw new Error(`unknown ai-pc shell tool: ${String(toolName || '')}`);
  }

  const argv = [];
  switch (entry.command) {
    case 'acpi':
      pushArg(argv, args?.action);
      break;
    case 'email':
      if (args?.mode === 'set_from') {
        pushArg(argv, 'set');
        pushArg(argv, args?.from);
      } else {
        pushArg(argv, args?.to);
        if (args?.mail_text !== undefined) {
          pushArg(argv, args.mail_text);
        }
      }
      break;
    case 'etc':
      pushArg(argv, args?.subcommand);
      break;
    case 'file':
      if (args?.action && args.action !== 'list') {
        pushArg(argv, args.action);
      }
      if (args?.action === 'format') {
        pushArg(argv, args?.disk_id);
      } else if (args?.action === 'ramdisc') {
        pushArg(argv, args?.size);
      }
      break;
    case 'inteldev':
      pushArg(argv, args?.action);
      pushKeyValueArg(argv, 'scope', args?.scope);
      pushKeyValueArg(argv, 'engine', args?.engine);
      pushKeyValueArg(argv, 'addr', args?.addr);
      pushKeyValueArg(argv, 'value', args?.value);
      pushKeyValueArg(argv, 'mask', args?.mask);
      pushKeyValueArg(argv, 'expected', args?.expected);
      pushKeyValueArg(argv, 'count', args?.count);
      pushKeyValueArg(argv, 'len', args?.len);
      pushKeyValueArg(argv, 'offset', args?.offset);
      pushKeyValueArg(argv, 'timeout_iters', args?.timeout_iters);
      pushKeyValueArg(argv, 'data_hex', args?.data_hex);
      pushKeyValueArg(argv, 'guard', args?.guard);
      break;
    case 'net':
      pushArg(argv, args?.subcommand);
      if (args?.subcommand === 'icmp') {
        pushArg(argv, args?.target);
        pushArg(argv, args?.selector);
      } else if (args?.subcommand === 'irc') {
        pushArg(argv, args?.host);
        pushArg(argv, args?.channel);
      } else if (args?.subcommand === 'nic') {
        pushArg(argv, args?.selector);
      } else if (args?.subcommand === 'hostname' && args?.name) {
        pushArg(argv, args.name);
      }
      break;
    case 'probe':
      pushArg(argv, args?.domain);
      pushArg(argv, args?.action);
      if (args?.domain === 'usb' && (args?.action === 'kick' || args?.action === 'rebind')) {
        pushArg(argv, args?.controller);
      } else if (args?.domain === 'nvme' && args?.action === 'flr') {
        pushArg(argv, args?.pci);
      }
      break;
    case 'set':
      pushArg(argv, args?.width);
      break;
    case 'smp':
      pushArg(argv, args?.slot);
      break;
    case 'tlb':
      if (args?.target === 'usb_probe') {
        pushArg(argv, 'usb');
        pushArg(argv, 'probe');
      } else {
        pushArg(argv, args?.target);
      }
      break;
    case 'turbo':
      pushArg(argv, args?.action);
      if (args?.action === 'verify') {
        pushArg(argv, args?.spins);
      }
      break;
    default:
      break;
  }

  return Object.freeze(argv);
}

export function buildAiPcShellCommandLine(toolName, args = {}) {
  const entry = findAiPcShellCommandByToolName(toolName);
  if (!entry) {
    throw new Error(`unknown ai-pc shell tool: ${String(toolName || '')}`);
  }
  const argv = buildAiPcShellCommandArgs(toolName, args);
  return argv.length > 0
    ? `${entry.command} ${argv.map((value) => quoteDisplayArg(value)).join(' ')}`
    : entry.command;
}

export function buildAiPcIntelCommandLine(args = {}) {
  const inteldev = getInteldevShellCommand();
  if (!inteldev) {
    throw new Error('inteldev shell command is unavailable');
  }
  return buildAiPcShellCommandLine(inteldev.toolName, args);
}

function bytesToHex(bytesLike) {
  if (!(bytesLike instanceof Uint8Array)) {
    return null;
  }
  let out = '';
  for (const value of bytesLike) {
    out += value.toString(16).padStart(2, '0');
  }
  return out;
}

function readPrimaryFsTree(maxEntries) {
  if (typeof globalThis.__trueosAiReadPrimaryFsTreeJsonAll !== 'function') {
    throw new Error('TRUEOS primary filesystem adapter is unavailable');
  }
  const raw = globalThis.__trueosAiReadPrimaryFsTreeJsonAll(maxEntries);
  if (typeof raw !== 'string' || !raw.trim()) {
    return null;
  }
  const compact = normalizeJsonFileTree(raw);
  try {
    return JSON.parse(compact);
  } catch {
    return compact;
  }
}

function summarizePrimaryFsTree(result) {
  if (!result || typeof result !== 'object' || !Array.isArray(result.entries)) {
    return result;
  }

  const root = String(result.root || '/');
  const topLevel = [];
  for (const entry of result.entries) {
    const path = String(entry?.path || '');
    const kind = String(entry?.kind || '');
    const depth = Number(entry?.depth || 0) | 0;
    if (!path || depth > 1) {
      continue;
    }
    topLevel.push({ path, kind, depth });
    if (topLevel.length >= 24) {
      break;
    }
  }

  return {
    version: Number(result.version || 1) || 1,
    root,
    max_entries: Number(result.max_entries || 0) || 0,
    truncated: !!result.truncated,
    entry_count: Array.isArray(result.entries) ? result.entries.length : 0,
    top_level: topLevel,
  };
}

export function executeAiPcFileTool(toolName, args = {}) {
  switch (toolName) {
    case 'file_adapter_get_primary_tree': {
      const maxEntries = Math.max(1, Number(args?.max_entries || 100) | 0);
      const result = readPrimaryFsTree(maxEntries);
      return {
        ok: !!result,
        tool_name: toolName,
        max_entries: maxEntries,
        result: summarizePrimaryFsTree(result),
      };
    }
    default:
      throw new Error(`unknown ai-pc file tool: ${String(toolName || '')}`);
  }
}

export function executeAiPcDriverdevTool(toolName, args = {}) {
  switch (toolName) {
    case 'driverdev_list_xhci_controllers':
      return {
        ok: true,
        tool_name: toolName,
        result: driverdev.listControllers(),
      };
    case 'driverdev_get_xhci_controller_snapshot': {
      const controllerId = Number(args?.controller_id || 0) | 0;
      const result = driverdev.getControllerSnapshot(controllerId);
      return {
        ok: !!result,
        tool_name: toolName,
        controller_id: controllerId,
        result,
      };
    }
    case 'driverdev_request_xhci_probe': {
      const controllerId = Number(args?.controller_id || 0) | 0;
      const rc = driverdev.requestProbe(controllerId);
      return {
        ok: rc === 0,
        tool_name: toolName,
        controller_id: controllerId,
        rc,
      };
    }
    case 'driverdev_request_xhci_rebind': {
      const controllerId = Number(args?.controller_id || 0) | 0;
      const rc = driverdev.requestRebind(controllerId);
      return {
        ok: rc === 0,
        tool_name: toolName,
        controller_id: controllerId,
        rc,
      };
    }
    case 'driverdev_list_devices':
      return {
        ok: true,
        tool_name: toolName,
        result: driverdev.listDevices(),
      };
    case 'driverdev_get_device_descriptor': {
      const handle = Number(args?.handle || 0) | 0;
      return {
        ok: true,
        tool_name: toolName,
        handle,
        result: driverdev.getDeviceDescriptor(handle),
      };
    }
    case 'driverdev_get_descriptor': {
      const handle = Number(args?.handle || 0) | 0;
      const descType = Number(args?.desc_type || 0) | 0;
      const descIndex = Number(args?.desc_index || 0) | 0;
      const length = Number(args?.length || 255) | 0;
      const bytes = driverdev.getDescriptor(handle, descType, descIndex, length);
      return {
        ok: !!bytes,
        tool_name: toolName,
        handle,
        desc_type: descType,
        desc_index: descIndex,
        length,
        bytes_hex: bytesToHex(bytes),
      };
    }
    case 'driverdev_get_hid_report_descriptor': {
      const handle = Number(args?.handle || 0) | 0;
      const interfaceNumber = Number(args?.interface_number || 0) | 0;
      const length = Number(args?.length || 512) | 0;
      const bytes = driverdev.getHidReportDescriptor(handle, interfaceNumber, length);
      return {
        ok: !!bytes,
        tool_name: toolName,
        handle,
        interface_number: interfaceNumber,
        length,
        bytes_hex: bytesToHex(bytes),
      };
    }
    case 'driverdev_port_reset': {
      const controllerId = Number(args?.controller_id || 0) | 0;
      const portIdx = Number(args?.port_idx || 0) | 0;
      const rc = driverdev.portReset(controllerId, portIdx);
      return {
        ok: rc === 0 || rc === 1,
        tool_name: toolName,
        controller_id: controllerId,
        port_idx: portIdx,
        rc,
      };
    }
    case 'driverdev_read_transfer_event': {
      const handle = Number(args?.handle || 0) | 0;
      const epTarget = Number(args?.ep_target || 0) | 0;
      const result = driverdev.readTransferEvent(handle, epTarget);
      return {
        ok: !!result,
        tool_name: toolName,
        handle,
        ep_target: epTarget,
        result,
      };
    }
    default:
      throw new Error(`unknown ai-pc driverdev tool: ${String(toolName || '')}`);
  }
}
