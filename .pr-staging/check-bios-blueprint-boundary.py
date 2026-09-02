#!/usr/bin/env python3
"""Mechanical guard for the read-only BIOS Blueprint and its public ABI."""

from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[1]
SERVER = ROOT / 'apps/bios/server.rs'
CLIENT = ROOT / 'apps/bios/app.js'
ABI = ROOT / 'crates/trueos-v/src/vbios.rs'
API = ROOT / 'api/src/lib.rs'

paths = [SERVER, CLIENT, ABI, API]
missing = [str(path.relative_to(ROOT)) for path in paths if not path.is_file()]
if missing:
    raise SystemExit(f'missing BIOS Blueprint source files: {", ".join(missing)}')

server = SERVER.read_text(encoding='utf-8')
client = CLIENT.read_text(encoding='utf-8')
abi = ABI.read_text(encoding='utf-8')
api = API.read_text(encoding='utf-8')
common = '\n'.join((server, client, abi)).casefold()
server_folded = server.casefold()
client_folded = client.casefold()

forbidden_common = (
    'get_variable(',
    'set_variable(',
    'route_config(',
    'form_browser',
    'reset_system(',
    'update_capsule(',
)
for token in forbidden_common:
    if token in common:
        raise SystemExit(f'read-only BIOS Blueprint boundary violated by token: {token}')

forbidden_server_routes = (
    '.post(',
    '.put(',
    '.patch(',
    '.delete(',
    'routing::{get, post}',
)
for token in forbidden_server_routes:
    if token in server_folded:
        raise SystemExit(f'BIOS HTTP server exposes a mutation route: {token}')

for method in ('post', 'put', 'patch', 'delete'):
    patterns = (
        f"method: '{method}'",
        f'method: "{method}"',
        f"method:'{method}'",
        f'method:"{method}"',
    )
    if any(pattern in client_folded for pattern in patterns):
        raise SystemExit(f'BIOS client attempts an HTTP {method.upper()} request')

for route in re.findall(r'\.route\(([^\n]+)', server):
    if 'get(' not in route:
        raise SystemExit(f'BIOS HTTP route is not GET-only: {route.strip()}')

required_server = (
    '/api/bios/schema',
    '/api/bios/status',
    'active_write_path: "none"',
    'read_only: true',
)
for token in required_server:
    if token not in server:
        raise SystemExit(f'missing BIOS server guard: {token}')

required_client = (
    "event.key === 'F10'",
    "event.ctrlKey || event.metaKey",
    'Save is disabled by construction',
    "fetch('/api/bios/schema'",
)
for token in required_client:
    if token not in client:
        raise SystemExit(f'missing BIOS client guard: {token}')

required_abi = (
    'trueos_vlayer_bios_schema_snapshot_read',
    'pub fn schema_len()',
    'pub fn schema_bytes()',
    'pub fn schema_text()',
)
for token in required_abi:
    if token not in abi:
        raise SystemExit(f'missing BIOS ABI surface: {token}')

if 'pub use v::vbios as bios;' not in api:
    raise SystemExit('TRUEOS public API does not re-export the BIOS schema surface')

print('bios-blueprint-boundary: GET-only read-only explorer verified')
