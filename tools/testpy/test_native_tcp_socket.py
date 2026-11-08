#!/usr/bin/env python3
"""Execute the production kernel native TCP socket lifecycle without publication.

The complete native TCP creation/bind/connect/listen module and extracted actual
Mio state/event/option/close bodies run over instrumented network queues and VM
identities. The real private VNet connect constructor is retained. These tests
check state and contract behavior; packets, NICs and VM memory copies are not
executed. Separate Tokio tests exercise complete client and reactor sources.
"""
from __future__ import annotations
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import tempfile

from test_native_mio_readiness import item, constant
from test_native_mio import environment
from test_native_tokio import TOOLCHAIN

ROOT = Path(__file__).resolve().parents[2]


def function(source, name):
    return item(source, rf'(?:pub(?:\(crate\))? )?(?:unsafe )?(?:extern "(?:Rust|C)" )?fn {name}\(')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    output = parser.parse_args().output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    paths = {name: ROOT / path for name, path in {
        'mio': 'src/mio_compat.rs', 'socket': 'src/mio_compat/tcp_socket.rs',
        'bridge': 'src/r/net/mod.rs', 'adapter': 'src/net/adapter.rs',
        'protocol': 'crates/trueos-vm/src/vmcall.rs'}.items()}
    sources = {name: path.read_text() for name, path in paths.items()}
    fragments = {}

    def keep(name, body):
        fragments[name] = body
        return body

    mio = sources['mio']
    methods = ['alloc_socket_id', 'tcp_listener_address_in_use', 'tcp_listener_port_in_use',
               'alloc_tcp_listen_port', 'socket', 'socket_mut', 'socket_capacity_available',
               'discard_socket', 'socket_for_owner', 'socket_mut_for_owner',
               'socket_by_handle_mut', 'socket_by_handle_id', 'drop_pending_open',
               'pending_opens_for_socket', 'post_tcp_listener_open', 'ensure_tcp_listener_depth',
               'submit', 'submit_for_actor', 'handle_event', 'handle_unattributed_error']
    # Keep any implementation helper added to distinguish failed enqueue from
    # cancellation retirement. Missing required references fail compilation.
    for candidate in ['cancel_pending_open', 'retire_pending_open', 'cancel_last_pending_open']:
        if re.search(r'\bfn ' + candidate + r'\(', mio):
            methods.append(candidate)
    substitutions = {
        '@VNET@': json.dumps(str(ROOT / 'crates/trueos-v/src/vnet.rs')),
        '@PROGRESS@': json.dumps(str(ROOT / 'src/mio_compat/progress.rs')),
        '@TCP_SOCKET@': json.dumps(str(paths['socket'])),
        '@OP_CONSTANTS@': '\n'.join(keep(name, constant(sources['protocol'], name)) for name in [
            'OP_BP_MIO_TCP_SOCKET_CREATE', 'OP_BP_MIO_TCP_SOCKET_BIND',
            'OP_BP_MIO_TCP_SOCKET_CONNECT', 'OP_BP_MIO_TCP_SOCKET_LISTEN']),
        '@MIO_CONSTANTS@': '\n'.join(keep(name, constant(mio, name)) for name in [
            'STATUS_OK', 'STATUS_UNSUPPORTED', 'STATUS_WOULD_BLOCK', 'STATUS_NOT_CONNECTED',
            'STATUS_INVALID_INPUT', 'STATUS_NOT_FOUND', 'STATUS_IO', 'STATUS_NO_DEVICE',
            'STATUS_RESOURCE_EXHAUSTED', 'TCP_LISTENER_PREPOST']),
        '@RAW_ADDR@': keep('TrueosMioSocketAddr', item(mio, 'pub struct TrueosMioSocketAddr')),
        '@ADDR@': keep('CompatAddr', item(mio, 'enum CompatAddr')),
        '@ADDR_IMPL@': keep('CompatAddr impl', item(mio, 'impl CompatAddr')),
        '@KIND@': keep('MioSocketKind', item(mio, 'enum MioSocketKind')),
        '@SETUP@': keep('TcpSocketSetup', item(mio, 'struct TcpSocketSetup')),
        '@STATE@': keep('MioSocketState', item(mio, 'struct MioSocketState')),
        '@PENDING@': keep('PendingOpen', item(mio, 'struct PendingOpen')),
        '@METHODS@': '\n'.join(keep(name, function(mio, name)) for name in methods),
        '@OWNER_MATCHES@': keep('owner_matches', function(mio, 'owner_matches')),
        '@CAPTURE_NETWORK_ACTOR@': keep('capture_network_actor', function(mio, 'capture_network_actor')),
        '@SOCKET_HAS_HANDLE@': keep('socket_has_handle', function(mio, 'socket_has_handle')),
        '@UDP_PEER_MATCHES@': keep('udp_peer_matches', function(mio, 'udp_peer_matches')),
        '@READ_ADDR@': keep('read_addr', function(mio, 'read_addr')),
        '@CLOSE@': keep('mio_socket_close_host', function(mio, 'mio_socket_close_host')),
        '@OPTION@': keep('mio_socket_option_host', function(mio, 'mio_socket_option_host')),
        '@ACCEPT@': keep('mio_tcp_listener_accept_host', function(mio, 'mio_tcp_listener_accept_host')),
        '@BRIDGE_CONNECT@': keep('VNetBridge::submit_tcp_connect_from', function(sources['bridge'], 'submit_tcp_connect_from')),
        '@BRIDGE_ACTOR_CONNECT@': keep('VNet::submit_tcp_connect_from_for_actor', function(sources['bridge'], 'submit_tcp_connect_from_for_actor')),
        '@NET_ENDPOINT@': keep('NetEndpoint', item(sources['adapter'], 'pub struct NetEndpoint')),
        '@NET_ENDPOINT6@': keep('NetEndpointV6', item(sources['adapter'], 'pub struct NetEndpointV6')),
        '@SOCKET_KIND@': keep('SocketKind', item(sources['adapter'], 'pub enum SocketKind')),
        '@SOCKET_RECORD@': keep('SocketRecord', item(sources['adapter'], 'struct SocketRecord')),
        '@OPEN_CONNECT@': keep('open_tcp_connect', function(sources['adapter'], 'open_tcp_connect')),
        '@OPEN_CONNECT6@': keep('open_tcp_connect_v6', function(sources['adapter'], 'open_tcp_connect_v6')),
        '@CONNECT_DISPATCH@': keep('OpenTcpConnectFrom dispatch', item(sources['adapter'], r'NetCommand::OpenTcpConnectFrom \{ local, remote \} => match \(local, remote\) \{')),
        '@PRIVATE_ADAPTER_METHODS@': '\n'.join(keep(name, function(sources['adapter'], name)) for name in [
            'open_tcp_listener_for_actor', 'open_loopback_tcp_connect', 'route_tcp_send',
            'flush_loopback_tcp_tx', 'close_loopback_tcp', 'revoke_tcp_access', 'remove_record']),
        '@TCP_TX_QUEUE@': json.dumps(str(ROOT / 'src/net/tcp_tx_queue.rs')),
    }
    harness = (ROOT / 'tools/testpy/native_mio/tcp_socket_kernel.rs').read_text()
    for key, value in substitutions.items():
        harness = harness.replace(key, value)
    assert not re.findall(r'@[A-Z_]+@', harness)
    generated = output / 'production.rs'
    generated.write_text(harness)
    (output / 'Cargo.toml').write_text(f'''[package]
name="trueos-kernel-tcp-socket-proof"
version="0.0.0"
edition="2024"
[workspace]
[lib]
path="production.rs"
[dependencies]
smoltcp={{path={json.dumps(str(ROOT / 'vendor/smoltcp-0.13.1'))},default-features=false,features=["alloc","proto-ipv4","proto-ipv6","socket-tcp","medium-ethernet"]}}
''')
    checks = []
    env = environment(output / 'target')
    for mode, flags in [('debug', []), ('release', ['--release'])]:
        commands = [['rustup', 'run', TOOLCHAIN, 'cargo', 'test', '--offline', *flags,
                     '--target', 'x86_64-unknown-linux-gnu', '--manifest-path', str(output / 'Cargo.toml'),
                     '--', '--test-threads=1']]
        log = output / (mode + '.log')
        with log.open('w') as stream:
            for command in commands:
                result = subprocess.run(command, cwd=tempfile.gettempdir(), env=env, stdout=stream,
                                        stderr=subprocess.STDOUT, timeout=180)
                if result.returncode:
                    raise RuntimeError(log.read_text()[-16000:])
        tests = int(re.search(r'test result: ok\. (\d+) passed', log.read_text())[1])
        assert tests == 40, tests
        checks.append({'mode': mode, 'tests': tests, 'log': str(log), 'commands': commands})
    receipt = {'scope': __doc__, 'checks': checks,
               'cases_per_mode': {'native_socket_lifecycle': 20, 'real_smoltcp_endpoints': 4,
                                  'embedded_production_progress': 3, 'private_native_admission_and_capture': 3,
                                  'private_loopback_authority': 6, 'embedded_transmit_queue': 4},
               'source_sha256': {name: hashlib.sha256(path.read_bytes()).hexdigest() for name, path in paths.items()},
               'production_fragments_sha256': {name: hashlib.sha256(body.encode()).hexdigest() for name, body in fragments.items()},
               'harness_sha256': hashlib.sha256(harness.encode()).hexdigest()}
    (output / 'receipt.json').write_text(json.dumps(receipt, indent=2) + '\n')
    print('Kernel native TCP lifecycle/private loopback/real smoltcp: 40 production-body cases in debug and release.')


if __name__ == '__main__':
    main()
