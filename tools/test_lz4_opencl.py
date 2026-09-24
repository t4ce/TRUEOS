#!/usr/bin/env python3
"""Run the production LZ4 SPIR-V on a local OpenCL GPU (no TRUEOS rig access).

This checks the shader on a driver-managed GPU. It does not validate TRUEOS's
GuC submission, PPGTT ownership, retirement, or the pinned ADL-S native image.
"""
import ctypes as c
import os
from pathlib import Path
import struct
import time

ROOT=Path(__file__).resolve().parents[1]

def main():
    cl=c.CDLL('libOpenCL.so.1')
    ptr=c.c_void_p; uint=c.c_uint; size=c.c_size_t; integer=c.c_int; ulong=c.c_ulonglong
    def api(name, args, result=integer):
        f=getattr(cl,name); f.argtypes=args; f.restype=result; return f
    platforms=api('clGetPlatformIDs',[uint,c.POINTER(ptr),c.POINTER(uint)])
    devices=api('clGetDeviceIDs',[ptr,ulong,uint,c.POINTER(ptr),c.POINTER(uint)])
    info=api('clGetDeviceInfo',[ptr,uint,size,ptr,c.POINTER(size)])
    context=api('clCreateContext',[ptr,uint,c.POINTER(ptr),ptr,ptr,c.POINTER(integer)],ptr)
    queue=api('clCreateCommandQueue',[ptr,ptr,ulong,c.POINTER(integer)],ptr)
    program=api('clCreateProgramWithIL',[ptr,ptr,size,c.POINTER(integer)],ptr)
    build=api('clBuildProgram',[ptr,uint,c.POINTER(ptr),c.c_char_p,ptr,ptr])
    build_info=api('clGetProgramBuildInfo',[ptr,ptr,uint,size,ptr,c.POINTER(size)])
    kernel=api('clCreateKernel',[ptr,c.c_char_p,c.POINTER(integer)],ptr)
    buffer=api('clCreateBuffer',[ptr,ulong,size,ptr,c.POINTER(integer)],ptr)
    arg=api('clSetKernelArg',[ptr,uint,size,ptr])
    launch=api('clEnqueueNDRangeKernel',[ptr,ptr,uint,ptr,c.POINTER(size),c.POINTER(size),uint,ptr,c.POINTER(ptr)])
    read=api('clEnqueueReadBuffer',[ptr,ptr,uint,size,size,ptr,uint,ptr,ptr])
    finish=api('clFinish',[ptr])
    release_mem=api('clReleaseMemObject',[ptr])
    release_event=api('clReleaseEvent',[ptr])
    profile=api('clGetEventProfilingInfo',[ptr,uint,size,ptr,ptr])
    ps=(ptr*16)(); count=uint(); assert platforms(16,ps,c.byref(count))==0
    selected=None
    for platform in ps[:count.value]:
        ds=(ptr*16)(); dc=uint()
        if devices(platform,4,16,ds,c.byref(dc))!=0: continue
        for device in ds[:dc.value]:
            name=c.create_string_buffer(256); info(device,0x102b,256,name,None)
            if b'Intel' in name.value: selected=ptr(device); print('GPU:',name.value.decode()); break
        if selected: break
    if not selected: raise SystemExit('No Intel OpenCL GPU; use an Intel ICD via OCL_ICD_VENDORS.')
    error=integer(); ctx=context(None,1,c.byref(selected),None,None,c.byref(error)); assert error.value==0,error.value
    q=queue(ctx,selected,2,c.byref(error)); assert error.value==0,error.value
    il=(ROOT/'crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp/lz4_blocks.spv').read_bytes()
    prog=program(ctx,il,len(il),c.byref(error)); assert error.value==0,error.value
    rc=build(prog,1,c.byref(selected),b'',None,None)
    if rc:
        log=c.create_string_buffer(65536);build_info(prog,selected,0x1183,len(log),log,None);raise RuntimeError(log.value.decode())
    kern=kernel(prog,b'lz4_blocks',c.byref(error));assert error.value==0,error.value
    lib=c.CDLL('liblz4.so.1')
    lib.LZ4_decompress_safe.argtypes=[ptr,ptr,integer,integer]
    lib.LZ4_compress_default.argtypes=[ptr,ptr,integer,integer]
    def execute(inputs, capacities, encode):
        packed=b''.join(inputs); output_bytes=sum(capacities); desc=[]; si=0; di=0
        for source,cap in zip(inputs,capacities):
            desc.extend([si,len(source),di,cap,0,1]);si+=len(source);di+=cap
        rawdesc=struct.pack('<'+'I'*len(desc),*desc)
        host=[c.create_string_buffer(packed or b'\0'),c.create_string_buffer(output_bytes or 1),c.create_string_buffer(rawdesc),c.create_string_buffer(((len(inputs)+15)//16)*4096*4)]
        lengths=[max(1,len(packed)),max(1,output_bytes),len(rawdesc),len(host[3])]
        buffers=[]
        try:
            for i,(data,length) in enumerate(zip(host,lengths)):
                mem=ptr(buffer(ctx,1|32,length,data,c.byref(error)));assert error.value==0,error.value
                buffers.append(mem); assert arg(kern,i,c.sizeof(ptr),c.byref(mem))==0
            for i,value in [(4,len(inputs)),(5,0 if encode else 1)]:
                value=uint(value); assert arg(kern,i,4,c.byref(value))==0
            global_size=size(((len(inputs)+15)//16)*16);local_size=size(16);event=ptr()
            started=time.perf_counter();assert launch(q,kern,1,None,c.byref(global_size),c.byref(local_size),0,None,c.byref(event))==0
            assert finish(q)==0
            elapsed=(time.perf_counter()-started)*1000
            a=ulong();b=ulong();assert profile(event,0x1282,8,c.byref(a),None)==0;assert profile(event,0x1283,8,c.byref(b),None)==0
            release_event(event)
            for i in [1,2]:assert read(q,buffers[i],1,0,lengths[i],host[i],0,None,None)==0
            result=struct.unpack('<'+'I'*len(desc),host[2].raw[:len(rawdesc)])
            outputs=[]
            for i,cap in enumerate(capacities):
                d=result[i*6:i*6+6];assert d[5]==0,(i,d);assert d[4]<=cap
                outputs.append(host[1].raw[d[2]:d[2]+d[4]])
            print(f'{"encode" if encode else "decode"}: {len(inputs)} blocks, {len(packed)} input bytes, GPU {(b.value-a.value)/1e6:.3f} ms, submit/wait {elapsed:.3f} ms')
            return outputs
        finally:
            for mem in buffers:release_mem(mem)
    for data in [b'a'*1048576,(b'hello shader LZ4\n'*65536)[:1048576],os.urandom(1048576)]:
        chunks=[data[i:i+4096] for i in range(0,len(data),4096)]
        encoded=execute(chunks,[len(b)+len(b)//255+16 for b in chunks],True)
        for source,compressed in zip(chunks,encoded):
            out=c.create_string_buffer(len(source));assert lib.LZ4_decompress_safe(compressed,out,len(compressed),len(source))==len(source);assert out.raw==source
        assert execute(encoded,[len(b) for b in chunks],False)==chunks
        reference=[]
        for source in chunks:
            out=c.create_string_buffer(len(source)+len(source)//255+16)
            n=lib.LZ4_compress_default(source,out,len(source),len(out));assert n>0;reference.append(out.raw[:n])
        assert execute(reference,[len(b) for b in chunks],False)==chunks
    for name,obj in [('clReleaseKernel',kern),('clReleaseProgram',prog),('clReleaseCommandQueue',q),('clReleaseContext',ctx)]:api(name,[ptr])(obj)
    print('Production SPIR-V: GPU encode/decode and reference-LZ4 cross-check passed')
if __name__=='__main__':main()
