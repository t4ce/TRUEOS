# TRUEOS agent guidance

## Shell2 knowledge

- Before reasoning about Shell2 commands, cmd/apps mode, the `§` Matrix/headjack operator, Blueprint terminal handoff, launch scripts, logging tiers/capture files, the TRUEOSFS HTTP API, checked-in architecture/hardware references, or test-rig endpoints, query `trueos-doc` instead of relying on memory.
- Start with `trueos-doc context` when several Shell2 concepts are involved. Use `trueos-doc topic <name>` for architecture and `trueos-doc command <name>` before composing an exact command.
- Treat `trueos-doc` as read-only documentation. A documented example does not authorize connecting to the rig, launching or stopping a Blueprint, rebooting hardware, or performing another external mutation.
- Preserve requested ordering across hardware operations. For example, when evidence must be collected before reboot, do not issue the reboot unless collection completed successfully.
- If `trueos-doc` is unavailable on `PATH`, run `tools/trueos-doc` from this repository. Fix the documentation source or its tests when knowledge is stale; do not duplicate changing Shell2 schemas or LAN defaults here.
- Before diagnosing a missing TRUEOS log, query `trueos-doc topic logs`; a record rejected by its area/level policy never reaches a capture file.
- Use `trueos-doc topic references` to discover the repository's HTML fact references instead of relying on filenames remembered out of context.
- Use `trueos-doc topic trueosfs-http` before transferring rig files. Discover the mounted root ID and verify an expected new artifact exists; never silently pull an older file as its replacement.

## Validation

- After changing `tools/trueos-doc` or its companion skill, run `python3 tools/test_trueos_doc.py` and validate the skill with the skill-creator validator.
