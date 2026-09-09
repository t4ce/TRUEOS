#!/usr/bin/env python3
"""Boot the existing QEMU runner, verify UI4 virgl readiness, capture scanout.

Uses a temporary disk snapshot and stops only the VM it started. Build first:
  make iso START_BAREMETAL_LOG=0 PUBLISH_RELEASE_SMB=0 RELEASE_BUMP_CNT=0
  python3 tools/qemu/verify-virgl.py --command 'img kernel:logo'
"""
import argparse
import json
import os
from pathlib import Path
import socket
import subprocess
import struct
import time

ROOT = Path(__file__).resolve().parents[2]


def capture_vnc(path, destination, timeout):
    """Read raw pixels from QEMU's local VNC display (also works with GL)."""
    from PIL import Image
    with socket.socket(socket.AF_UNIX) as sock:
        sock.settimeout(timeout)
        sock.connect(path)
        def read(n):
            data = bytearray()
            while len(data) < n:
                chunk = sock.recv(n - len(data))
                if not chunk:
                    raise EOFError('VNC closed during capture')
                data.extend(chunk)
            return bytes(data)
        if read(12) != b'RFB 003.008\n':
            raise RuntimeError('Expected QEMU RFB 3.8')
        sock.sendall(b'RFB 003.008\n')
        kinds = read(read(1)[0])
        if 1 not in kinds:
            raise RuntimeError('Local VNC requires unexpected authentication')
        sock.sendall(b'\x01')
        if read(4) != b'\0' * 4:
            raise RuntimeError('VNC negotiation failed')
        sock.sendall(b'\x01')
        width, height = struct.unpack('!HH', read(4))
        read(16)
        read(struct.unpack('!I', read(4))[0])
        sock.sendall(b'\0' * 4 + struct.pack('!BBBBHHHBBBxxx', 32, 24, 0, 1, 255, 255, 255, 0, 8, 16))
        sock.sendall(struct.pack('!BBHi', 2, 0, 1, 0))  # raw rectangles
        sock.sendall(struct.pack('!BBHHHH', 3, 0, 0, 0, width, height))
        pixels = bytearray(width * height * 4)
        while True:
            kind = read(1)[0]
            if kind == 2:  # bell
                continue
            if kind == 3:  # clipboard
                read(3)
                read(struct.unpack('!I', read(4))[0])
                continue
            if kind != 0:
                raise RuntimeError(f'Unexpected VNC message {kind}')
            read(1)
            count = struct.unpack('!H', read(2))[0]
            for _ in range(count):
                x, y, w, h, encoding = struct.unpack('!HHHHi', read(12))
                if encoding != 0 or x + w > width or y + h > height:
                    raise RuntimeError('Invalid raw VNC rectangle')
                data = read(w * h * 4)
                for row in range(h):
                    offset = ((y + row) * width + x) * 4
                    pixels[offset:offset + w * 4] = data[row * w * 4:(row + 1) * w * 4]
            if count:
                break
        Image.frombytes('RGB', (width, height), bytes(pixels), 'raw', 'RGBX').save(destination)


def verify_interaction(command, vnc_path, output, timeout):
    """Exercise cursor-only damage and a held/released selection on the desktop."""
    from PIL import Image, ImageChops

    def capture(name):
        path = output / name
        capture_vnc(vnc_path, path, timeout)
        return Image.open(path).convert('RGB')

    def move(axis, value):
        command('input-send-event', {'events': [
            {'type': 'rel', 'data': {'axis': axis, 'value': value}}]})
        time.sleep(.5)

    def button(down):
        command('input-send-event', {'events': [
            {'type': 'btn', 'data': {'down': down, 'button': 'left'}}]})
        time.sleep(.3)

    before = capture('interaction-before.png')
    # TRUEOS boot HID integrates each count as 1/1024 of the desktop extent.
    for axis, value in [('x', -5000), ('y', -5000), ('x', 205), ('y', 307)]:
        move(axis, value)
    cursor = capture('cursor.png')
    width, height = cursor.size
    x, y = round(width * 205 / 1024), round(height * 307 / 1024)
    region = (max(0, x - 40), max(0, y - 40), min(width, x + 40), min(height, y + 40))
    if not ImageChops.difference(before, cursor).crop(region).getbbox():
        raise RuntimeError('Software cursor did not appear after USB mouse motion')
    button(True)
    try:
        move('x', 205)
        move('y', 256)
        held = capture('selection-held.png')
    finally:
        button(False)
    released = capture('selection-released.png')
    diff = ImageChops.difference(held, released)
    bounds = diff.getbbox()
    if bounds is None:
        raise RuntimeError('Selection outline did not change on button release')
    box_width, box_height = bounds[2] - bounds[0], bounds[3] - bounds[1]
    # A large, thin border must disappear. Cursor-only damage or a filled
    # rectangle cannot satisfy this check.
    changed = sum(value != 0 for value in diff.convert('L').tobytes())
    if not (box_width > width * .15 and box_height > height * .15
            and box_width + box_height < changed < box_width * box_height * .1):
        raise RuntimeError(f'Unexpected selection damage: bounds={bounds}, pixels={changed}')
    print(f'Software cursor and selection passed: bounds={bounds}, erased_pixels={changed}', flush=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--command', action='append', default=[])
    parser.add_argument('--expect-log', action='append', default=[], help='Required text in serial or terminal output')
    parser.add_argument('--timeout', type=float, default=30)
    parser.add_argument('--settle', type=float, default=2)
    parser.add_argument('--output', type=Path, default=ROOT / 'bld/emulator-logs/virgl-verify')
    parser.add_argument('--keep-running', action='store_true')
    parser.add_argument('--interaction-smoke', action='store_true', help='Verify the software cursor and selection outline on the idle desktop')
    args = parser.parse_args()
    if args.interaction_smoke and args.command:
        parser.error("--interaction-smoke requires an idle desktop; omit --command")
    args.output = args.output.resolve()
    args.output.mkdir(parents=True, exist_ok=True)
    serial = args.output / 'serial.log'
    qmp_path = str(args.output / 'qmp.sock')
    if Path(qmp_path).exists():
        raise SystemExit(f'QMP socket already exists: {qmp_path}; use a fresh --output directory')
    serial.write_text('')
    firmware = os.environ.get('QEMU_UEFI_FIRMWARE') or next((str(p) for p in [Path('/usr/share/ovmf/OVMF.fd'), ROOT / 'bld/trueos-release/ovmf-code-x86_64.fd'] if p.is_file()), None)
    if not firmware:
        raise SystemExit('Set QEMU_UEFI_FIRMWARE to a combined OVMF image for tools/qemu/run.sh')
    env = dict(os.environ, QEMU_SERIAL=f'file:{serial}', QEMU_UEFI_FIRMWARE=os.environ.get('QEMU_UEFI_FIRMWARE', firmware))
    env.setdefault('QEMU_DISPLAY', 'egl-headless')
    vnc_path = str(args.output / 'vnc.sock')
    qemu_log = (args.output / 'qemu.log').open('w')
    process = subprocess.Popen([str(ROOT / 'tools/qemu/run.sh'), 'iso', '-snapshot', '-vnc', f'unix:{vnc_path}', '-qmp', f'unix:{qmp_path},server=on,wait=off'], cwd=ROOT, env=env, stdout=qemu_log, stderr=subprocess.STDOUT, start_new_session=True)
    qmp = None
    shell = None
    success = False
    started = time.monotonic()

    def until(predicate):
        deadline = time.monotonic() + args.timeout
        while time.monotonic() < deadline:
            if process.poll() is not None:
                raise RuntimeError(f'QEMU exited {process.returncode}; see {args.output / "qemu.log"}')
            if predicate():
                return
            time.sleep(.05)
        raise TimeoutError(f'Verification timed out; see {serial}')

    try:
        until(lambda: 'virgl-ui4: ready' in serial.read_text(errors='replace'))
        print(f'virgl UI4 ready in {time.monotonic() - started:.2f}s', flush=True)
        qmp = socket.socket(socket.AF_UNIX)
        qmp.settimeout(args.timeout)
        qmp.connect(qmp_path)
        stream = qmp.makefile('rwb', buffering=0)
        json.loads(stream.readline())

        def command(name, arguments=None):
            stream.write((json.dumps(dict(execute=name, **({'arguments': arguments} if arguments else {}))) + '\n').encode())
            while True:
                result = json.loads(stream.readline())
                if 'error' in result:
                    raise RuntimeError(result)
                if 'return' in result:
                    return result['return']

        command('qmp_capabilities')
        if args.command:
            until(lambda: 'spawn-svc: started net-shell-listener' in serial.read_text(errors='replace'))
            shell = socket.create_connection(('127.0.0.1', int(env.get('QEMU_HOST_TCP_PORT_NET_SHELL', 14245))), args.timeout)
            shell.settimeout(.1)
            shell_log = bytearray()
            def drain_shell(seconds):
                deadline = time.monotonic() + seconds
                while time.monotonic() < deadline:
                    try:
                        data = shell.recv(65536)
                        if not data:
                            raise EOFError('TRUEOS terminal closed')
                        shell_log.extend(data)
                        if b'\x1b[18t' in data:
                            shell.sendall(b'\x1b[8;40;140t')
                    except socket.timeout:
                        pass

            # Finish terminal geometry negotiation before submitting commands.
            drain_shell(2)
            for text in args.command:
                shell.sendall((text + '\r').encode())
                drain_shell(args.settle)
            (args.output / 'shell.log').write_bytes(shell_log)
        else:
            time.sleep(args.settle)
        observed = serial.read_text(errors='replace')
        if shell is not None:
            observed += shell_log.decode(errors='replace')
        for expected in args.expect_log:
            if expected not in observed:
                raise RuntimeError(f'Missing expected output: {expected!r}; see {args.output}')
        screenshot = args.output / 'scanout.png'
        capture_vnc(vnc_path, screenshot, args.timeout)
        print(f'Scanout: {screenshot}\nSerial: {serial}', flush=True)
        from PIL import Image
        if not any(high > low for low, high in Image.open(screenshot).getextrema()):
            raise RuntimeError(f'Scanout is a uniform color; see {screenshot}')
        if args.interaction_smoke:
            until(lambda: 'hid mouse 0627:0001 ready' in serial.read_text(errors='replace'))
            if 'interaction=slot4-software hardware_cursor=disabled' not in observed:
                raise RuntimeError('Expected software interaction plane with hardware cursor disabled')
            verify_interaction(command, vnc_path, args.output, args.timeout)
        host_errors = (args.output / 'qemu.log').read_text(errors='replace')
        if any(marker in host_errors.lower() for marker in ['context error', 'illegal resource', 'failed to complete framebuffer']):
            raise RuntimeError(f'Host renderer rejected work; see {args.output / "qemu.log"}')
        if 'virgl-ui4: device failed' in serial.read_text(errors='replace'):
            raise RuntimeError(f'Guest stopped the GPU; see {serial}')
        success = True
        if args.keep_running:
            (args.output / 'qemu.pid').write_text(str(process.pid) + '\n')
            print(f'QEMU remains running (pid {process.pid}, QMP {qmp_path})', flush=True)
        else:
            command('quit')
    finally:
        if shell:
            shell.close()
        if qmp:
            qmp.close()
        if not (success and args.keep_running):
            if process.poll() is None:
                process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
            Path(qmp_path).unlink(missing_ok=True)
            Path(vnc_path).unlink(missing_ok=True)
        qemu_log.close()


if __name__ == '__main__':
    main()
