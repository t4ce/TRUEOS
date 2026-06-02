#ifndef INSTRUCTIONS_H
#define INSTRUCTIONS_H

//////////////////////////////
// Command Types
#define MI_COMMAND              0x0
#define GFXPIPE                 0x3

// Command SybTypes
#define GFXPIPE_COMMON          0x0
#define Media                   0x2
#define GFXPIPE_SINGLE_DW       0x1
#define GFXPIPE_3D              0x3

// Command Opcode
#define GFXPIPE_NONPIPELINED    0x1

// Command SubOpcode

//////////////////////////////

////////// shifters //////////
// CT - Command Type
// CO - Command Opcode
// CST - Command SubType
// CSO - Command SubOpcode

#define CT(x)       (x << 29)

// MI
#define MI_CO(x)    (x << 23)

// 3D
#define _3D_CST(x)  (x << 27)
#define _3D_CO(x)   (x << 24)
#define _3D_CSO(x)  (x << 16)
//////////////////////////////

//////////////////////////////
// instruction creator
#define INT_MI(ct, co, x)               (CT(ct) | MI_CO(co) | x)
#define INT_3D(ct, cst, co, cso, x)     (CT(ct) | _3D_CST(cst) | _3D_CO(co) | _3D_CSO(cso) | x)
//////////////////////////////

//////////////////////////////
// instructions
#define MI_NOOP                             INT_MI(MI_COMMAND, 0x0, 0)
#define MI_NOOP_NOPID(x)                    INT_MI(MI_COMMAND, 0x0, ((0x1 << 22) | x))
#define MI_LOAD_REGISTER_IMM(x)             INT_MI(MI_COMMAND, 0x22, x)
#define MI_BATCH_BUFFER_START(x)            INT_MI(MI_COMMAND, 0x31, x)
#define MI_BATCH_BUFFER_END                 INT_MI(MI_COMMAND, 0xA, 0)
#define PIPE_CONTROL(x)                     INT_3D(GFXPIPE, GFXPIPE_3D, 0x2, 0x0, x)
#define PIPELINE_SELECT(x)                  INT_3D(GFXPIPE, GFXPIPE_SINGLE_DW, GFXPIPE_NONPIPELINED, 0x4, x)
#define STATE_BASE_ADDRESS(x)               INT_3D(GFXPIPE, GFXPIPE_COMMON, GFXPIPE_NONPIPELINED, 0x1, x)
#define STATE_SIP(x)                        INT_3D(GFXPIPE, GFXPIPE_COMMON, GFXPIPE_NONPIPELINED, 0x2, x)
#define MEDIA_VFE_STATE(x)                  INT_3D(GFXPIPE, Media, 0x0, 0x0, x)
#define MEDIA_INTERFACE_DESCRIPTOR_LOAD(x)  INT_3D(GFXPIPE, Media, 0x0, 0x2, x)
#define MEDIA_CURBE_LOAD(x)                 INT_3D(GFXPIPE, Media, 0x0, 0x1, x)
#define MEDIA_STATE_FLUSH(x)                INT_3D(GFXPIPE, Media, 0x0, 0x4, x)
#define MEDIA_POOL_STATE(x)                 INT_3D(GFXPIPE, Media, 0x0, 0x5, x)
#define GPGPU_WALKER(x)                     INT_3D(GFXPIPE, Media, 0x1, 0x5, x)
//////////////////////////////

#endif /* !INSTRUCTIONS_H */

