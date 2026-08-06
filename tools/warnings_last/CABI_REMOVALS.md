# Unsupported C-ABI surface removal

The warning baseline contained thirteen declarations which had no kernel
export. They were therefore unusable imports rather than compatibility
contracts with live behavior:

- `trueos_cabi_gfx_texture_dimensions`
- `trueos_cabi_gfx_texture_status`
- `trueos_cabi_gfx_upload_skybox_rgb565`
- `trueos_cabi_gfx_upload_texture_rgba_image`
- `trueos_cabi_input_pop_mouse`
- `trueos_cabi_input_pop_tablet`
- `trueos_cabi_input_write_keyboard_key`
- `trueos_cabi_input_write_keyboard_text`
- `trueos_cabi_shell1_submit_input`
- `trueos_cabi_shell2_print_line`
- `trueos_cabi_shell_command_registry_json`
- `trueos_cabi_shell_history_lines`
- `trueos_cabi_shell_history_lines_all`

The kernel repository baseline is
`dcf16f69b46a176219c55649e405a452ed0a4bf0`. The mirrored Blueprint SDK
baseline is `76c88676f0cf348d8b75b741ec9192c8b9256efa`. Each changed file has a
reversible patch at its mirrored path below `TRUEOS/` or
`TRUEOS-Blueprints/`.

`trueos-v::vshell::shell2_print_line` remains public and functional. Its stale
unresolved import was replaced with the already-live
`trueos_cabi_shell_attached_write` contract, including the Blueprint SDK's
existing CRLF behavior. Supported cursor-event, keyboard-output, mediated
input-control, attached-shell, and versioned Shell2 frontend APIs are
unchanged.
