---
name: baremetalshell
description: TRUEOS iteration loop: build ISO, connect QJS via nc, reboot, repeat.
---
TRUEOS bare-metal AI bridge cycle (QJS REPL over TCP).

This skill is intentionally strict and copy-paste safe for agents.

## Golden loop (copy-paste)

1) Build:

```sh
cd /home/t4ce/Repos/TRUEOS
make iso-release
```

2) Reboot guest from QJS bridge:

```sh
timeout 12 sh -c '{
	printf "\r\n"
	sleep 1
	printf "TRUEOS.acpi(\"reboot\")\r\n"
	sleep 1
	printf ".exit\r\n"
	sleep 1
} | nc -w 6 192.168.178.78 4246 | cat'
```

3) Wait for reboot:

```sh
sleep 30
```

4) Validate bridge + TRUEOS API on new boot:

```sh
timeout 15 sh -c '{
	printf "\r\n"
	sleep 1
	printf "print(\"ping\")\r\n"
	sleep 1
	printf "print(typeof TRUEOS)\r\n"
	sleep 1
	printf "print(TRUEOS[\"logs\"])\r\n"
	sleep 1
	printf ".exit\r\n"
	sleep 1
} | nc -w 6 192.168.178.78 4246 | cat'
```

Expected proof markers in output:

- `qjs: ping`
- `qjs: object`

If running the newest kernel containing `TRUEOS.logs`, then:

- `print(TRUEOS["logs"])` prints a native function body
- `print(TRUEOS.logs(256))` returns text (possibly empty, but no TypeError)

## Why the initial Enter is mandatory

Always send one initial `\r\n` and a short `sleep 1` before first JS command.
Without this, the session may show only `qjs>` prompt echoes and ignore payload.

## Reliable non-interactive read of bringup log cache

```sh
timeout 15 sh -c '{
	printf "\r\n"
	sleep 1
	printf "print(TRUEOS.logs(8192))\r\n"
	sleep 1
	printf ".exit\r\n"
	sleep 1
} | nc -w 6 192.168.178.78 4246 | cat'
```

## Troubleshooting checklist

- If you get only `qjs>` prompt, increase delays to `sleep 2` between lines.
- If `TRUEOS["logs"]` is `undefined`, running OS image is older than current build; reboot into latest image and retry.
- Prefer bracket syntax in automation (`TRUEOS["logs"]`) to avoid shell escaping mistakes.