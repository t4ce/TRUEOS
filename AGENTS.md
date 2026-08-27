# TRUEOS agent guidance

## Shell2 knowledge

- Before reasoning about Shell2 commands, cmd/apps mode, the `§` Matrix/headjack operator, Blueprint terminal handoff, launch scripts, or test-rig endpoints, query `trueos-doc` instead of relying on memory.
- Start with `trueos-doc context` when several Shell2 concepts are involved. Use `trueos-doc topic <name>` for architecture and `trueos-doc command <name>` before composing an exact command.
- Treat `trueos-doc` as read-only documentation. A documented example does not authorize connecting to the rig, launching or stopping a Blueprint, rebooting hardware, or performing another external mutation.
- Preserve requested ordering across hardware operations. For example, when evidence must be collected before reboot, do not issue the reboot unless collection completed successfully.
- If `trueos-doc` is unavailable on `PATH`, run `tools/trueos-doc` from this repository. Fix the documentation source or its tests when knowledge is stale; do not duplicate changing Shell2 schemas or LAN defaults here.

## Validation

- After changing `tools/trueos-doc` or its companion skill, run `python3 tools/test_trueos_doc.py` and validate the skill with the skill-creator validator.
