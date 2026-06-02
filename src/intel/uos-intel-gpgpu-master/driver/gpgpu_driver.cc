#include "../stubs.h"
#include "../cpp_support.h"
#include "gpgpu_driver.h"
#include "ggtt32.h"
#include "ppgtt32.h"
#include "ringbuffer.h"
#include "registers.h"
#include "instructions.h"
#include "wgsize.h"

void GPGPU_Driver::init(uint8_t interrupt_vector)
{
    m_int_vec = interrupt_vector;

    // find Intel UHD Graphics 620 (Kabylake generation)
    // PCI_S::pci.dumpDevices(0x5917, 0x8086);

    // IGP is always at 0/2/0
    m_pci_config_header = calculatePCIConfigHeaderAddress(0, 2, 0);

    // setup MSI and IRQ Handler
    setupMSI(interrupt_vector);
    // plugbox.assignAllCPUs(interrupt_vector, m_irq_handler);

    // read memory info
    uint16_t mggc0 = (uint16_t)readPCIConfigSpace(m_pci_config_header + 0x50);
    printk("MGGC0: 0x%x\n", mggc0); // different meaning in HSW and SKL!

    // 2MB MMIO, 6MB unused, 8MB GTT
    uint64_t gttmmadr = readPCIConfigSpace(m_pci_config_header + 0x10);
    printk("mem addr: 0x%x\n", (gttmmadr & ~0xf));
    printk("prefetchable: 0x%x type: 0x%x io: 0x%x\n", (gttmmadr & 0x8), (gttmmadr & 0x6), (gttmmadr & 0x1));

    // graphics memory region
    uint64_t gmadr = readPCIConfigSpace(m_pci_config_header + 0x18);
    printk("mem addr: 0x%x\n", (gmadr & ~0xf));
    printk("prefetchable: 0x%x type: 0x%x io: 0x%x\n", (gmadr & 0x8), (gmadr & 0x6), (gmadr & 0x1));

    // io ports
    uint32_t iobar = readPCIConfigSpace(m_pci_config_header + 0x20);
    printk("io addr: 0x%x\n", (iobar & ~0xf));
    printk("prefetchable: 0x%x type: 0x%x io: 0x%x\n", (iobar & 0x8), (iobar & 0x6), (iobar & 0x1));

    // uint32_t bdsm = readPCIConfigSpace(m_pci_config_header + 0x5c);
    // printk("BDSM: 0x%x\n", bdsm);

    // the first 2MB in the gttmmadr region are used for MMIO registers
    m_mmadr = gttmmadr & ~0xf;

    // Global GTT entries
    ggtt.init((uint64_t *)(m_mmadr + 0x800000));

    // drivers system buffer
    allocate_system_buffer(&sys_buffer); // allocate system buffer without ring buffer
    sys_buffer.m_ringbuff = (uint32_t *)gpgpu_aligned_alloc(0x1000, 0x1000);

    // driver context
    m_ctx.gtt = &ggtt;
    m_ctx.sys_buffer = &sys_buffer;

    // map buffer for global gtt
    map_system_buffer(&sys_buffer, &ggtt);
    // note: ringbuffer mapping always at the end
    // each gtt has the same va address for system buffers
    sys_buffer.m_ga_ringbuff = ggtt.mapPermanent((uint64_t)sys_buffer.m_ringbuff, 0x1000);

    // initialize MULTIFORCEWAKE 0xA188 to hold the chip awake
    // t.b.d -> see Broadwell register manual
    volatile uint32_t *fw1 = (uint32_t *)(m_mmadr + FORCEWAKE_RENDER_GEN9);            // forcewake Render
    volatile uint32_t *fw2 = (uint32_t *)(m_mmadr + FORCEWAKE_GT_GEN9);                // forcewake GT
    volatile uint32_t *fw3 = (uint32_t *)(m_mmadr + FORCEWAKE_MEDIA_GEN9);             // forcewake Media
    volatile uint32_t *fw1_status = (uint32_t *)(m_mmadr + FORCEWAKE_ACK_RENDER_GEN9); // ack
    volatile uint32_t *fw2_status = (uint32_t *)(m_mmadr + FORCEWAKE_ACK_GT_GEN9);     // ack
    volatile uint32_t *fw3_status = (uint32_t *)(m_mmadr + FORCEWAKE_ACK_MEDIA_GEN9);  // ack

    // should be done by driver during initialization
    // see Doc Ref # IHD-OS-BDW-Vol 2c-11.15 page 493 FORCE_WAKE register description

    *fw1 = 0xffff0000; // clear bits (see BDW register manual)
    *fw2 = 0xffff0000; // clear bits (see BDW register manual and linux driver)
    *fw3 = 0xffff0000; // clear bits (see BDW register manual and linux driver)

    *fw1 = 0x10001; // for wake thread 0, Render
    *fw2 = 0x10001; // for wake thread 0, GT
    *fw3 = 0x10001; // for wake thread 0, Media

    while (*fw1_status != 0x1)
        ;                                                // wait for the effect (first bit set <=> thread 0 is awake)
    printk("Forcewake Render Engine %u\n", *fw1_status); // this should be 0x1

    while (*fw2_status != 0x1)
        ;                                            // wait for the effect (first bit set <=> thread 0 is awake)
    printk("Forcewake GT Engine %u\n", *fw2_status); // this should be 0x1

    while (*fw3_status != 0x1)
        ;                                               // wait for the effect (first bit set <=> thread 0 is awake)
    printk("Forcewake Media Engine %u\n", *fw3_status); // this should be 0x1
    // TODO: sleep thread 0 after setup?

    // get frequency info in units of 50 MHz
    volatile uint32_t *GEN6_RP_STATE_CAP = (uint32_t *)(m_mmadr + 0x145998);
    m_max_freq = (*GEN6_RP_STATE_CAP >> 0) & 0xff; // highest non-oc frequency (RP0)
    // uint32_t rp1 = (*GEN6_RP_STATE_CAP >>  8) & 0xff; // most efficient
    m_min_freq = (*GEN6_RP_STATE_CAP >> 16) & 0xff; // lowest frequency

    // frequency will be set later in units of 16.667 MHz
    m_max_freq *= 3;
    m_min_freq *= 3;

    // init Interrupts
    // volatile uint32_t* ISR = (uint32_t*)(m_mmadr + 0x44300); // Interrupt Status Register
    volatile uint32_t *IMR = (uint32_t *)(m_mmadr + 0x44304); // Interrupt Mask Register
    *IMR &= ~0x90;                                            // unmask PIPE_CONTROL interrupt
    // volatile uint32_t* IIR = (uint32_t*)(m_mmadr + 0x44308); // Interrupt Identity Register
    volatile uint32_t *IER = (uint32_t *)(m_mmadr + 0x4430C);            // Interrupt Enable Register
    *IER |= 0x90;                                                        // enable PIPE_CONTROL interrupt
    volatile uint32_t *imr = (uint32_t *)(m_mmadr + 0x20A8);             // global Interrupt Mask Register
    *imr &= ~0x90;                                                       // unmask pipe control
    volatile uint32_t *master_int_ctl = (uint32_t *)(m_mmadr + 0x44200); // master interrupt control
    *master_int_ctl = 0x80000000;                                        // enable interrupts

    // init moc 0
    uint32_t *moc0 = (uint32_t *)(m_mmadr + 0xc800);
    uint32_t *lncf0 = (uint32_t *)(m_mmadr + 0xb020);
    // moc0: enable write back mode for l3 cache, lrum ?, skip cache control ?, ...
    *moc0 = 0b110111;
    // set l3cc (cache control register for moc index 0)
    *lncf0 = 0x3UL << 20 | 0x3UL << 4;

    // create CommandStreamer
    RingBuffer &rb = RingBuffer::getInstance();
    rb.init(m_mmadr, sys_buffer.m_ringbuff, sys_buffer.m_ga_ringbuff, 0x1000);

    //// Init GPGPU Pipe - Start ////

    // set up Preemption
    rb.enqueue(MI_LOAD_REGISTER_IMM(0x1));
    rb.enqueue(0x00002580); // ?undocumented?
    rb.enqueue(0x00060002);

    // Pipe select
    rb.enqueue(PIPELINE_SELECT(0x302)); // select GPGPU Pipeline

    // set up L3 Cache
    rb.enqueue(MI_LOAD_REGISTER_IMM(0x1));
    rb.enqueue(0x00007034); // L3CNTLREG
    rb.enqueue(0x60000321);

    // flush
    rb.enqueue(PIPE_CONTROL(0x4));
    rb.enqueue(0x00100000); // CS Stall
    rb.enqueue(0x00000000); // address for post sync
    rb.enqueue(0x00000000); // higher address bits
    rb.enqueue(0x00000000); // immediate data
    rb.enqueue(0x00000000); // immediate data

    // DebugRegister
    rb.enqueue(MI_LOAD_REGISTER_IMM(0x1));
    rb.enqueue(0x0000E404); // ?undocumented?
    rb.enqueue(0x00000100);

    // flush
    rb.enqueue(PIPE_CONTROL(0x4));
    rb.enqueue(0x00101021); // Depth Cache, DC, Render Target, CS Stall
    rb.enqueue(0x00000000); // address for post sync
    rb.enqueue(0x00000000); // higher address bits
    rb.enqueue(0x00000000); // immediate data
    rb.enqueue(0x00000000); // immediate data

    // VFE State
    rb.enqueue(MEDIA_VFE_STATE(0x7));
    rb.enqueue(0x00000000); // Scratch Space, Stack Size
    rb.enqueue(0x00000000); // higher bits of Scratch Space
    rb.enqueue(0x00A70100); // max threads, number of URBE, Gateway Control
    rb.enqueue(0x00000000); // all subslices enabled
    rb.enqueue(0x07820000); // URBE alloc size, CURBE alloc size
    rb.enqueue(0x00000000); // scoreboard settings
    rb.enqueue(0x00000000); // scoreboard deltas
    rb.enqueue(0x00000000); // scoreboard deltas

    // flush
    rb.enqueue(PIPE_CONTROL(0x4));
    rb.enqueue(0x00100420); // DC, Texure Cache, CS Stall
    rb.enqueue(0x00000000); // address for post sync
    rb.enqueue(0x00000000); // higher address bits
    rb.enqueue(0x00000000); // immediate data
    rb.enqueue(0x00000000); // immediate data

    // State base address
    rb.enqueue(STATE_BASE_ADDRESS(0x11));
    rb.enqueue(sys_buffer.m_ga_base | 0x1);     // general state base address
    rb.enqueue(0x00000000);                     // higher address bits
    rb.enqueue(0x00040000);                     // MOCS for DataPort (also use index 0 here?)
    rb.enqueue(sys_buffer.m_ga_surf | 0x1);     // surface base address
    rb.enqueue(0x00000000);                     // higher address bits
    rb.enqueue(sys_buffer.m_ga_dynamic | 0x1);  // dynamic base address
    rb.enqueue(0x00000000);                     // higher address bits
    rb.enqueue(sys_buffer.m_ga_indirect | 0x1); // indirect object base address
    rb.enqueue(0x00000000);                     // higher address bits
    rb.enqueue(sys_buffer.m_ga_instr | 0x1);    // instruction base address
    rb.enqueue(0x00000000);                     // higher address bits
    rb.enqueue(0x00001001);                     // general size
    rb.enqueue(0x00001001);                     // dynamic size
    rb.enqueue(0x00001001);                     // indirect size
    uint16_t instrPages = (uint16_t)roundUp(MAX_KERNEL_SIZE, 0x1000);
    rb.enqueue((instrPages << 12) | 0x1); // instruction size
    rb.enqueue(0x00000000);               // bindless surface state base address
    rb.enqueue(0x00000000);               // higher address bits
    rb.enqueue(0x00000000);               // bindless size

    // flush
    rb.enqueue(PIPE_CONTROL(0x4));
    rb.enqueue(0x00100000); // CS Stall
    rb.enqueue(0x00000000); // address for post sync
    rb.enqueue(0x00000000); // higher address bits
    rb.enqueue(0x00000000); // immediate data
    rb.enqueue(0x00000000); // immediate data

    // flush
    rb.enqueue(PIPE_CONTROL(0x4));
    rb.enqueue(0x00100000); // CS Stall
    rb.enqueue(0x00000000); // address for post sync
    rb.enqueue(0x00000000); // higher address bits
    rb.enqueue(0x00000000); // immediate data
    rb.enqueue(0x00000000); // immediate data

    if (!rb.wait())
    {
        printk("Init failed\n");
    }

    //// Init GPGPU Pipe - End ////
}

// TODO: implement proper power management with dynamic frequency scaling and turbo boost
void GPGPU_Driver::setMinFreq()
{
    // set frequency
    volatile uint32_t *GEN6_RPNSWREQ = (uint32_t *)(m_mmadr + 0xA008);
    *GEN6_RPNSWREQ = m_min_freq << 23;
}

void GPGPU_Driver::setMaxFreq()
{
    // set frequency
    volatile uint32_t *GEN6_RPNSWREQ = (uint32_t *)(m_mmadr + 0xA008);
    *GEN6_RPNSWREQ = m_max_freq << 23;
}

void GPGPU_Driver::free()
{
    // free Buffer Mappings
    ggtt.reset();

    // free memory
    free_system_buffer(&sys_buffer);
    ::free(sys_buffer.m_ringbuff);
}

void GPGPU_Driver::setupMSI(uint8_t interrupt_vector)
{
    // this is the MSI
    uint32_t basic = readPCIConfigSpace(m_pci_config_header + 0xAC);

    // Give it all the channels
    uint8_t requested_channels = (basic & (0b111 << 17)) >> 17;
    basic &= (0b111 << 20);
    basic |= (requested_channels << 20);
    writePCIConfigSpace(m_pci_config_header + 0xAC, basic);

    // Programm the address register (this is Intel default MSI Address)
    uint32_t msi_address = 0xfee00000;
    msi_address |= (0b11 << 2);  // logical destination mode
    msi_address |= (0xFF << 12); // Set all CPUs as destination
    writePCIConfigSpace(m_pci_config_header + 0xB0, msi_address);

    // Write the MSI Data
    uint16_t msi_data = interrupt_vector;
    msi_data |= (0b001 << 8); // lowest pri
    msi_data &= ~(0b1 << 15); // edge trigger
    writePCIConfigSpace(m_pci_config_header + 0xB4, msi_data);

    // Enable the MSI
    basic |= (0b1 << 16);
    writePCIConfigSpace(m_pci_config_header + 0xAC, basic);
}

void GPGPU_Driver::handleInterrupt()
{
    volatile uint32_t *IIR = (uint32_t *)(m_mmadr + 0x44308); // Interrupt Identity Register
    if (*IIR & 0x80)                                          // Page Fault
    {
        // clear by writing 1 to PF Interrupt
        *IIR |= 0x80;
        printk("GPU Page Fault\n");
    }
    if (*IIR & 0x10) // GPU Program finished
    {
        // clear by writing 1 to PIPE_CONTROL Interrupt
        *IIR |= 0x10;
    }
}

void GPGPU_Driver::runNext()
{
    // get old kernel and remove it
    kernel_config &kconf_old = *m_current_kernel;

    // clear GTT for next task
    kconf_old.ctx->gtt->clear();

    // run old kernel callback
    kconf_old.finished = true;
    if (kconf_old.finish_callback)
    {
        // enqueue callback
        kconf_old.finish_callback();
        // processor.enqueue(*kconf_old.finish_callback);
    }

    // get new kernel (or set m_current_kernel to NULL)
    m_current_kernel = (kernel_config *)m_task_queue.dequeue();

    // is there a new kernel?
    if (!m_current_kernel)
        return;

    // run new kernel
    run(*m_current_kernel);
}

void GPGPU_Driver::flush()
{
    RingBuffer &rb = RingBuffer::getInstance();

    // flush
    rb.enqueue(PIPE_CONTROL(0x4));
    rb.enqueue(0x001408BC); // CS Stall, TLB invalidate, Instr. Cache Flush, PC Flush, VF Cache invalidate, Constant Cache invalidate, State Cache invalidate
    rb.enqueue(0x00000000); // address for post sync
    rb.enqueue(0x00000000); // higher address bits
    rb.enqueue(0x00000000); // immediate data
    rb.enqueue(0x00000000); // immediate data

    if (!rb.wait())
    {
        printk("flush failed\n");
    }
}

bool GPGPU_Driver::prepareIOBuffers(kernel_config &kconf)
{
    GTT *gtt = kconf.ctx->gtt;

    for (uint8_t i = 0; i < kconf.buffCount; i++)
    {
        // do not map constant memory
        if (kconf.buffConfigs[i].non_pointer_type)
            continue;

        // map physical memory
        kconf.buffConfigs[i].ga = gtt->map((uint64_t)kconf.buffConfigs[i].buffer, kconf.buffConfigs[i].buffer_size);

        // check if map was successful
        if (kconf.buffConfigs[i].ga == MAP_FAILED)
        {
            return false;
        }
    }
    return true;
}

void GPGPU_Driver::enqueueRun(kernel_config &kconf)
{
    // set ggtt if no ppgtt is set
    if (kconf.ctx == nullptr)
    {
        kconf.ctx = &m_ctx;
    }

    // enqueue new kernel
    m_task_queue.enqueue(&kconf);

    // if the gpu is idle and the new kernel is the first in queue
    if (!isGPUTaskRunning() && m_task_queue.first() == &kconf)
    {
        // remove it from queue and run it
        m_current_kernel = (kernel_config *)m_task_queue.dequeue();
        run(kconf);
    }
}

bool GPGPU_Driver::prepareRun(kernel_config &kconf)
{
    // parse binary
    CrossThreadData_info crossInfo;
    parseGEN(crossInfo, kconf, (uint8_t *)kconf.ctx->sys_buffer->m_instr);

    // calculate some stuff
    if (kconf.workgroupsize[0] == 0)
    {
        kconf.workgroupsize[0] = findWorkGroupSize(kconf.range[0], kconf.simd);
        kconf.workgroupsize[1] = 1;
        kconf.workgroupsize[2] = 1;
    }

    // total workgroupsize can be max 256!
    uint32_t workgroupsize_total = kconf.workgroupsize[0] * kconf.workgroupsize[1] * kconf.workgroupsize[2];
    uint32_t workgroupcount[3];
    for (int i = 0; i < 3; i++)
    {
        workgroupcount[i] = roundUp(kconf.range[i], kconf.workgroupsize[i]);
        if (workgroupcount[i] == 0)
            workgroupcount[i] = 1; // need at least one workgroup for every dim
    }
    uint32_t dispatchcount = roundUp(workgroupsize_total, kconf.simd);
    uint32_t rest = workgroupsize_total % kconf.simd;

    // create buffers
    if (!createBuffers(kconf))
    {
        printk("Could not start GPGPU Task: GTT Out of Memory!\n");

        // remove kernel from queue
        kernel_config *kconf_old = (kernel_config *)m_task_queue.dequeue();

        // clear GTT
        kconf_old->ctx->gtt->clear();

        // cancel run
        return false;
    }
    createBindingTable(kconf);

    // Cross Thread Data
    uint32_t *in = kconf.ctx->sys_buffer->m_indirect;
    // number of chunks
    uint32_t crossnum = roundUp(crossInfo.enqueuedWorkDim[2], 8 * sizeof(uint32_t)); // sizeof(cross-chunk) = 8;
    // size of Cross Thread Data in DWORDs
    uint32_t crosssize = crossnum * 8; // sizeof(cross-chunk) = 8

    // clear possible old data
    // memset(in, 0, crosssize * sizeof(uint32_t));
    for (uint32_t i = 0; i < crosssize; i++)
    {
        in[i] = 0;
    }

    // workgroup offset
    /*in[crossInfo.workOffset[0] / sizeof(uint32_t)] = 0; // not used atm
    in[crossInfo.workOffset[1] / sizeof(uint32_t)] = 0;
    in[crossInfo.workOffset[2] / sizeof(uint32_t)] = 0; */

    // workgroup dimensions
    in[crossInfo.workDim[0] / sizeof(uint32_t)] = kconf.workgroupsize[0];
    in[crossInfo.workDim[1] / sizeof(uint32_t)] = kconf.workgroupsize[1];
    in[crossInfo.workDim[2] / sizeof(uint32_t)] = kconf.workgroupsize[2];

    // buffer and parameters
    for (uint8_t i = 0; i < kconf.buffCount; i++)
    {
        if (kconf.buffConfigs[i].non_pointer_type)
        {
            uint8_t *pos = (uint8_t *)&in[kconf.buffConfigs[i].pos / sizeof(uint32_t)];
            uint8_t *data = (uint8_t *)kconf.buffConfigs[i].buffer;

            // copy parameter data
            for (uint32_t s = 0; s < kconf.buffConfigs[i].buffer_size; s++)
                *pos++ = *data++;
        }
        else
        {
            in[kconf.buffConfigs[i].pos / sizeof(uint32_t)] = kconf.buffConfigs[i].ga;
            in[(kconf.buffConfigs[i].pos / sizeof(uint32_t)) + 1] = 0x00000000; // upper address bits
        }
    }

    // enqueued workgroup dimensions
    in[crossInfo.enqueuedWorkDim[0] / sizeof(uint32_t)] = kconf.workgroupsize[0];
    in[crossInfo.enqueuedWorkDim[1] / sizeof(uint32_t)] = kconf.workgroupsize[1];
    in[crossInfo.enqueuedWorkDim[2] / sizeof(uint32_t)] = kconf.workgroupsize[2];

    // increase pointer
    in += crosssize;

    // Thread Payload
    uint16_t *in16 = (uint16_t *)in;
    uint16_t id[3] = {0, 0, 0};
    for (uint32_t p = 0; p < dispatchcount; p++)
    {
        // base address of package
        uint32_t b = p * 3 * kconf.simd;

        for (uint16_t s = 0; s < kconf.simd; s++)
        {
            in16[b + 0 * kconf.simd + s] = id[0]; // x
            in16[b + 1 * kconf.simd + s] = id[1]; // y
            in16[b + 2 * kconf.simd + s] = id[2]; // z

            // increment ids
            id[0]++;
            if (id[0] == kconf.workgroupsize[0]) // edge of workgroup in X-dim reached
            {
                // reset first dim (X)
                id[0] = 0;

                // increment second dim (Y)
                id[1]++;
                if (id[1] == kconf.workgroupsize[1]) // edge of workgroup in Y-dim reached
                {
                    // reset second dim (Y)
                    id[1] = 0;

                    // increment third dim (Z)
                    id[2]++;
                    /*if(id[2] == kconf.workgroupsize[2])
                    {
                        // this will only happen in the last iteration where all ids already written
                        // also, it makes no sense to reset Z
                        id[2] = 0;
                    }*/
                }
            }
        }
    }

    // write INTERFACE_DESCRIPTOR_DATA to dynamic
    uint32_t *config_data = kconf.ctx->sys_buffer->m_dynamic;
    *config_data++ = 0x00000000; // Kernel Start Pointer Offset
    *config_data++ = 0x00000000; // upper address bits for kernel start pointer
    *config_data++ = 0x00080000; // MPF, normal prio, std. floats, disable exceptions
    *config_data++ = 0x00000000; // no samplers (this seems to be for textures etc.)
    uint8_t bndtblcnt = 0;
    for (int i = 0; i < kconf.buffCount; i++)
    {
        if (!kconf.buffConfigs[i].non_pointer_type)
            bndtblcnt++;
    }
    *config_data++ = (16 /*sizeof(RENDER_SURFACE_STATE)*/ * sizeof(int) * bndtblcnt) | bndtblcnt; // binding table address offset, binding table entry count
    if (kconf.simd == 32)
    {
        // URBE len = (SIMD * x,y,z * sizeof(ID)) / (8-DW * sizeof(DW))
        // (32 * 3 * 2) / (8 * 4) = 6
        *config_data++ = 0x00060000; // URBE len, CURBE offset
    }
    else
    {
        // URBE len = (SIMD * x,y,z * sizeof(ID)) / (8-DW * sizeof(DW))
        // (16 * 3 * 2) / (8 * 4) = 3
        // ((8 + 8) * 3 * 2) / (8 * 4) = 3 // seems like one Dim has to take at least 8DWs // TODO: check SIMD8 kernel
        *config_data++ = 0x00030000; // URBE len, CURBE offset
    }

    *config_data = dispatchcount; // round to nearest even, no barrier, 0 shared mem, threadcount in group
    if (kconf.useBarrier)
    {
        *config_data |= 0x1 << 21; // barrier enable bit
    }
    config_data++;
    *config_data++ = (crosssize / 8); // cross thread constant data read len

    uint32_t *batch = kconf.ctx->sys_buffer->m_batchbuff;
    *batch++ = MEDIA_STATE_FLUSH(0x0);
    *batch++ = 0x00000000; // ID offset

    // load ID
    *batch++ = MEDIA_INTERFACE_DESCRIPTOR_LOAD(0x2);
    *batch++ = 0x00000000; // MBZ
    *batch++ = 0x00000020; // ID lenght
    *batch++ = 0x00000000; // ID start address

    // start GPGPU_WALKER dispatch
    *batch++ = GPGPU_WALKER(0xD);
    *batch++ = 0x00000000; // ID offset
    uint32_t indirectsize = (crosssize + dispatchcount * (3 /*xyz*/ * kconf.simd / 2 /*uint16*/)) * 4 /*in byte*/;
    uint32_t indirectrest = indirectsize % 64;
    if (indirectrest > 0) // align to 32 bytes
        indirectsize = indirectsize - indirectrest + 64;
    *batch++ = indirectsize;                                                                                               // indirect data length
    *batch++ = 0x00000000;                                                                                                 // indirect data offset
    *batch++ = ((kconf.simd / 16) << 30) /*| (dispatchcount - 1) << 16 | (dispatchcount - 1) << 8*/ | (dispatchcount - 1); // SIMD32, max X/Y/Z // TODO: set depth and high counter?
    *batch++ = 0x00000000;                                                                                                 // start X
    *batch++ = 0x00000000;                                                                                                 // MBZ
    *batch++ = workgroupcount[0];                                                                                          // X dim
    *batch++ = 0x00000000;                                                                                                 // start Y
    *batch++ = 0x00000000;                                                                                                 // MBZ
    *batch++ = workgroupcount[1];                                                                                          // Y dim
    *batch++ = 0x00000000;                                                                                                 // start Z
    *batch++ = workgroupcount[2];                                                                                          // Z dim
    uint32_t mask = 0xFFFFFFFF;
    if (rest > 0)
    {
        mask = 0;
        for (uint32_t i = 0; i < rest; i++)
        {
            mask |= (1 << i);
        }
    }
    *batch++ = mask;       // right exec mask
    *batch++ = 0xFFFFFFFF; // bottom exec mask

    // flush
    *batch++ = MEDIA_STATE_FLUSH(0x0);
    *batch++ = 0x00000000; // ID offset
    *batch++ = PIPE_CONTROL(0x4);
    *batch++ = 0x00100000; // CS Stall
    *batch++ = 0x00000000; // address for post sync
    *batch++ = 0x00000000; // higher address bits
    *batch++ = 0x00000000; // immediate data
    *batch++ = 0x00000000; // immediate data

    // flush + wait operation
    *batch++ = PIPE_CONTROL(0x4);
    *batch++ = 0x00100120; // DC, Interrupt Notify
    *batch++ = 0x00000000; // address for post sync
    *batch++ = 0x00000000; // higher address bits
    *batch++ = 0x00000000; // immediate data
    *batch++ = 0x00000000; // immediate data

    // end
    *batch = MI_BATCH_BUFFER_END;

    // reset runner in kconfig
    kconf.ctx->sys_buffer->m_surf_runner = kconf.ctx->sys_buffer->m_surf;

    return true;
}

void GPGPU_Driver::run(kernel_config &kconf)
{
    // ggtt default
    uint32_t batch_conf = 0b000000001;

    // set ppgtt mode if needed
    if (kconf.ctx != &m_ctx)
    {
        submitPPGTT((PPGTT32 *)kconf.ctx->gtt);
        batch_conf |= (0x1UL << 8); // use ppgtt
    }

    // prepare kernel
    prepareRun(kconf);

    // flush between runs
    flush();

    RingBuffer &rb = RingBuffer::getInstance();
    rb.enqueue(MI_BATCH_BUFFER_START(batch_conf));
    rb.enqueue(kconf.ctx->sys_buffer->m_ga_batchbuff); // buffer start address
    rb.enqueue(0x00000000);                            // upper address bits for memory address
    rb.enqueue(MI_NOOP);                               // padding

    // submit buffer (do not wait for finish)
    rb.submit();
}

bool GPGPU_Driver::createBuffers(kernel_config &kconf)
{
    if (!prepareIOBuffers(kconf))
    {
        // kernel buffers are too big for gtt
        return false;
    }

    uint32_t *&m_surf_data = kconf.ctx->sys_buffer->m_surf_runner;

    for (uint8_t i = 0; i < kconf.buffCount; i++)
    {
        // skip constant memory
        if (kconf.buffConfigs[i].non_pointer_type)
            continue;

        // write RENDER_SURFACE_STATE to surf
        *m_surf_data++ = 0x87FD4000; // SURFTYPE_BUFFER, RAW, VALIGN 4, HALIGN 4, linear, write-only
        *m_surf_data++ = 0x00000000; // MOCS
        uint32_t r = kconf.buffConfigs[i].buffer_size - 1;
        *m_surf_data++ = ((r & 0x1FFF80) << 9) | (r & 0x7F) | 0x3; // Height=len[20:7], Width=len[6:0] with last 2 bit set and mul 4, but -1
        *m_surf_data++ = (r & 0xFFE00000);                         // Depth=len[32:21]
        *m_surf_data++ = 0x00000000;                               // min element, no multisampling
        *m_surf_data++ = 0x00004000;                               // no offsets, CPU coherent
        *m_surf_data++ = 0x00000000;                               // UV plane settings
        *m_surf_data++ = 0x00000000;                               // memory compression, shader channels
        *m_surf_data++ = kconf.buffConfigs[i].ga;                  // address of buffer
        *m_surf_data++ = 0x00000000;                               // higher bits of address
        *m_surf_data++ = 0x00000000;                               // V plane settings
        *m_surf_data++ = 0x00000000;                               // V plane settings
        *m_surf_data++ = 0x00000000;                               // red clear color
        *m_surf_data++ = 0x00000000;                               // green clear color
        *m_surf_data++ = 0x00000000;                               // blue clear color
        *m_surf_data++ = 0x00000000;                               // alpha clear color
    }
    return true;
}

void GPGPU_Driver::createBindingTable(kernel_config &kconf)
{
    uint32_t *&m_surf_data = kconf.ctx->sys_buffer->m_surf_runner;

    // write binding entries
    int binding_index = 0;
    for (int i = 0; i < kconf.buffCount; i++)
    {
        // constant memory does not have binding table entries
        if (kconf.buffConfigs[i].non_pointer_type)
            continue;

        *m_surf_data++ = 0x40 * binding_index++; // sizeof(RENDER_SURFACE_STATE) = 0x40
    }
}

bool GPGPU_Driver::printErrorState()
{
    printk("Printing Error State #### START\n");

    bool ret = false;
    uint32_t *error = 0;

    // TLB page fault
    error = (uint32_t *)(m_mmadr + MAIN_ARBITER_ERROR);
    if (*error & 0x1)
    {
        ret = true;

        uint32_t data0 = *(uint32_t *)(m_mmadr + FAULT_TLB_RD_DATA0);
        uint32_t data1 = *(uint32_t *)(m_mmadr + FAULT_TLB_RD_DATA1);
        uint64_t error_ga = ((uint64_t)(data1 & 0b1111) << 44) | data0;
        bool ggtt = ((data1 >> 4) & 0x1);

        printk("TLB Page Fault Error\n");
        printk("Fault cycle Virtual address [47:12] %lu\n", error_ga);
        printk("1-GGTT Cycle, 0-PPGTT Cycle : %u\n", ggtt);

        // more information about fault
        uint32_t fault_reg = *(uint32_t *)(m_mmadr + FAULT_REG);

        if (fault_reg & 0x1) // valid
        {
            // Fault type (GFX_FT)
            uint32_t info = (fault_reg >> 1) & 0b11;
            if (info == 0x0)
                printk("Invalid PTE Fault\n");
            if (info == 0x1)
                printk("Invalid PDE Fault\n");
            if (info == 0x2)
                printk("Invalid PDPE Fault\n");
            if (info == 0x3)
                printk("Invalid PML4E Fault\n");

            // SRCID of Fault
            info = (fault_reg >> 3) & 0xff;
            printk("SRCID of Fault %u\n", info);

            // Engine ID
            info = (fault_reg >> 12) & 0b111;
            if (info == 0x0)
                printk("Engine ID GFX\n");
            if (info == 0x1)
                printk("Engine ID MFX0\n");
            if (info == 0x2)
                printk("Engine ID MFX1\n");
            if (info == 0x3)
                printk("Engine ID VEBX\n");
            if (info == 0x4)
                printk("Engine ID BLT\n");
        }
    }

    // Invalid Page Directory Entry Error
    if ((*error >> 2) & 0x1)
    {
        printk("Invalid Page Directory Entry Error: PD entry's valid bit is 0\n");
        ret = true;
    }

    // Unloaded PD Error
    if ((*error >> 8) & 0x1)
    {
        printk("Unloaded PD Error\n");
        ret = true;
    }

    // rstrm_fault_nowb_atomic_err
    if ((*error >> 10) & 0x1)
    {
        printk("rstrm_fault_nowb_atomic_err\n");
        ret = true;
    }

    // ctx_fault_pasid_dis_err
    if ((*error >> 11) & 0x1)
    {
        printk("ctx_fault_pasid_dis_err\n");
        ret = true;
    }

    // ctx_fault_pasid_ovflw_err
    if ((*error >> 12) & 0x1)
    {
        printk("ctx_fault_pasid_ovflw_err\n");
        ret = true;
    }

    // ctx_fault_pasid_not_prsnt_err
    if ((*error >> 13) & 0x1)
    {
        printk("ctx_fault_pasid_not_prsnt_err\n");
        ret = true;
    }

    // ctx_fault_root_not_prsmt_err
    if ((*error >> 14) & 0x1)
    {
        printk("ctx_fault_root_not_prsmt_err\n");
        ret = true;
    }

    // ctx_fault_ctxt_not_prsmt_err
    if ((*error >> 15) & 0x1)
    {
        printk("ctx_fault_ctxt_not_prsmt_err\n");
        ret = true;
    }

    // ERROR_2 start
    error = (uint32_t *)(m_mmadr + MAIN_ARBITER_ERROR_2);
    if (*error & 0b111111)
    {
        printk("tlbpend_reg_faultcnt %u\n", (*error & 0b111111));
        ret = true;
    }
    // ERROR_2 end

    // ERROR_3 start
    error = (uint32_t *)(m_mmadr + MAIN_ARBITER_ERROR_3);

    // reg_wrid_internal_error
    if (*error & 0x1)
    {
        printk("reg_wrid_internal_error\n");
        ret = true;
    }

    // tlbpend_internal_error
    if ((*error >> 1) & 0x1)
    {
        printk("tlbpend_internal_error\n");
        ret = true;
    }

    // gttc_internal_error
    if ((*error >> 3) & 0x1)
    {
        printk("gttc_internal_error\n");
        ret = true;
    }

    // invalid_fixedmtrr_memtype_value_wr.
    if ((*error >> 4) & 0x1)
    {
        printk("invalid_fixedmtrr_memtype_value_wr\n");
        ret = true;
    }

    // invalid_fixedmtrr_memtype_value_rd.
    if ((*error >> 5) & 0x1)
    {
        printk("invalid_fixedmtrr_memtype_value_rd\n");
        ret = true;
    }

    // invalid_varmtrr_memtype_value_wr.
    if ((*error >> 6) & 0x1)
    {
        printk("invalid_varmtrr_memtype_value_wr\n");
        ret = true;
    }

    // invalid_varmtrr_memtype_value_rd.
    if ((*error >> 7) & 0x1)
    {
        printk("invalid_varmtrr_memtype_value_rd\n");
        ret = true;
    }

    // invalid_default_memtype_value_wr.
    if ((*error >> 8) & 0x1)
    {
        printk("invalid_default_memtype_value_wr\n");
        ret = true;
    }

    // invalid_default_memtype_value_rd.
    if ((*error >> 9) & 0x1)
    {
        printk("invalid_default_memtype_value_rd\n");
        ret = true;
    }

    // invalid_varmtrr_overlap_memtype_wr.
    if ((*error >> 10) & 0x1)
    {
        printk("invalid_varmtrr_overlap_memtype_wr\n");
        ret = true;
    }

    // invalid_varmtrr_overlap_memtype_rd.
    if ((*error >> 11) & 0x1)
    {
        printk("invalid_varmtrr_overlap_memtype_rd\n");
        ret = true;
    }
    // ERROR_3 end

    // Interrupt status register
    error = (uint32_t *)(m_mmadr + ISR);
    uint32_t isr = *error;
    if (isr)
    {
        printk("Interrupt Status Register %u\n", isr);
    }

    // Error Reporting Register
    error = (uint32_t *)(m_mmadr + ERR);

    // Buffer full Error Slice 0
    if ((*error >> 1) & 0x1)
    {
        printk("Buffer full Error Slice 0\n");
        ret = true;
    }

    // Write Expire Error Slice 0
    if ((*error >> 2) & 0x1)
    {
        printk("Write Expire Error Slice 0\n");
        ret = true;
    }

    // Second Buffer ready slice 0
    if ((*error >> 3) & 0x1)
    {
        printk("Second Buffer ready slice 0\n");
        ret = true;
    }

    // First Content Buffer Ready 0
    if ((*error >> 4) & 0x1)
    {
        printk("First Content Buffer Ready 0\n");
        ret = true;
    }

    // Error Status Register
    error = (uint32_t *)(m_mmadr + ESR);

    // Instruction Error
    if ((*error) & 0x1)
    {
        printk("Instruction Error\n");
        ret = true;
    }

    // Command Privilege Violation Error
    if ((*error >> 2) & 0x1)
    {
        printk("Command Privilege Violation Error\n");
        ret = true;
    }

    // Page Table Error
    if ((*error >> 4) & 0x1)
    {
        printk("Page Table Error\n");
        ret = true;
    }

    printk("Printing Error State #### END\n");

    return ret;
}

void GPGPU_Driver::submitPPGTT(PPGTT32 *gtt)
{
    // GFX_MODE PPGTT enable
    uint32_t *gfx_mode = (uint32_t *)(m_mmadr + GFX_MODE);

    *gfx_mode = 0xFFFF0201; // ppgtt enable, privilage check disable TODO change this

    RingBuffer &rb = RingBuffer::getInstance();

    // set four pdp register --> setting pdp0 last because this triggers the ppgtt switch
    for (uint8_t pdp_idx = 4; pdp_idx > 0; pdp_idx--)
    {
        uint64_t pd_table = (uint64_t)((gtt->getPDP()[pdp_idx - 1]));

        rb.enqueue(MI_NOOP);
        rb.enqueue(MI_LOAD_REGISTER_IMM(0x1));
        rb.enqueue(PDPX_RCSUNIT(pdp_idx - 1) + 4);
        rb.enqueue((uint32_t)SHIFT_RIGHT(pd_table, 32)); // high bits

        rb.enqueue(MI_NOOP);
        rb.enqueue(MI_LOAD_REGISTER_IMM(0x1));
        rb.enqueue(PDPX_RCSUNIT(pdp_idx - 1));
        rb.enqueue(pd_table & 0xFFFFFFFF); // lower bits
    }
}

void GPGPU_Driver::allocate_system_buffer(struct system_buffer *_buffer)
{
#ifdef LARGE_PAGES
    const uint32_t size = 0x10000;
#else
    const uint32_t size = 0x1000;
#endif // LARGE_PAGES

    // allocate buffers
    _buffer->m_base = (uint32_t *)gpgpu_aligned_alloc(size, 0x1000);
    _buffer->m_batchbuff = (uint32_t *)gpgpu_aligned_alloc(size, 0x2000);
    _buffer->m_surf = (uint32_t *)gpgpu_aligned_alloc(size, 0x1000);
    _buffer->m_dynamic = (uint32_t *)gpgpu_aligned_alloc(size, 0x1000);
    _buffer->m_indirect = (uint32_t *)gpgpu_aligned_alloc(size, 0x1000);
    _buffer->m_instr = (uint32_t *)gpgpu_aligned_alloc(size, MAX_KERNEL_SIZE);

    // set runner
    _buffer->m_surf_runner = _buffer->m_surf;
}

void GPGPU_Driver::map_system_buffer(struct system_buffer *_buffer, GTT *gtt)
{
#ifdef LARGE_PAGES
    const uint32_t size = 0x10000;
#else
    const uint32_t size = 0x1000;
#endif // LARGE_PAGES

    // map buffers to gpu
    _buffer->m_ga_base = gtt->mapPermanent((uint64_t)_buffer->m_base, size);            // General State Memory (not used)
    _buffer->m_ga_batchbuff = gtt->mapPermanent((uint64_t)_buffer->m_batchbuff, size);  // Batchbuffer Memory
    _buffer->m_ga_surf = gtt->mapPermanent((uint64_t)_buffer->m_surf, size);            // Surface Memory
    _buffer->m_ga_dynamic = gtt->mapPermanent((uint64_t)_buffer->m_dynamic, size);      // Dynamic State Memory
    _buffer->m_ga_indirect = gtt->mapPermanent((uint64_t)_buffer->m_indirect, size);    // Indirect Object Memory
    _buffer->m_ga_instr = gtt->mapPermanent((uint64_t)_buffer->m_instr, MAX_KERNEL_SIZE); // Instruction Memory (OpenCl program)
}

void GPGPU_Driver::free_system_buffer(struct system_buffer *_buffer)
{
    // free all buffers
    ::free(_buffer->m_base);
    ::free(_buffer->m_batchbuff);
    ::free(_buffer->m_surf);
    ::free(_buffer->m_dynamic);
    ::free(_buffer->m_indirect);
    ::free(_buffer->m_instr);
}

struct context *GPGPU_Driver::createContext()
{
    context *ctx = (context *)gpgpu_aligned_alloc(0x0, sizeof(context));
    ctx->gtt = new (gpgpu_aligned_alloc(0x0, sizeof(PPGTT32))) PPGTT32();
    ctx->gtt->init(0);
    ctx->sys_buffer = (system_buffer *)gpgpu_aligned_alloc(0x0, sizeof(system_buffer));
    allocate_system_buffer(ctx->sys_buffer);
    map_system_buffer(ctx->sys_buffer, ctx->gtt);
    return ctx;
}

void GPGPU_Driver::freeContext(context *ctx)
{
    ((PPGTT32 *)ctx->gtt)->free();
    free_system_buffer(ctx->sys_buffer);
    ::free(ctx->sys_buffer);
    ::free(ctx->gtt);
    ::free(ctx);
}
