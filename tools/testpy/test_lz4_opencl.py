#!/usr/bin/env python3
"""Run the production LZ4 SPIR-V on a local OpenCL GPU (no TRUEOS rig access).

Use --native to execute the pinned ADL-S image instead of compiling SPIR-V,
and --frame PATH to compare every compressed block of an independent LZ4
frame against liblz4. This does not validate TRUEOS's GuC submission, PPGTT
ownership, context switching, or retirement.
"""
import argparse
import ctypes as c
import os
from pathlib import Path
import struct
import time

ROOT=Path(__file__).resolve().parents[2]

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--native',action='store_true')
    parser.add_argument('--cooperative',action='store_true',help='test the candidate SIMD-group-per-block decoder')
    parser.add_argument('--artifact-dir',type=Path,default=ROOT/'crates/trueos-shader/gpgpu/kernels/artifacts/adls/cpp')
    parser.add_argument('--frame',type=Path)
    parser.add_argument('--repeat',type=int,default=1)
    parser.add_argument('--batch-bytes',type=int,default=16777216,help='frame dispatch capacity (16777216 matches the cooperative TRUEOS path)')
    options=parser.parse_args()
    if options.repeat < 1: parser.error('--repeat must be positive')
    batch_limit=268435456 if options.cooperative else 4194304
    if not 1 <= options.batch_bytes <= batch_limit: parser.error(f'--batch-bytes must be between 1 and {batch_limit}')
    kernel_name='lz4_decode_cooperative' if options.cooperative else 'lz4_blocks'
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
    il=(options.artifact_dir/f'{kernel_name}.spv').read_bytes()
    if options.native:
        native=(options.artifact_dir/f'{kernel_name}.bin').read_bytes()
        binary=c.create_string_buffer(native); binary_ptr=c.cast(binary,ptr);binary_size=size(len(native));status=integer()
        create_binary=api('clCreateProgramWithBinary',[ptr,uint,c.POINTER(ptr),c.POINTER(size),c.POINTER(ptr),c.POINTER(integer),c.POINTER(integer)],ptr)
        prog=create_binary(ctx,1,c.byref(selected),c.byref(binary_size),c.byref(binary_ptr),c.byref(status),c.byref(error))
        assert error.value==0 and status.value==0,(error.value,status.value)
    else:
        prog=program(ctx,il,len(il),c.byref(error)); assert error.value==0,error.value
    rc=build(prog,1,c.byref(selected),b'',None,None)
    if rc:
        log=c.create_string_buffer(65536);build_info(prog,selected,0x1183,len(log),log,None);raise RuntimeError(f'clBuildProgram={rc}: {log.value.decode()}')
    if options.native:
        # Zebin also embeds SPIR-V. A driver can silently rebuild for another
        # product; prove that this test really executed the pinned EU code.
        program_info=api('clGetProgramInfo',[ptr,uint,size,ptr,c.POINTER(size)])
        binary_length=size()
        assert program_info(prog,0x1165,c.sizeof(size),c.byref(binary_length),None)==0
        built=c.create_string_buffer(binary_length.value);built_ptr=c.cast(built,ptr)
        assert program_info(prog,0x1166,c.sizeof(ptr),c.byref(built_ptr),None)==0
        def kernel_text(elf):
            assert elf[:6]==b'\x7fELF\x02\x01','expected little-endian ELF64 Zebin'
            shoff=struct.unpack_from('<Q',elf,40)[0]
            shsize,shnum,shstr=struct.unpack_from('<HHH',elf,58)
            headers=[struct.unpack_from('<IIQQQQIIQQ',elf,shoff+i*shsize) for i in range(shnum)]
            strings=elf[headers[shstr][4]:headers[shstr][4]+headers[shstr][5]]
            for header in headers:
                name=strings[header[0]:].split(b'\0',1)[0]
                if name==f'.text.{kernel_name}'.encode(): return elf[header[4]:header[4]+header[5]]
            raise AssertionError('missing native kernel text')
        assert kernel_text(native)==kernel_text(built.raw),'driver rebuilt the native image; pinned-code test is not valid on this GPU'
        print('Pinned EU instruction bytes preserved by the OpenCL driver')
    kern=kernel(prog,kernel_name.encode(),c.byref(error));assert error.value==0,error.value
    lib=c.CDLL('liblz4.so.1')
    lib.LZ4_decompress_safe.argtypes=[ptr,ptr,integer,integer]
    lib.LZ4_compress_default.argtypes=[ptr,ptr,integer,integer]
    timings=[]
    staged_timings=[]
    def execute(inputs, capacities, encode, report=True, expect_failure=False):
        staged_start=time.perf_counter()
        assert not (options.cooperative and encode),'cooperative kernel only decodes'
        packed=b''.join(inputs); output_bytes=sum(capacities); desc=[]; si=0; di=0
        for source,cap in zip(inputs,capacities):
            desc.extend([si,len(source),di,cap,0,1]);si+=len(source);di+=cap
        rawdesc=struct.pack('<'+'I'*len(desc),*desc)
        host=[c.create_string_buffer(packed or b'\0'),c.create_string_buffer(output_bytes or 1),c.create_string_buffer(rawdesc),c.create_string_buffer(((len(inputs)+15)//16)*4096*4)]
        lengths=[max(1,len(packed)),max(1,output_bytes),len(rawdesc),len(host[3])]
        if options.cooperative: host=host[:3];lengths=lengths[:3]
        buffers=[]
        try:
            for i,(data,length) in enumerate(zip(host,lengths)):
                mem=ptr(buffer(ctx,1|32,length,data,c.byref(error)));assert error.value==0,error.value
                buffers.append(mem); assert arg(kern,i,c.sizeof(ptr),c.byref(mem))==0
            arguments=[(3,len(inputs))] if options.cooperative else [(4,len(inputs)),(5,0 if encode else 1)]
            for i,value in arguments:
                value=uint(value); assert arg(kern,i,4,c.byref(value))==0
            global_size=size(len(inputs)*16 if options.cooperative else ((len(inputs)+15)//16)*16);local_size=size(16);event=ptr()
            started=time.perf_counter();assert launch(q,kern,1,None,c.byref(global_size),c.byref(local_size),0,None,c.byref(event))==0
            assert finish(q)==0
            elapsed=(time.perf_counter()-started)*1000
            a=ulong();b=ulong();assert profile(event,0x1282,8,c.byref(a),None)==0;assert profile(event,0x1283,8,c.byref(b),None)==0
            release_event(event)
            for i in [1,2]:assert read(q,buffers[i],1,0,lengths[i],host[i],0,None,None)==0
            result=struct.unpack('<'+'I'*len(desc),host[2].raw[:len(rawdesc)])
            output_data=host[1].raw[:output_bytes]
            outputs=[]
            for i,cap in enumerate(capacities):
                d=result[i*6:i*6+6]
                if expect_failure:
                    assert d[5]!=0 and d[4]==0,(i,d)
                else:
                    assert d[5]==0,(i,d);assert d[4]<=cap
                outputs.append(output_data[d[2]:d[2]+d[4]])
            gpu_ms=(b.value-a.value)/1e6
            timings.append(gpu_ms)
            staged_timings.append((time.perf_counter()-staged_start)*1000)
            if report: print(f'{"encode" if encode else "decode"}: {len(inputs)} blocks, {len(packed)} input bytes, GPU {gpu_ms:.3f} ms, submit/wait {elapsed:.3f} ms')
            return outputs
        finally:
            for mem in buffers:release_mem(mem)
    if options.frame:
        read_start=time.perf_counter()
        frame=options.frame.read_bytes()
        read_ms=(time.perf_counter()-read_start)*1000
        assert frame[:4]==b'\x04\x22\x4d\x18','expected a standard LZ4 frame'
        flags,bd=frame[4:6]
        assert flags&0xe0==0x60 and not flags&1,'expected independent blocks without a dictionary'
        maximum={4:65536,5:262144,6:1048576,7:4194304}[(bd>>4)&7]
        offset=6+(8 if flags&8 else 0)+1
        blocks=[]; raw_blocks=0
        while True:
            value=struct.unpack_from('<I',frame,offset)[0];offset+=4
            if not value: break
            n=value&0x7fffffff
            assert 0<n<=maximum and offset+n<=len(frame),'truncated or oversized block'
            source=frame[offset:offset+n];offset+=n+(4 if flags&16 else 0)
            blocks.append(None if value&0x80000000 else source)
            raw_blocks+=bool(value&0x80000000)
        assert offset+(4 if flags&4 else 0)==len(frame),'trailing or missing frame data'
        batch_size=min(256,max(1,options.batch_bytes//maximum))
        for iteration in range(options.repeat):
            timings.clear()
            staged_timings.clear()
            for first in range(0,len(blocks),batch_size):
                inputs=[b for b in blocks[first:first+batch_size] if b is not None]
                if not inputs: continue
                outputs=execute(inputs,[maximum]*len(inputs),False,False)
                for source,decoded in zip(inputs,outputs):
                    out=c.create_string_buffer(maximum)
                    n=lib.LZ4_decompress_safe(source,out,len(source),maximum)
                    assert n>=0 and decoded==out.raw[:n],(iteration,first,'liblz4 mismatch')
            print(f'Frame run {iteration+1}: {len(blocks)-raw_blocks} compressed blocks verified, {raw_blocks} raw blocks skipped, GPU total {sum(timings):.3f} ms, maximum batch {max(timings,default=0):.3f} ms, staging/dispatch/readback {sum(staged_timings):.3f} ms, initial file read {read_ms:.3f} ms; excludes reference validation, frame parsing/checksum and TRUEOS VM delivery')
    for data in [b'a'*1048576,(b'hello shader LZ4\n'*65536)[:1048576],os.urandom(1048576)]:
        chunks=[data[i:i+4096] for i in range(0,len(data),4096)]
        if not options.cooperative:
            encoded=execute(chunks,[len(b)+len(b)//255+16 for b in chunks],True)
            for source,compressed in zip(chunks,encoded):
                out=c.create_string_buffer(len(source));assert lib.LZ4_decompress_safe(compressed,out,len(compressed),len(source))==len(source);assert out.raw==source
            assert execute(encoded,[len(b) for b in chunks],False)==chunks
        reference=[]
        for source in chunks:
            out=c.create_string_buffer(len(source)+len(source)//255+16)
            n=lib.LZ4_compress_default(source,out,len(source),len(out));assert n>0;reference.append(out.raw[:n])
        assert execute(reference,[len(b) for b in chunks],False)==chunks
    # Truncated extensions, literals and offsets, plus invalid match seeds.
    bad=[b'\xf0',b'\x20x',b'\x10x\x01',b'\x10x\x00\x00',b'\x10x\x02\x00',b'\x1fx\x01\x00']
    execute(bad,[64]*len(bad),False,expect_failure=True)
    # A valid block must also reject an output allocation smaller than its data.
    execute([b'\x20xy'],[1],False,expect_failure=True)
    for name,obj in [('clReleaseKernel',kern),('clReleaseProgram',prog),('clReleaseCommandQueue',q),('clReleaseContext',ctx)]:api(name,[ptr])(obj)
    print(f'Production {"native image" if options.native else "SPIR-V"}: GPU encode/decode and reference-LZ4 cross-check passed')
if __name__=='__main__':main()
