"""Local NVIDIA-compatible preview of the maintained shader source.
Run with pyopencl, numpy, pygame-ce and Pillow installed.
C++ template dispatch is lowered to an equivalent runtime Mode parameter;
no effect equations are copied. This does not use the Intel SPIR-V artifact.
"""
from pathlib import Path
import re, time, os
import numpy as np
import pyopencl as cl
import pygame
from PIL import Image

HERE=Path(__file__).resolve().parent
ROOT=HERE.parents[2]
source=(ROOT/'crates/trueos-shader/gpgpu/kernels/spirit_vfx_sprite_rgba8.clcpp').read_text()
source=source.replace('#include "include/trueos_clcpp.hpp"','')
source=source.replace('struct SpiritCppSpriteEdges {','typedef struct SpiritCppSpriteEdges {').replace('    float inner;\n};','    float inner;\n} SpiritCppSpriteEdges;')
source=source.replace('return SpiritCppSpriteEdges {','return (SpiritCppSpriteEdges) {')
source=source.replace('template <uint Mode>\n','').replace('vfx_cpp_sprite_layer(\n','vfx_cpp_sprite_layer(\n    uint Mode,\n')
source=re.sub(r'    static_assert\([^;]+;\n','',source)
source=source.replace('if constexpr','if')
source=re.sub(r'vfx_cpp_sprite_layer<(\d+u)>\(',r'vfx_cpp_sprite_layer(\1, ',source)
source=source.replace('__attribute__((intel_reqd_sub_group_size(16)))','')
context=cl.Context(devices=[d for p in cl.get_platforms() for d in p.get_devices(device_type=cl.device_type.GPU)][:1])
queue=cl.CommandQueue(context)
program=cl.Program(context,source).build(options=['-cl-std=CL1.2'])
kernel=program.spirit_vfx_sprite_rgba8
print('GPU:',context.devices[0].name,flush=True)
main=(HERE/'main.c').read_text()
block=main.split('SPRITE_PRESETS[EFFECT_COUNT] = {',1)[1].split('\n};',1)[0]
presets=[]
for name,params,a,b in re.findall(r'\{"([^"]+)",\s*\{([^}]+)\},\s*\{([^}]+)\},\s*\{([^}]+)\}',block):
    presets.append((name,[float(v.strip().rstrip('f')) for v in params.split(',')],[[int(v.strip(),16) for v in c.split(',')] for c in (a,b)]))
folder=Path('/home/t4ce/Repositories/LilyUp/PNG_shared32_cropped')
paths=[folder/n for n in ['static.png','uncrossed.png','reading-book-v2.png']]
frames=[]
for path in paths:
    im=Image.open(path).convert('RGBA')
    canvas=Image.new('RGBA',(256,256))
    canvas.paste(im,((256-im.width)//2,256-im.height))
    pixels=np.array(canvas,dtype=np.uint8)
    frames.append((canvas.size,cl.Buffer(context,cl.mem_flags.READ_ONLY|cl.mem_flags.COPY_HOST_PTR,hostbuf=pixels)))
control=np.zeros(32,dtype=np.uint32)
floats=control.view(np.float32)
control[0]=0x53564658;control[1]=1
floats[13]=1.;floats[14]=0.;floats[15]=.02
control[27]=1024;control[28]=1;control[30]=60;floats[31]=0
cb=cl.Buffer(context,cl.mem_flags.READ_ONLY,control.nbytes)
out=np.empty((256,256,4),dtype=np.uint8)
ob=cl.Buffer(context,cl.mem_flags.WRITE_ONLY,out.nbytes)
pygame.init()
screen=pygame.display.set_mode((1080,1190),pygame.RESIZABLE)
pygame.display.set_caption('LilyUp shared 32 · Sprite VFX · 1/2/3 choose frame · Space pause · Esc close')
font=pygame.font.Font(None,21)
clock=pygame.time.Clock()
frame=0;running=True;paused=False;t=0.;saved_preview=False
background=pygame.Surface((256,256))
for y in range(0,256,16):
 for x in range(0,256,16):
  pygame.draw.rect(background,(35,40,52) if (x//16+y//16)%2 else (44,50,63),(x,y,16,16))
while running:
    dt=clock.tick(30)/1000
    for event in pygame.event.get():
        if event.type==pygame.QUIT:running=False
        if event.type==pygame.KEYDOWN:
            if event.key==pygame.K_ESCAPE:running=False
            if event.key in (pygame.K_1,pygame.K_2,pygame.K_3):frame=event.key-pygame.K_1
            if event.key==pygame.K_SPACE:paused=not paused
    if not paused:t+=dt
    screen.fill((20,23,31))
    screen.blit(font.render(f'{paths[frame].stem} | 1: crossed  2: uncrossed  3: reading | Space: pause | Esc: close',True,(235,235,245)),(12,10))
    (width,height),sb=frames[frame]
    control[24]=width;control[25]=height;control[26]=width*4
    floats[4]=t
    for effect,(name,params,colors) in enumerate(presets):
        control[17]=effect;floats[18:22]=params
        for idx,c in enumerate(colors):control[22+idx]=c[0]|c[1]<<8|c[2]<<16
        cl.enqueue_copy(queue,cb,control)
        kernel(queue,(256,256),(16,1),sb,cb,ob)
        cl.enqueue_copy(queue,out,ob).wait()
        # Shader produces premultiplied BGRA; SDL surface expects straight RGBA.
        rgba=out[:,:,[2,1,0,3]].copy()
        alpha=rgba[:,:,3:4].astype(np.float32)
        rgba[:,:,:3]=np.minimum(255,rgba[:,:,:3].astype(np.float32)*255/np.maximum(alpha,1)).astype(np.uint8)
        tile=background.copy()
        tile.blit(pygame.image.frombuffer(rgba.tobytes(),(256,256),'RGBA'),(0,0))
        x=12+(effect%4)*268;y=43+(effect//4)*284
        screen.blit(tile,(x,y))
        screen.blit(font.render(f'{effect:02} {name}',True,(225,220,240)),(x,y+260))
    pygame.display.flip()
    if not saved_preview:
        pygame.image.save(screen, str(folder.parent/"shared32-vfx-demo.png"))
        saved_preview=True
pygame.quit()
