---
name: baremetalshell
description: TRUEOS iteration loop: build ISO, connect QJS via nc, reboot, repeat.
---
TRUEOS bare-metal AI bridge cycle (QJS REPL over TCP).

Use this loop for fast kernel iteration:

## 1) Make ISO

```sh
cd /home/t4ce/Repos/TRUEOS
make iso
```

## 2) Connect with nc (AI/QJS bridge)

```sh
nc 192.168.178.78 4246
```

This is the runtime QJS REPL.

## 3) Reboot from QJS

In the QJS session:

```js
TRUEOS.acpi("reboot")
```

Alternative:

```js
TRUEOS.reboot()
```

## 4) Wait before reconnect

After sending reboot, wait at least 30 seconds before reconnecting:

```sh
sleep 30
```

Then reconnect to `4246` and continue.

## Loop shape

`program -> make iso -> nc/qjs -> acpi("reboot") -> sleep 30 -> reconnect -> repeat`

## Working live demos (validated)

Run these in the QJS REPL after connecting:

```js
import proc, { cwd } from "node:process"; print("proc-cwd", cwd())
import * as path from "path"; print("join", path.join("bm", "qjs"))
import("left-pad@1.3.0").then(m => print("leftpad", m.default("ok", 6, "."))).catch(e => print(e))
```

Expected style of output:

- `qjs: proc-cwd /`
- `qjs: join bm/qjs`
- `qjs: leftpad ....ok`

Notes from validation:

- Static `import ... from "left-pad@1.3.0"; ...` can fail in single-line REPL usage; dynamic `import(...)` is reliable.
- Seeing `qjs: => [object Promise]` after `import(...)` is expected.

## Quick probe example (non-interactive)

```sh
timeout 8 sh -c "{ sleep 1; printf '\r\n'; sleep 1; printf 'TRUEOS.acpi(\"reboot\")\r\n'; sleep 1; } | nc 192.168.178.78 4246"
sleep 30
timeout 8 sh -c "{ sleep 1; printf '\r\n'; sleep 1; } | nc 192.168.178.78 4246"
```

Use a short timeout for the reboot send itself; the required boot grace period is the explicit `sleep 30` after reboot.

Notes:
- Use `TRUEOS.acpi("s1")`..`TRUEOS.acpi("s5")` for sleep states when needed.
- If output gets noisy or prompt state looks stale, disconnect and reconnect.