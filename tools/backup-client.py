#!/usr/bin/env python3
"""Download a TRUEOS whole-disk backup, reconnecting to the same held snapshot.
Requires PyNaCl (python3 -m pip install PyNaCl). The key is prompted, never saved.
"""
import argparse
import fcntl
import getpass
import hashlib
import json
import os
from pathlib import Path
import socket
import struct
import time

from nacl.bindings import (
    crypto_aead_xchacha20poly1305_ietf_encrypt as encrypt,
    crypto_aead_xchacha20poly1305_ietf_decrypt as decrypt,
)
from nacl.exceptions import CryptoError


class ProtocolError(Exception):
    pass


def receive(sock, size):
    result = bytearray()
    while len(result) < size:
        part = sock.recv(size - len(result))
        if not part:
            raise ConnectionError("connection closed")
        result.extend(part)
    return bytes(result)


def nonce(hello, sequence, response=False):
    return hello[8:24] + struct.pack("<Q", sequence | ((1 << 63) if response else 0))


def checkpoint(path, state):
    temporary = path.with_name(path.name + ".tmp")
    with temporary.open("w") as out:
        json.dump(state, out, sort_keys=True)
        out.flush()
        os.fsync(out.fileno())
    os.replace(temporary, path)
    directory = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(directory)
    finally:
        os.close(directory)


def prefix_digest(image, length):
    image.seek(0)
    digest = hashlib.sha256()
    remaining = length
    while remaining:
        block = image.read(min(remaining, 1024 * 1024))
        if not block:
            raise ProtocolError("image is shorter than its saved checkpoint")
        digest.update(block)
        remaining -= len(block)
    return digest


def download(host, port, path, session, key, *, retry_delay=2):
    state_path = path.with_name(path.name + ".resume.json")
    # Never overwrite an existing image without its matching checkpoint.
    if path.exists() != state_path.exists():
        raise ProtocolError("image and .resume.json must both exist, or both be absent")
    with path.open("r+b" if path.exists() else "x+b") as image:
        fcntl.flock(image, fcntl.LOCK_EX | fcntl.LOCK_NB)
        if state_path.exists():
            state = json.loads(state_path.read_text())
            if state.get("session") != session.hex():
                raise ProtocolError("checkpoint belongs to a different backup session")
        else:
            state = {"session": session.hex(), "offset": 0,
                     "sha256": hashlib.sha256().hexdigest()}
            image.flush()
            os.fsync(image.fileno())
            checkpoint(state_path, state)
        offset = state["offset"]
        if type(offset) is not int or offset < 0:
            raise ProtocolError("invalid checkpoint offset")
        digest = prefix_digest(image, offset)
        if digest.hexdigest() != state["sha256"]:
            raise ProtocolError("saved image prefix failed SHA-256 verification")
        image.truncate(offset)  # Discard a chunk not committed to the sidecar.
        image.seek(offset)
        while True:
            try:
                with socket.create_connection((host, port), timeout=15) as sock:
                    sock.settimeout(45)
                    hello = receive(sock, 56)
                    if hello[:8] != b"TBKP0001" or hello[24:40] != session:
                        raise ProtocolError("wrong service or expired/replaced backup session")
                    total, block_size, chunk = struct.unpack("<QII", hello[40:])
                    if (not total or not block_size or not chunk or chunk > 256 * 1024
                            or chunk % block_size or total % block_size
                            or offset > total or offset % block_size):
                        raise ProtocolError("invalid disk geometry or checkpoint")
                    geometry = [total, block_size, chunk]
                    if "geometry" in state and state["geometry"] != geometry:
                        raise ProtocolError("snapshot geometry changed")
                    sequence = 0
                    while True:
                        request = encrypt(struct.pack("<Q", offset), hello,
                                          nonce(hello, sequence), key)
                        sock.sendall(request)
                        size = struct.unpack("<I", receive(sock, 4))[0]
                        expected = min(chunk, total - offset)
                        if size != expected + 16:
                            raise ProtocolError("invalid encrypted chunk length")
                        try:
                            data = decrypt(receive(sock, size), hello,
                                           nonce(hello, sequence, True), key)
                        except CryptoError as error:
                            raise ProtocolError("authentication failed; key or data is invalid") from error
                        sequence += 1
                        if not data:
                            state["complete"] = True
                            checkpoint(state_path, state)
                            sock.sendall(encrypt(struct.pack("<Q", (1 << 64) - 1), hello,
                                                 nonce(hello, sequence), key))
                            print(f"\nSuccess: {total} bytes saved; SHA-256 {digest.hexdigest()}")
                            return state
                        image.write(data)
                        image.flush()
                        os.fsync(image.fileno())
                        digest.update(data)
                        offset += len(data)
                        state.update(offset=offset, sha256=digest.hexdigest(), geometry=geometry)
                        checkpoint(state_path, state)
                        # Only the next authenticated request acknowledges this
                        # fsynced data and checkpoint. Lost responses are replayed.
                        print(f"\r{offset * 100 // total}% saved — {total - offset} bytes left", end="", flush=True)
            except (ConnectionError, TimeoutError, OSError) as error:
                # Disk errors must not enter the network retry loop.
                if isinstance(error, OSError) and not isinstance(error, (ConnectionError, TimeoutError)) and error.errno not in (101, 104, 110, 111, 113):
                    raise
                print(f"\nConnection interrupted ({error}); retrying from {offset} bytes…", flush=True)
                time.sleep(retry_delay)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("host")
    parser.add_argument("image", type=Path)
    parser.add_argument("--port", type=int, default=4246)
    parser.add_argument("--session", required=True, help="session ID printed by the builtin")
    args = parser.parse_args()
    try:
        session = bytes.fromhex(args.session)
        key = bytes.fromhex(getpass.getpass("Temporary backup key: ").strip())
        if len(session) != 16 or len(key) != 32:
            raise ValueError("session must be 32 hex digits and key 64 hex digits")
        download(args.host, args.port, args.image, session, key)
    except KeyboardInterrupt:
        print("\nPaused. Run the same command to resume while the builtin remains open.")
    except (ProtocolError, ValueError, OSError, KeyError) as error:
        parser.exit(1, f"backup: {error}\n")


if __name__ == "__main__":
    main()
