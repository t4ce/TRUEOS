This folder is meant to be copied into the FAT disk image at `/qjs`.

Suggested layout on the disk image:

- `/qjs/main.mjs`
- `/qjs/util.mjs`
- `/qjs/sub/rel.mjs`

Then in the TRUEOS shell:

- `qjsm @/qjs/main.mjs`
