#include "ggtt32.h"
#include "../stubs.h"

void GGTT32::init(uint64_t *base)
{
    this->m_base = base; // starting GGTT32 address (first address entry)
}

uint32_t GGTT32::mapPermanent(uint64_t virt, uint32_t size)
{
    // clear GGTT32 to start at start page
    clear();

    // map the memory
    uint32_t ga = map(virt, size);
    if (ga == MAP_FAILED)
    {
        return MAP_FAILED;
    }

    // set the new start page
    m_start = m_next;
    return ga;
}

uint32_t GGTT32::map(uint64_t virt, uint32_t size)
{
    uint64_t phys = (uint64_t)virt_to_phys((void *)virt);

    // check address bits < 38
    if (phys > 0x7FFFFFFFFF)
    {
        printk("Out of memory! phys: %lu\n", phys);
        return MAP_FAILED;
    }

    // get pagecount
    uint32_t pagecount = roundUp(size, 0x1000);

    // check space in GGTT32
    if (m_next + pagecount > GTT_GA_END)
    {
        printk("Too many pages! phys: %lu next: %u pagecount: %u\n", phys, m_next, pagecount);
        return MAP_FAILED;
    }

    // map Pages
    for (uint32_t i = 0; i < pagecount; i++)
    {
        mapPage(m_next + i, phys + i * 0x1000);
    }

    // va
    uint32_t va = m_next << 12;

    // move to next free page
    m_next += pagecount;

    // return starting graphics address
    return va;
}

void GGTT32::clear()
{
    m_next = m_start;
}

void GGTT32::reset()
{
    m_start = GTT_GA_START;
    clear();
}

void GGTT32::mapPage(uint32_t graphicsaddress, uint64_t phys)
{
    // set address and valid bit
    m_base[graphicsaddress] = phys | 0x1; // enable pte
}

uint64_t GGTT32::get(uint32_t graphicsaddress) const
{
    // get address bits [38:12] ([45:12] for server)
    return m_base[graphicsaddress >> 12] & 0x7FFFFFF000;
}
