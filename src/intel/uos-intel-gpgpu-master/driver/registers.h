#ifndef REG_H
#define REG_H

/**
 * GPGPU Register 
 * 
*/

/* useful macros */
#define SHIFT_RIGHT(x,n) ((x) >> (n))
#define ENABLE_MASK_BIT(x) ((0x1UL << x) << 16) // setting mask for bit x 

// Ringbuffer
#define RING_BUFFER_TAIL    0x2030
#define RING_BUFFER_HEAD    0x2034
#define RING_BUFFER_START   0x2038
#define RING_BUFFER_CTL     0x203C
#define MI_MODE             0x209c
#define ACTHD               0x2074

// force wake
#define  FORCEWAKE_MT				    (0xa188) /* multi-threaded */
#define  FORCEWAKE_MEDIA_GEN9			(0xa270)
#define  FORCEWAKE_RENDER_GEN9			(0xa278)
#define  FORCEWAKE_GT_GEN9			    (0xa188)
#define  FORCEWAKE_ACK_MEDIA_GEN9		(0x0D88)
#define  FORCEWAKE_ACK_RENDER_GEN9		(0x0D84)
#define  FORCEWAKE_ACK_GT_GEN9			(0x130044)

/* render mode register */
#define GFX_MODE	    0x229c

// PDP0|1|2|3
#define PDP0_RCSUNIT 0x2270U // Bit 63: PD Load Busy
#define PDP1_RCSUNIT 0x2278U
#define PDP2_RCSUNIT 0x2280U
#define PDP3_RCSUNIT 0x2288U

// PPGTT
#define PDPX_RCSUNIT(x) ((PDP0_RCSUNIT) + ((x)*(0x8U)))

// Error
#define MAIN_ARBITER_ERROR 0x40a0
#define MAIN_ARBITER_ERROR_2 0x40a4
#define MAIN_ARBITER_ERROR_3 0x40a8
#define FAULT_TLB_RD_DATA0 0x4b10 // TLB fault data 0 x
#define FAULT_TLB_RD_DATA1 0x4b14 // TLB fault data 1 x
#define FAULT_REG 0x4094

#define ISR 0x44300 // Interrupt Status Register
#define EIR 0x20b0 // Error Identity Register (used for clearing error, reported in master bit eror in isr)
#define ESR 0x20b8 // Error Status Register
#define EMR 0x20b4 // Error Mask Register (which error from ESR are reported to EIR)
#define ERR 0xb42c // Error Reporting Register

#endif // REG_H
