#include "ppgtt32.h"
#include "../stubs.h"

void PPGTT32::init(uint64_t *base)
{
    (void)base;

    const uint32_t entries = PPGTT_PAGE_DIRECTORY_POINTER_COUNT * PPGTT_PAGE_DIRECTORY_SIZE + // page directories
        PPGTT_PAGE_DIRECTORY_POINTER_COUNT * PPGTT_PAGE_DIRECTORY_SIZE * PPGTT_PAGE_TABLE_SIZE; // page tables
    const uint32_t size = entries * sizeof(uint64_t);
    uint64_t* tables = (uint64_t *)gpgpu_aligned_alloc(0x1000, size);

    for(uint32_t i = 0; i < entries; i++)
        tables[i] = 0;

    pd_base = tables;
    pt_base = tables + (PPGTT_PAGE_DIRECTORY_POINTER_COUNT * PPGTT_PAGE_DIRECTORY_SIZE);

    uint64_t *pd_iter = pd_base;
    uint64_t *pt_iter = pt_base;

    // connect tables
    for(uint8_t pd = 0; pd < PPGTT_PAGE_DIRECTORY_POINTER_COUNT; pd++)
    {
        // connect pd table to pdp register (here saving for pdp register)
        pdp[pd] = (uint64_t *)virt_to_phys(pd_iter);

        //  pt table connection to pde and enable default
        for(uint16_t pdentry = 0; pdentry < PPGTT_PAGE_DIRECTORY_SIZE; pdentry++)
        {
#ifdef LARGE_PAGES
            *pd_iter++ = (uint64_t)virt_to_phys(pt_iter) | (0x1 << 11) | 0x2 | 0x1; // enable pde with page table address
#else
            *pd_iter++ = (uint64_t)virt_to_phys(pt_iter) | 0x2 | 0x1; // enable pde with page table address
#endif // LARGE_PAGES
            pt_iter += PPGTT_PAGE_DIRECTORY_SIZE;
        }
    }

    // set all pt entries valid
    dummy = gpgpu_aligned_alloc(page_size, page_size);
    for(uint32_t entry = 0; entry < (PPGTT_PAGE_DIRECTORY_POINTER_COUNT * PPGTT_PAGE_DIRECTORY_SIZE * PPGTT_PAGE_TABLE_SIZE); entry++)
    {
        mapPage(entry, (uint64_t)virt_to_phys(dummy));
    }
}

void PPGTT32::free()
{
    ::free(pd_base);
    ::free(dummy);
}

uint32_t PPGTT32::mapPTE(uint64_t virt, uint32_t size)
{
    uint64_t phys = (uint64_t)virt_to_phys((void *)virt);

    // check address bits < 38
    if (phys > 0x7FFFFFFFFF)
    {
        printk("Out of memory! phys: %lu\n", phys);
        return MAP_FAILED;
    }

    // get pagecount
    uint32_t pagecount = roundUp(size, page_size);
#ifdef LARGE_PAGES
    const uint32_t stepwidth = 16;
#else
    const uint32_t stepwidth = 1;
#endif // LARGE_PAGES

    // check space in PPGTT32
    if(pte_idx + (pagecount * stepwidth) > GTT_GA_END)
    {
        printk("Too many pages! phys: %lu next: %u pagecount: %u\n", phys, pte_idx, pagecount * stepwidth);
        return MAP_FAILED;
    }

    // map Pages
    for (uint32_t i = 0; i < pagecount; i++)
    {
        mapPage(pte_idx + i * stepwidth, phys + i * page_size);
    }

    // move to next free page
    pte_idx += (pagecount * stepwidth);

    // return starting graphics address
    return pte_idx - (pagecount * stepwidth);
}

uint32_t PPGTT32::map(uint64_t virt, uint32_t size)
{
    // map pte and return pte address
    uint32_t va_pte = mapPTE(virt, size);

    if(va_pte == MAP_FAILED)
    {
        printk("mapPTE failed\n");
        return MAP_FAILED;
    }

    // move bits to according positions
    va_pte <<= 12;

    return va_pte;
}

uint32_t PPGTT32::mapPermanent(uint64_t addr, uint32_t size)
{
    // clear PPGTT32
    clear();

    // mapping GTT
    uint32_t va = map(addr, size);
    if(va == MAP_FAILED)
        return MAP_FAILED;

    // save permanent
    pte_idx_start = pte_idx;

    return va;
}

void PPGTT32::mapPage(uint32_t graphicsaddress, uint64_t phys)
{
    // set address and valid bit
    pt_base[graphicsaddress] = phys | 0x2 | 0x1; // write permission on, enable pte
}

uint64_t PPGTT32::get(uint32_t graphicsaddress) const
{
    // get address bits [38:12] ([45:12] for server)
    // no need to look up PD and PT, because its allocated as one big chunk
    uint32_t pdpe_idx = (graphicsaddress >> PPGTT_PDP_INDEX) & 0b11;
    uint32_t pde_idx = (graphicsaddress >> PPGTT_PD_INDEX) & 0x1ff;
    uint32_t pte = ((graphicsaddress >> 12) & 0x1ff) +
                   ((pde_idx * 512) + (pdpe_idx * 512 * 512));

    return pt_base[pte] & 0x7FFFFFF000;
}
