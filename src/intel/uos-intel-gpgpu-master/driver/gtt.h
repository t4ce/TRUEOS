#ifndef GTT_H
#define GTT_H

#include <stdint.h>

#define GTT_SIZE 0x800000 // 8MB
#define GTT_GA_END (GTT_SIZE / sizeof(uint64_t))
#define LARGE_PAGES // 64K Pages

// WaSkipStolenMemoryFirstPage
#ifdef LARGE_PAGES
#define GTT_GA_START 0x10
#else
#define GTT_GA_START 0x1
#endif // LARGE_PAGES

#define MAP_FAILED 0xFFFFFFFF

#ifdef __cplusplus

/**
 * @brief calculates x / y, but always rounds up to next integer
 *
 * @param x x
 * @param y y
 * @return uint32_t the rounded value
 */
uint32_t roundUp(uint32_t x, uint32_t y);

class GTT
{
public:
    GTT() {}
    virtual ~GTT() = default;

    /**
     * @brief this method has to be called before using the GTT!
     *
     * @param base the starting address of the GTT
     */
    virtual void init(uint64_t *base) = 0;

    /**
     * @brief maps the physical memory to a graphicaladdress
     *
     * @param phys the physical address, must be 4K aligned, all address bits > 38 are ignored
     * @param size the size to be mapped in bytes
     * @return uint32_t the graphicsaddress
     */
    virtual uint32_t map(uint64_t phys, uint32_t size) = 0;

    /**
     * @brief maps the physical memory to a graphicaladdress and removes this space from the allocation pool
     *
     * @param phys the physical address, must be 4K aligned, all address bits > 38 are ignored
     * @param size the size to be mapped in bytes
     * @return uint32_t the graphicsaddress
     */
    virtual uint32_t mapPermanent(uint64_t phys, uint32_t size) = 0;

    /**
     * @brief reset the complete GTT-State
     *
     */
    virtual void reset() = 0;

    /**
     * @brief clears the complete GTT
     *
     */
    virtual void clear() = 0;

    /**
     * @brief get the mappend phys address
     *
     * @param graphicsaddress the graphical address
     * @return uint64_t the physical address
     */
    virtual uint64_t get(uint32_t graphicsaddress) const = 0;

private:
    /**
     * @brief Maps a 4K graphics page to a physical page.
     *
     * @param graphicsaddress the graphical address
     * @param phys the 4K aligned physical address
     */
    virtual void mapPage(uint32_t graphicsaddress, uint64_t phys) = 0;
};

#endif // __cplusplus

#endif /* !GTT_H */
