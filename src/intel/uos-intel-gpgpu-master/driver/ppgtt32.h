#ifndef PPGTT32_H
#define PPGTT32_H

#include <stdint.h>
#include "gtt.h"

#define PPGTT_PAGE_DIRECTORY_POINTER_COUNT 4
#define PPGTT_PAGE_DIRECTORY_SIZE 512
#define PPGTT_PAGE_TABLE_SIZE 512

#define PPGTT_PD_INDEX 21
#define PPGTT_PDP_INDEX 30

class PPGTT32 : public GTT
{
public:
    PPGTT32() : dummy(nullptr), pdp{nullptr, nullptr, nullptr, nullptr}, pd_base(nullptr), pt_base(nullptr),
#ifdef LARGE_PAGES
        page_size(0x10000),
#else
        page_size(0x1000),
#endif // LARGE_PAGES
        pte_idx_start(GTT_GA_START), pte_idx(GTT_GA_START) {}

    /**
     * @brief not required for ppgtt
     *
     * @param base 0
     */
    void init(uint64_t *base = nullptr) override;

    /**
     * @brief Returns all four Page Directory Pointer for submitting this ppgtt
     *
     * @return uint64_t** array with four elements
     */
    inline uint64_t **getPDP()
    {
        return pdp;
    }

    /**
     * @brief starting mapping at start position
     *
     * @return uint64_t** array with four elements
     */
    void clear() override
    {
        // reset pd and pdp
        pte_idx = pte_idx_start;
    }

    /**
     * @brief reset complete PPGTT
     */
    void reset() override
    {
        // reset pdp and pd
        pte_idx_start = GTT_GA_START;
        clear();
    }

    /**
     * @brief maps the memory to a graphicaladdress
     *
     * @param virt the virtual address, must be 4K aligned, all address bits > 38 are ignored
     * @param size the size to be mapped in bytes
     * @return uint32_t the graphicsaddress
     */
    uint32_t map(uint64_t virt, uint32_t size) override;

    /**
     * @brief maps the memory to a graphicaladdress and removes this space from the allocation pool
     *
     * @param virt the virtual address, must be 4K aligned, all address bits > 38 are ignored
     * @param size the size to be mapped in bytes
     * @return uint32_t the graphicsaddress
     */
    uint32_t mapPermanent(uint64_t virt, uint32_t size) override;

    /**
     * @brief get the mappend phys address
     *
     * @param graphicsaddress the graphical address
     * @return uint64_t the physical address
     */
    uint64_t get(uint32_t graphicsaddress) const override;

    /**
     * @brief free memory allocated by the PPGTT32
     *
     */
    void free();

private:
    PPGTT32(const PPGTT32 &copy);
    PPGTT32 operator=(const PPGTT32 &);

    /**
     * @brief Maps a 4K graphics page to a physical page.
     *
     * @param graphicsaddress the graphical address
     * @param phys the 4K aligned physical address
     */
    void mapPage(uint32_t graphicsaddress, uint64_t phys) override;

    /**
     * @brief maps the memory to a graphicaladdress
     *
     * @param virt the virtual address, must be 4K aligned, all address bits > 38 are ignored
     * @param size the size to be mapped in bytes
     * @return uint32_t the graphicsaddress
     */
    uint32_t mapPTE(uint64_t virt, uint32_t size);

    // dummy page
    void *dummy;

    // PDP -- * --> PD Table -- * --> PT Table --- * --> phy. Address

    // page directory pointers
    uint64_t *pdp[4];

    // page directories
    uint64_t *pd_base;

    // page tables
    uint64_t *pt_base;

    uint32_t page_size;
    uint32_t pte_idx_start;

    // next free pte index
    uint32_t pte_idx;
};

#endif /* !PPGTT_H */
