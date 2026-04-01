import * as driverdev from '/qjs/dd/driverdev.mjs';

const SUPPORTED_SHELL_COMMANDS = Object.freeze(new Set([
  'acpi',
  'email',
  'etc',
  'file',
  'net',
  'probe',
  'set',
  'smp',
  'tlb',
  'turbo',
]));

const SHELL_COMMAND_DESCRIPTION_OVERRIDES = Object.freeze({
  acpi: 'Run the shell1 acpi command. Use raw_args like `shutdown`, `reboot`, or `sleep`.',
  email: 'Run the shell1 email command. Use raw_args exactly as shell1 expects.',
  etc: 'Run the shell1 etc command. Use raw_args exactly as shell1 expects.',
  file: 'Run the shell1 file command for filesystem listing and file-related tasks. Use raw_args exactly as shell1 expects.',
  net: 'Run the shell1 net command for network status and probes. Use raw_args exactly as shell1 expects.',
  probe: 'Run the shell1 probe command for hardware and bus inspection. Use raw_args exactly as shell1 expects.',
  set: 'Run the shell1 set command for terminal sizing. Use raw_args exactly as shell1 expects.',
  smp: 'Run the shell1 smp command. Use raw_args exactly as shell1 expects.',
  tlb: 'Run the shell1 tlb table inspection command. Use raw_args like `pci`, `usb`, `acpi`, or `dump`.',
  turbo: 'Run the shell1 turbo command. Use raw_args exactly as shell1 expects.',
});

const DRIVERDEV_TOOLS = Object.freeze([
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
]);

function rawArgSchema(command) {
  return Object.freeze({
    type: 'object',
    properties: Object.freeze({
      raw_args: Object.freeze({
        type: 'string',
        description: `Optional raw shell argument string appended after \`${command}\`. Use exactly the tokens shell1 expects.`,
      }),
    }),
    required: Object.freeze([]),
    additionalProperties: false,
  });
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
    out.push(Object.freeze({
      command,
      toolName: `shell1_${command.replace(/[^a-z0-9]+/gi, '_').replace(/^_+|_+$/g, '').toLowerCase() || 'command'}`,
      mode: String(entry?.mode || 'cmd').trim() || 'cmd',
      description: SHELL_COMMAND_DESCRIPTION_OVERRIDES[command]
        || `Run the shell1 ${command} command. Use raw_args exactly as shell1 expects.`,
    }));
  }
  return out;
}

function getShellCommands() {
  return Object.freeze(readShellRuntimeCommands());
}

function shellToolBundle() {
  return getShellCommands().map((entry) => ({
    type: 'function',
    name: entry.toolName,
    description: entry.description,
    parameters: rawArgSchema(entry.command),
  }));
}

export function buildAiPcShellToolBundle() {
  return Object.freeze(shellToolBundle());
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

export function buildAiPcToolBundle() {
  return Object.freeze([
    ...buildAiPcShellToolBundle(),
    ...buildAiPcDriverdevToolBundle(),
  ]);
}

export function findAiPcShellCommandByToolName(toolName) {
  return getShellCommands().find((entry) => entry.toolName === toolName) || null;
}

export function buildAiPcShellCommandLine(toolName, args = {}) {
  const entry = findAiPcShellCommandByToolName(toolName);
  if (!entry) {
    throw new Error(`unknown ai-pc shell tool: ${String(toolName || '')}`);
  }
  const rawArgs = typeof args?.raw_args === 'string' ? args.raw_args.trim() : '';
  return rawArgs ? `${entry.command} ${rawArgs}` : entry.command;
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

export function executeAiPcDriverdevTool(toolName, args = {}) {
  switch (toolName) {
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
    default:
      throw new Error(`unknown ai-pc driverdev tool: ${String(toolName || '')}`);
  }
}
