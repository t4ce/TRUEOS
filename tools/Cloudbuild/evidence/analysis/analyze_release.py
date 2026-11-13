#!/usr/bin/env python3
"""Bounded, read-only release introspection. Never executes extracted binaries."""
from pathlib import Path
import ctypes, ctypes.util, hashlib, io, json, struct, tarfile

ROOT = Path(__file__).resolve().parent
SOURCE = ROOT.parent / 'upload/TrueOS-0.0.235(1).lz4'
MAGIC = b'\x04\x22\x4d\x18'
lib = ctypes.CDLL(ctypes.util.find_library('lz4'))
lib.LZ4F_isError.argtypes = [ctypes.c_size_t]
lib.LZ4F_isError.restype = ctypes.c_uint
lib.LZ4F_getErrorName.argtypes = [ctypes.c_size_t]
lib.LZ4F_getErrorName.restype = ctypes.c_char_p
lib.LZ4F_createDecompressionContext.argtypes = [ctypes.POINTER(ctypes.c_void_p), ctypes.c_uint]
lib.LZ4F_createDecompressionContext.restype = ctypes.c_size_t
lib.LZ4F_freeDecompressionContext.argtypes = [ctypes.c_void_p]
lib.LZ4F_freeDecompressionContext.restype = ctypes.c_size_t
lib.LZ4F_decompress.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.POINTER(ctypes.c_size_t), ctypes.c_void_p, ctypes.POINTER(ctypes.c_size_t), ctypes.c_void_p]
lib.LZ4F_decompress.restype = ctypes.c_size_t

def check(n):
    if lib.LZ4F_isError(n): raise ValueError(lib.LZ4F_getErrorName(n).decode())
    return n

def decode_frame(data, max_output=512*1024*1024):
    ctx=ctypes.c_void_p()
    check(lib.LZ4F_createDecompressionContext(ctypes.byref(ctx),100))
    src=ctypes.create_string_buffer(data)
    dst=ctypes.create_string_buffer(1024*1024)
    out=bytearray(); offset=0
    try:
        while True:
            used=ctypes.c_size_t(min(len(data)-offset,1024*1024))
            produced=ctypes.c_size_t(min(len(dst),max_output-len(out)+1))
            if produced.value <= 0: raise ValueError('decode limit')
            hint=check(lib.LZ4F_decompress(ctx,dst,ctypes.byref(produced),ctypes.byref(src,offset),ctypes.byref(used),None))
            offset += used.value
            out.extend(dst.raw[:produced.value])
            if len(out)>max_output: raise ValueError('decode limit')
            if not hint: return bytes(out),offset
            if not used.value and not produced.value: raise ValueError('truncated LZ4 frame')
    finally:
        lib.LZ4F_freeDecompressionContext(ctx)

def sha(b): return hashlib.sha256(b).hexdigest()

def elf_sections(data):
    if data[:6] != b'\x7fELF\x02\x01': raise ValueError('expected ELF64 little endian')
    header=struct.unpack_from('<16sHHIQQQIHHHHHH',data)
    shoff,shentsize,shnum,shstrndx=header[6],header[11],header[12],header[13]
    if shentsize != 64 or shoff+shnum*shentsize>len(data): raise ValueError('section table bounds')
    sections=[struct.unpack_from('<IIQQQQIIQQ',data,shoff+i*shentsize) for i in range(shnum)]
    names_section=sections[shstrndx]
    names=data[names_section[4]:names_section[4]+names_section[5]]
    out=[]
    for i,s in enumerate(sections):
        name=names[s[0]:].split(b'\0',1)[0].decode('utf8','replace')
        out.append(dict(index=i,name=name,type=s[1],flags=s[2],address=s[3],offset=s[4],bytes=s[5],link=s[6],info=s[7],alignment=s[8],entry_bytes=s[9]))
    return out

if __name__=='__main__':
    packed=SOURCE.read_bytes()
    raw, consumed=decode_frame(packed)
    if consumed != len(packed): raise ValueError('unexpected trailing bytes after archive frame')
    (ROOT/'release.tar').write_bytes(raw)
    members=[]
    wanted={'TRUEOS.elf','EFI/BOOT/BOOTX64.EFI','EFI/BOOT/tgl_guc_70.bin'}
    with tarfile.open(fileobj=io.BytesIO(raw),mode='r:') as archive:
        for member in archive:
            record={'name':member.name,'bytes':member.size,'regular_file':member.isfile()}
            if member.isfile() and member.name in wanted and member.size <= 256*1024*1024:
                data=archive.extractfile(member).read()
                if len(data)!=member.size: raise ValueError('truncated member')
                target=ROOT/'extracted'/member.name
                target.parent.mkdir(parents=True,exist_ok=True)
                target.write_bytes(data)
                record['sha256']=sha(data)
            members.append(record)
    kernel=(ROOT/'extracted/TRUEOS.elf').read_bytes()
    report={'attachment':{'filename':SOURCE.name,'bytes':len(packed),'sha256':sha(packed)},'tar_bytes':len(raw),'members':members,'kernel_sections':elf_sections(kernel)}
    (ROOT/'release-inspection.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({k:v for k,v in report.items() if k!='kernel_sections'},indent=2))
    print(json.dumps(report['kernel_sections'],indent=2))
