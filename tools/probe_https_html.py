#!/usr/bin/env python3
"""Download public pages through the rig's Shell2 hyper, then inspect saved bytes.

Each run uses unique filenames and requires a matching save receipt and a newly
listed TRUEOSFS artifact. This checks transport/HTML only, never Solara rendering.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import socket
import subprocess
import time

SITES = ['https://' + host + '/' for host in (
    'www.wikipedia.org', 'wikipedia.com', 'en.wikipedia.org', 'www.wikimedia.org',
    'www.python.org', 'www.rust-lang.org', 'www.mozilla.org', 'www.gnu.org',
    'www.debian.org', 'www.kernel.org', 'www.w3.org', 'www.ietf.org',
    'www.rfc-editor.org', 'www.w3schools.com', 'www.example.com', 'example.org',
    'www.openstreetmap.org', 'www.archive.org', 'www.nasa.gov', 'www.bbc.com',
    'www.cern.ch', 'www.mit.edu', 'www.stanford.edu', 'www.harvard.edu',
    'www.freebsd.org', 'www.netbsd.org', 'www.openbsd.org', 'www.sqlite.org',
    'www.postgresql.org', 'www.curl.se')]
ANSI = re.compile(rb'\x1b\[[0-?]*[ -/]*[@-~]|\x1b\][^\x07]*\x07')


def download(url):
    return subprocess.run(['curl', '--noproxy', '*', '--fail', '--silent', '--show-error',
                           '--retry', '3', '--retry-all-errors', '--max-time', '20', url],
                          check=True, capture_output=True).stdout


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--host', default='192.168.178.94')
    ap.add_argument('--url', action='append')
    ap.add_argument('--output', type=Path, required=True)
    args = ap.parse_args()
    args.output.mkdir(parents=True, exist_ok=True)
    run = str(time.time_ns())
    s = socket.create_connection((args.host, 4245), 10)
    s.settimeout(0.5)

    def read_for(seconds, completion=None):
        end = time.monotonic() + seconds
        out = b''
        while time.monotonic() < end:
            try:
                data = s.recv(65536)
                if not data:
                    raise RuntimeError('Shell2 disconnected')
                if b'\x1b[18t' in data:
                    s.sendall(b'\x1b[8;40;160t')
                out += data
                plain = ANSI.sub(b'', out)
                if completion and (re.search(rb'hyper: saved \d+ bytes -> '+re.escape(completion.encode()), plain) or b'hyper: download failed:' in plain or b'hyper: write failed:' in plain):
                    return out
            except TimeoutError:
                pass
        return out

    read_for(2)
    s.sendall('§httpq\r'.encode())
    read_for(1)
    results = []
    for index, url in enumerate(args.url or SITES):
        name = f'httpq-{run}-{index:02}.html'
        s.sendall('§§\r'.encode())
        read_for(1)
        s.sendall(f'hyper {url} {name}\r'.encode())
        start = time.monotonic()
        raw = read_for(100, name)
        (args.output / f'{index:02}.shell.raw').write_bytes(raw)
        plain = ANSI.sub(b'', raw).decode('utf-8', 'replace')
        result = dict(url=url, file=name, elapsed_s=round(time.monotonic()-start, 2), valid_html=False)
        saved = re.search(r'hyper: saved (\d+) bytes -> '+re.escape(name), plain)
        if saved:
            result['saved_bytes'] = int(saved.group(1))
        try:
            if not saved:
                failure = re.search(r'hyper: (?:download|write) failed: ([^\r\n]+)', plain)
                raise RuntimeError(failure.group(1)[:240] if failure else 'no completion receipt')
            tree = download(f'http://{args.host}/').decode()
            (args.output / 'tree.html').write_text(tree)
            # The actual link both proves presence and supplies the mounted root.
            links = re.findall(r'(?:href|data-download)=["\']([^"\']+)["\']', tree)
            link = next((v for v in links if '/dl/' in v and v.endswith('/'+name)), None)
            if link is None:
                # Tree actions carry root/path attributes rather than direct links.
                match = re.search(r'data-root=["\'](\d+)["\'][^>]*data-path=["\']'+re.escape(name)+r'["\']', tree)
                if not match:
                    raise RuntimeError('new artifact absent from discovered filesystem tree')
                link = f'/dl/{match.group(1)}/{name}'
            body = download(f'http://{args.host}'+link)
            if len(body) != int(saved.group(1)):
                raise RuntimeError('download size differs from save receipt')
            (args.output / f'{index:02}.html').write_bytes(body)
            result.update(bytes=len(body), sha256=hashlib.sha256(body).hexdigest(),
                          valid_html=bool(re.search(rb'<(?:!doctype\s+html|html\b)', body[:65536], re.I)))
            if not result['valid_html']:
                result['error'] = 'saved response lacks an HTML document signature'
        except Exception as exc:
            result['verification_error' if saved else 'error'] = str(exc)
        results.append(result)
        (args.output / 'results.json').write_text(json.dumps(results, indent=2)+'\n')
        print(json.dumps(result), flush=True)
    s.close()


if __name__ == '__main__':
    main()
