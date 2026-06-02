#ifndef GGTT32_H
#define GGTT32_H

#include <stdint.h>

#include "gtt.h"

class GGTT32 : public GTT
{
public:
    GGTT32() : m_base(nullptr), m_start(GTT_GA_START), m_next(GTT_GA_START){};

    /**
     * @brief this method has to be called before using the GTT!
     *
     * @param base the starting address of the GTT
     */
    void init(uint64_t *base) override;

    /**
     * @brief maps the memory to a graphicaladdress
     *
     * @param virt the virtual address, must be 4K aligned, all phys address bits > 38 are ignored
     * @param size the size to be mapped in bytes
     * @return uint32_t the graphicsaddress
     */
    uint32_t map(uint64_t virt, uint32_t size) override;

    /**
     * @brief maps the memory to a graphicaladdress and removes this space from the allocation pool
     *
     * @param virt the virtual address, must be 4K aligned, all phys address bits > 38 are ignored
     * @param size the size to be mapped in bytes
     * @return uint32_t the graphicsaddress
     */
    uint32_t mapPermanent(uint64_t virt, uint32_t size) override;

    /**
     * @brief reset the complete GTT-State
     *
     */
    void reset() override;

    /**
     * @brief clears the complete GTT
     *
     */
    void clear() override;

    /**
     * @brief get the mappend phys address
     *
     * @param graphicsaddress the graphical address
     * @return uint64_t the physical address
     */
    uint64_t get(uint32_t graphicsaddress) const override;

private:
    GGTT32(const GGTT32 &copy);
    GGTT32 operator=(const GGTT32 &);

    /**
     * @brief Maps a 4K graphics page to a physical page.
     *
     * @param graphicsaddress the graphical address
     * @param phys the 4K aligned physical address
     */
    void mapPage(uint32_t graphicsaddress, uint64_t phys) override;

    /// the base address of the GTT
    uint64_t *m_base;

    /// the start Page
    uint32_t m_start;

    /// the next Page after the last mapped graphics address
    uint32_t m_next;
};

#endif // GGTT32_H