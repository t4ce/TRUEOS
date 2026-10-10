#!/usr/bin/env python3
"""Repeat the Matrix hardware scenario and small native input updates.

Requires an already deployed, identified kernel and a visible Shell3 frame.
Mutates only test input/history; writes a phase protocol beside the captured log.
"""
import argparse
import json
from pathlib import Path
import socket
import struct
import time


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--host',default='192.168.178.94')
    parser.add_argument('--output',type=Path,required=True)
    args=parser.parse_args()
    args.output.mkdir(parents=True,exist_ok=True)
    events=[];sequence=int(time.time())&0xffffffff;device=0x7363
    udp=socket.socket(socket.AF_INET,socket.SOCK_DGRAM)
    def event(kind,payload,label):
        nonlocal sequence
        sequence=(sequence+1)&0xffffffff
        udp.sendto(b'THID'+struct.pack('<BBHIHH',1,kind,0,sequence,device,0)+payload,(args.host,100))
        events.append({'sequence':sequence,'kind':kind,'action':label,'host_ns':time.time_ns()})
    def tablet(wheel=0,buttons=0):
        event(3,struct.pack('<IIIh',32768,32768,buttons,wheel),f'wheel={wheel} buttons={buttons}')
    def key(code):
        event(2,bytes([0,0,code,0,0,0,0,0]),f'key={code}')
        time.sleep(.06)
        event(2,bytes(8),'key-release')
        time.sleep(.06)
    def phase(name,seconds=0):
        events.append({'phase':name,'host_ns':time.time_ns()});print(name,flush=True)
        if seconds:time.sleep(seconds)

    phase('focus');tablet(buttons=1);time.sleep(.15);tablet();time.sleep(1)
    phase('seed-300-rows')
    stream=socket.create_connection((args.host,22),3);stream.settimeout(.1);raw=bytearray()
    def drain(seconds):
        end=time.monotonic()+seconds
        while time.monotonic()<end:
            try:raw.extend(stream.recv(65536))
            except TimeoutError:pass
    drain(.5)
    # Plain nc starts ADM. Select HV and the shared default Matrix explicitly.
    stream.sendall('\t§\n'.encode());drain(.5)
    stream.sendall(('\n'.join(['preserve','eject','probe','status']*75)+'\n').encode());drain(2)
    (args.output/'seed.raw').write_bytes(raw);stream.close()
    phase('idle-font1',10)
    phase('type-20-z')
    for _ in range(20):key(29)
    phase('erase-20-z')
    for _ in range(20):key(42)
    phase('one-row-scroll-up-12')
    for _ in range(12):tablet(-1);time.sleep(.25)
    phase('one-row-scroll-down-12')
    for _ in range(12):tablet(1);time.sleep(.25)
    phase('both-history-limits')
    for wheel in [-1000,-1,1,1000,1]:tablet(wheel);time.sleep(.7)
    phase('font2');key(68);time.sleep(2)
    phase('idle-font2',6)
    phase('font2-type-erase');key(29);key(42)
    phase('font2-scroll');tablet(-1);time.sleep(.7);tablet(1);time.sleep(.7)
    phase('restore-font1');key(68);time.sleep(2)
    phase('complete')
    (args.output/'events.json').write_text(json.dumps(events,indent=2)+'\n')
    (args.output/'protocol.json').write_text(json.dumps({'host':args.host,'device':device,
        'fixture_rows':300,'fixture_retained_rows':256,'frame_expected':'600x275; font1 100x22 Matrix',
        'phases':[e for e in events if 'phase' in e]},indent=2)+'\n')


if __name__=='__main__':main()
