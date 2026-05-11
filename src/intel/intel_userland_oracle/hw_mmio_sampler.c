#define _GNU_SOURCE

#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <time.h>
#include <unistd.h>

#define FORCEWAKE_RENDER 0x0A278u
#define FORCEWAKE_GT 0x0A188u
#define FORCEWAKE_ACK_RENDER 0x0D84u
#define FORCEWAKE_ACK_GT 0x130044u
#define FORCEWAKE_KERNEL (1u << 0)
#define FORCEWAKE_FALLBACK (1u << 15)
#define RCS_RING_BASE 0x2000u
#define RCS_CS_GPR_BASE (RCS_RING_BASE + 0x600u)
#define MMIO_BYTES (16u * 1024u * 1024u)

typedef struct Reg {
    const char *name;
    uint32_t off;
} Reg;

static const Reg REGS[] = {
    { "forcewake_render_req", FORCEWAKE_RENDER },
    { "forcewake_gt_req", FORCEWAKE_GT },
    { "forcewake_render_ack", FORCEWAKE_ACK_RENDER },
    { "forcewake_gt_ack", FORCEWAKE_ACK_GT },
    { "rcs_tail", RCS_RING_BASE + 0x30 },
    { "rcs_head", RCS_RING_BASE + 0x34 },
    { "rcs_psmi_ctl", RCS_RING_BASE + 0x50 },
    { "rcs_acthd_udw", RCS_RING_BASE + 0x5C },
    { "rcs_dma_fadd_udw", RCS_RING_BASE + 0x60 },
    { "rcs_ipeir", RCS_RING_BASE + 0x64 },
    { "rcs_ipehr", RCS_RING_BASE + 0x68 },
    { "rcs_instdone", RCS_RING_BASE + 0x6C },
    { "rcs_instps", RCS_RING_BASE + 0x70 },
    { "rcs_acthd", RCS_RING_BASE + 0x74 },
    { "rcs_dma_fadd", RCS_RING_BASE + 0x78 },
    { "rcs_hws_pga", RCS_RING_BASE + 0x80 },
    { "rcs_nopid", RCS_RING_BASE + 0x94 },
    { "rcs_mi_mode", RCS_RING_BASE + 0x9C },
    { "rcs_eir", RCS_RING_BASE + 0xB0 },
    { "rcs_esr", RCS_RING_BASE + 0xB8 },
    { "rcs_instpm", RCS_RING_BASE + 0xC0 },
    { "rcs_cs_debug_mode2", RCS_RING_BASE + 0xD8 },
    { "rcs_cs_debug_mode1", RCS_RING_BASE + 0xEC },
    { "rcs_bbstate", RCS_RING_BASE + 0x110 },
    { "rcs_bbaddr", RCS_RING_BASE + 0x140 },
    { "rcs_bbaddr_udw", RCS_RING_BASE + 0x168 },
    { "rcs_execlist_status_lo", RCS_RING_BASE + 0x234 },
    { "rcs_execlist_status_hi", RCS_RING_BASE + 0x238 },
    { "rcs_context_control", RCS_RING_BASE + 0x244 },
    { "rcs_ring_mode_gen7", RCS_RING_BASE + 0x29C },
    { "rcs_execlist_sq_lo", RCS_RING_BASE + 0x510 },
    { "rcs_execlist_sq_hi", RCS_RING_BASE + 0x514 },
    { "rcs_execlist_control", RCS_RING_BASE + 0x550 },
    { "rcs_context_control_ref", RCS_RING_BASE + 0x5A0 },
    { "ts_gpgpu_threads_dispatched_lo", 0x2290 },
    { "ts_gpgpu_threads_dispatched_hi", 0x2294 },
    { "gfx_mode", 0x2520 },
    { "gen8_ring_fault", 0x4094 },
    { "error_gen6", 0x40A0 },
    { "gen8_fault_tlb_data0", 0x4B10 },
    { "gen8_fault_tlb_data1", 0x4B14 },
    { "sc_instdone", 0x7100 },
    { "sc_instdone_extra", 0x7104 },
    { "sc_instdone_extra2", 0x7108 },
    { "gen12_fault_tlb_data0", 0xCEB8 },
    { "gen12_fault_tlb_data1", 0xCEBC },
    { "gen12_ring_fault", 0xCEC4 },
    { "gen12_rcu_mode", 0x14800 },
    { "sampler_instdone", 0xE160 },
    { "row_instdone", 0xE164 },
    { "tdl_thr_status0", 0xE4B8 },
    { "tdl_thr_disp_count", 0xE4BC },
    { "tdl_thr_status1", 0xE5B8 },
    { "tdl_thr_pf_count", 0xE5BC },
    { "tdl_thr_pf_status0", 0xE6B8 },
    { "tdl_thr_pf_status1", 0xE7B8 },
};

static volatile sig_atomic_t stop_requested = 0;

static void on_signal(int sig) {
    (void)sig;
    stop_requested = 1;
}

static uint64_t now_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint64_t)ts.tv_sec * 1000000000ull + (uint64_t)ts.tv_nsec;
}

static uint32_t mask_en(uint32_t v) {
    return v | (v << 16);
}

static uint32_t mask_dis(uint32_t v) {
    return v << 16;
}

static uint32_t rd32(volatile uint8_t *mmio, uint32_t off) {
    return *(volatile uint32_t *)(mmio + off);
}

static void wr32(volatile uint8_t *mmio, uint32_t off, uint32_t value) {
    *(volatile uint32_t *)(mmio + off) = value;
}

static int wait_bits(volatile uint8_t *mmio, uint32_t off, uint32_t mask, uint32_t want) {
    for (int i = 0; i < 20000; ++i) {
        if ((rd32(mmio, off) & mask) == want) {
            return 1;
        }
    }
    return 0;
}

static void sample(FILE *out, volatile uint8_t *mmio, const char *phase, uint64_t seq) {
    const uint32_t acthd_lo = rd32(mmio, RCS_RING_BASE + 0x74);
    const uint32_t acthd_hi = rd32(mmio, RCS_RING_BASE + 0x5C);
    const uint32_t dma_lo = rd32(mmio, RCS_RING_BASE + 0x78);
    const uint32_t dma_hi = rd32(mmio, RCS_RING_BASE + 0x60);
    fprintf(
        out,
        "hw seq=%llu t_ns=%llu phase=%s acthd64=0x%08X%08X dma_fadd64=0x%08X%08X",
        (unsigned long long)seq,
        (unsigned long long)now_ns(),
        phase,
        acthd_hi,
        acthd_lo,
        dma_hi,
        dma_lo
    );
    for (size_t i = 0; i < sizeof(REGS) / sizeof(REGS[0]); ++i) {
        fprintf(out, " %s=0x%08X", REGS[i].name, rd32(mmio, REGS[i].off));
    }
    for (uint32_t i = 0; i < 16; ++i) {
        const uint32_t lo = rd32(mmio, RCS_CS_GPR_BASE + i * 8);
        const uint32_t hi = rd32(mmio, RCS_CS_GPR_BASE + i * 8 + 4);
        fprintf(out, " rcs_gpr%u=0x%08X%08X", i, hi, lo);
    }
    fputc('\n', out);
    fflush(out);
}

int main(int argc, char **argv) {
    const char *resource = "/sys/bus/pci/devices/0000:00:02.0/resource0";
    const char *out_path = NULL;
    unsigned interval_us = 200;

    for (int i = 1; i < argc; ++i) {
        if (strcmp(argv[i], "--resource") == 0 && i + 1 < argc) {
            resource = argv[++i];
        } else if (strcmp(argv[i], "--out") == 0 && i + 1 < argc) {
            out_path = argv[++i];
        } else if (strcmp(argv[i], "--interval-us") == 0 && i + 1 < argc) {
            interval_us = (unsigned)strtoul(argv[++i], NULL, 0);
        } else {
            fprintf(stderr, "usage: %s --out hw_mmio_log.txt [--resource resource0] [--interval-us 200]\n", argv[0]);
            return 2;
        }
    }
    if (!out_path) {
        fprintf(stderr, "missing --out\n");
        return 2;
    }

    FILE *out = fopen(out_path, "a");
    if (!out) {
        fprintf(stderr, "open output failed: %s: %s\n", out_path, strerror(errno));
        return 1;
    }

    int fd = open(resource, O_RDWR | O_SYNC);
    if (fd < 0) {
        fprintf(out, "hw t_ns=%llu open_resource_failed path=\"%s\" errno=%d text=\"%s\"\n",
                (unsigned long long)now_ns(), resource, errno, strerror(errno));
        fclose(out);
        return 1;
    }

    volatile uint8_t *mmio = mmap(NULL, MMIO_BYTES, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
    if (mmio == MAP_FAILED) {
        fprintf(out, "hw t_ns=%llu mmap_resource_failed path=\"%s\" errno=%d text=\"%s\"\n",
                (unsigned long long)now_ns(), resource, errno, strerror(errno));
        close(fd);
        fclose(out);
        return 1;
    }

    signal(SIGTERM, on_signal);
    signal(SIGINT, on_signal);

    fprintf(out, "hw t_ns=%llu sampler-start resource=\"%s\" interval_us=%u pid=%d\n",
            (unsigned long long)now_ns(), resource, interval_us, getpid());

    const uint32_t ack_render_before = rd32(mmio, FORCEWAKE_ACK_RENDER);
    const uint32_t ack_gt_before = rd32(mmio, FORCEWAKE_ACK_GT);
    wr32(mmio, FORCEWAKE_RENDER, mask_dis(FORCEWAKE_KERNEL | FORCEWAKE_FALLBACK));
    int render_cleared = wait_bits(mmio, FORCEWAKE_ACK_RENDER, FORCEWAKE_KERNEL | FORCEWAKE_FALLBACK, 0);
    wr32(mmio, FORCEWAKE_RENDER, mask_en(FORCEWAKE_KERNEL));
    int render_awake = wait_bits(mmio, FORCEWAKE_ACK_RENDER, FORCEWAKE_KERNEL, FORCEWAKE_KERNEL);
    wr32(mmio, FORCEWAKE_GT, mask_en(FORCEWAKE_KERNEL));
    int gt_awake = wait_bits(mmio, FORCEWAKE_ACK_GT, FORCEWAKE_KERNEL, FORCEWAKE_KERNEL);
    fprintf(
        out,
        "hw t_ns=%llu forcewake-acquire ack_render_before=0x%08X ack_gt_before=0x%08X render_cleared=%d render_awake=%d gt_awake=%d ack_render_after=0x%08X ack_gt_after=0x%08X wrote_render_clear=0x%08X wrote_render_set=0x%08X wrote_gt_set=0x%08X\n",
        (unsigned long long)now_ns(),
        ack_render_before,
        ack_gt_before,
        render_cleared,
        render_awake,
        gt_awake,
        rd32(mmio, FORCEWAKE_ACK_RENDER),
        rd32(mmio, FORCEWAKE_ACK_GT),
        mask_dis(FORCEWAKE_KERNEL | FORCEWAKE_FALLBACK),
        mask_en(FORCEWAKE_KERNEL),
        mask_en(FORCEWAKE_KERNEL)
    );

    uint64_t seq = 0;
    sample(out, mmio, "initial", seq++);
    struct timespec sleep_time = {
        .tv_sec = interval_us / 1000000u,
        .tv_nsec = (long)(interval_us % 1000000u) * 1000l,
    };
    while (!stop_requested) {
        sample(out, mmio, "poll", seq++);
        nanosleep(&sleep_time, NULL);
    }
    sample(out, mmio, "final-before-release", seq++);

    wr32(mmio, FORCEWAKE_RENDER, mask_dis(FORCEWAKE_KERNEL));
    wr32(mmio, FORCEWAKE_GT, mask_dis(FORCEWAKE_KERNEL));
    fprintf(
        out,
        "hw t_ns=%llu forcewake-release ack_render=0x%08X ack_gt=0x%08X wrote_render_clear=0x%08X wrote_gt_clear=0x%08X\n",
        (unsigned long long)now_ns(),
        rd32(mmio, FORCEWAKE_ACK_RENDER),
        rd32(mmio, FORCEWAKE_ACK_GT),
        mask_dis(FORCEWAKE_KERNEL),
        mask_dis(FORCEWAKE_KERNEL)
    );
    fprintf(out, "hw t_ns=%llu sampler-stop samples=%llu\n",
            (unsigned long long)now_ns(), (unsigned long long)seq);

    munmap((void *)mmio, MMIO_BYTES);
    close(fd);
    fclose(out);
    return 0;
}
