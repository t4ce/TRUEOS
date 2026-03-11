function getShell1RuntimeSource() {
  const runtime = globalThis.__trueosShell1Runtime;
  if (!runtime || typeof runtime !== "object" || !Array.isArray(runtime.commands)) {
    throw new Error("shell1 runtime registry is unavailable on globalThis.__trueosShell1Runtime");
  }
  return Object.freeze(runtime.commands.map((entry) => Object.freeze({
    command: String(entry && entry.command ? entry.command : ""),
    args: Object.freeze(Array.isArray(entry && entry.args)
      ? entry.args.map((arg) => Object.freeze({
        name: String(arg && arg.name ? arg.name : ""),
        type: String(arg && arg.type ? arg.type : "str"),
        required: !!(arg && arg.required),
      }))
      : []),
  })));
}

function getShell1Commands() {
  return getShell1RuntimeSource().map(enrichCommand);
}

const COMMAND_DESCRIPTION_OVERRIDES = Object.freeze({
  "§": "Select a shell1 status section slot and optionally print its current contents.",
  cmd: "List the available top-level shell1 terminal commands.",
  ecma48: "Send raw ECMA-48 control text through the shell1 terminal renderer.",
  net: "Show shell1 network status and related network information.",
  "net.icmp": "Run the shell1 ICMP probe command against a target host or address.",
  "net.nic": "Inspect or select shell1 network interface information by NIC index.",
  "net.hostname": "Read or update the shell1 network hostname.",
  surf: "Open a URL through the shell1 browser or surf pipeline.",
  frog: "Run the shell1 frog command with the API key it expects.",
  dmafpga: "Run the shell1 DMA FPGA command when the dma_nic_fpga feature is enabled.",
  update: "Run the shell1 update workflow.",
  install: "Run the shell1 install workflow.",
  format: "Run the shell1 format workflow.",
  bench: "Run the shell1 benchmark workflow.",
  "bench.net": "Run the shell1 network benchmark workflow.",
  file: "Inspect a shell1 file entry or file-related target by identifier.",
  ai: "Start the shell1 AI command and optionally seed it with the first prompt text.",
  qjs: "Run the shell1 QuickJS command with optional inline source text.",
  acpi: "Run the shell1 ACPI control command for the requested state.",
  hv: "Run the shell1 hypervisor command for the requested operation.",
  go: "Switch shell1 into the primary GO mode.",
  "go.two": "Switch shell1 into the alternate GO mode.",
  tlb: "Show the shell1 top-level table view.",
  "tlb.pci": "Show the shell1 PCI table view.",
  "tlb.pci.bar": "Show the shell1 PCI BAR table view.",
  "tlb.mem": "Show the shell1 memory table view.",
  "tlb.cpu": "Show the shell1 CPU table view.",
  "tlb.acpi": "Show the shell1 ACPI table view.",
  "tlb.acpi.facp": "Show the shell1 FACP ACPI table view.",
  "tlb.acpi.madt": "Show the shell1 MADT ACPI table view.",
  "tlb.acpi.hpet": "Show the shell1 HPET ACPI table view.",
  "tlb.acpi.mcfg": "Show the shell1 MCFG ACPI table view.",
  "tlb.acpi.ssdt": "Show the shell1 SSDT ACPI table view.",
  "tlb.x2apic": "Show the shell1 x2APIC table view.",
  "tlb.uefi": "Show the shell1 UEFI table view.",
  "tlb.usb": "Show the shell1 USB table view.",
  "tlb.dump": "Dump the current shell1 table view data.",
  mandel: "Render the shell1 Mandelbrot demo.",
  set: "Resize or reconfigure the shell1 terminal grid.",
  turbo: "Run the shell1 turbo control command.",
  smp: "Inspect or switch shell1 SMP state for a slot.",
  cube: "Start the shell1 cube demo.",
  "cube.ico": "Start the shell1 cube icon demo.",
  txt: "Show the shell1 text mode demo.",
  tetris: "Start the shell1 tetris demo.",
  rain: "Start the shell1 rain effect demo.",
  insane: "Start the shell1 insane demo.",
});

function sanitizeToolStem(command) {
  if (command === "§") {
    return "section";
  }
  return command
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "")
    .replace(/_+/g, "_");
}

function makeToolName(command) {
  const stem = sanitizeToolStem(command) || "command";
  return `shell1_${stem}`;
}

function fallbackCommandDescription(command) {
  if (command.includes(".")) {
    return `Run the shell1 \`${command}\` subcommand.`;
  }
  return `Run the shell1 \`${command}\` command.`;
}

function getCommandDescription(command) {
  return COMMAND_DESCRIPTION_OVERRIDES[command] || fallbackCommandDescription(command);
}

function schemaTypeForArg(argType) {
  if (argType === "u8" || argType === "usize") {
    return "integer";
  }
  return "string";
}

function buildTypeSentence(arg) {
  if (arg.type === "rest") {
    return "This property captures the remainder of the shell line, so it may contain spaces exactly as the command should receive them.";
  }
  if (arg.type === "u8") {
    return "Use a whole number in the 0 to 255 range so it matches the shell's `u8` parser.";
  }
  if (arg.type === "usize") {
    return "Use a non-negative whole number so it matches the shell's `usize` parser.";
  }
  return "Pass the exact string token that the shell command expects for this position.";
}

function buildNameSentence(command, arg) {
  const requirement = arg.required ? "required" : "optional";
  if (arg.name === "url") {
    return `Provide the ${requirement} URL value for the shell1 \`${command}\` command.`;
  }
  if (arg.name === "api_key") {
    return `Provide the ${requirement} API key string for the shell1 \`${command}\` command.`;
  }
  if (arg.name === "target") {
    return `Provide the ${requirement} target host or address for the shell1 \`${command}\` command.`;
  }
  if (arg.name === "nic") {
    return `Provide the ${requirement} NIC selector for the shell1 \`${command}\` command.`;
  }
  if (arg.name === "src") {
    return `Provide the ${requirement} inline source text for the shell1 \`${command}\` command.`;
  }
  return `Provide the ${requirement} \`${arg.name}\` value for the shell1 \`${command}\` command.`;
}

function buildParamDescription(command, arg) {
  return `${buildNameSentence(command, arg)} ${buildTypeSentence(arg)}`;
}

function buildPropertySchema(command, arg) {
  const schema = {
    type: schemaTypeForArg(arg.type),
    description: buildParamDescription(command, arg),
  };
  if (arg.type === "u8") {
    schema.minimum = 0;
    schema.maximum = 255;
  } else if (arg.type === "usize") {
    schema.minimum = 0;
  }
  return schema;
}

function enrichCommand(entry) {
  const args = Array.isArray(entry.args) ? entry.args.map((arg) => ({ ...arg })) : [];
  const properties = {};
  const required = [];

  for (const arg of args) {
    properties[arg.name] = buildPropertySchema(entry.command, arg);
    if (arg.required) {
      required.push(arg.name);
    }
  }

  return Object.freeze({
    command: entry.command,
    toolName: makeToolName(entry.command),
    description: getCommandDescription(entry.command),
    args: Object.freeze(args),
    parameters: Object.freeze({
      type: "object",
      properties: Object.freeze(properties),
      required: Object.freeze(required),
      additionalProperties: false,
    }),
  });
}

export function getAiPcShellCommands() {
  return Object.freeze(getShell1Commands());
}

export function buildAiPcShellToolBundle(options = {}) {
  void options;
  const bundle = [];

  for (const command of getShell1Commands()) {
    bundle.push({
      type: "function",
      name: command.toolName,
      description: command.description,
      parameters: command.parameters,
    });
  }

  return bundle;
}

export function findAiPcShellCommandByToolName(toolName) {
  if (typeof toolName !== "string" || !toolName) {
    return null;
  }
  for (const command of getShell1Commands()) {
    if (command.toolName === toolName) {
      return command;
    }
  }
  return null;
}

export function findAiPcShellCommand(commandName) {
  if (typeof commandName !== "string" || !commandName) {
    return null;
  }
  for (const command of getShell1Commands()) {
    if (command.command === commandName) {
      return command;
    }
  }
  return null;
}