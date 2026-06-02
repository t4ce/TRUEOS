#include "stubs.h"

#ifdef __cplusplus
extern "C"
{
    void __cxa_pure_virtual()
    {
        for (;;)
            ;
    }
#endif // __cplusplus

    // printing (optional)
    int printk(const char *__restrict, ...)
    {
        return 0;
    }

    // interrupts: TODO: call these methods when a gpu interrupt occurs
    // GPGPU_Driver::getInstance().handleInterrupt(); // optional: in prologue
    // GPGPU_Driver::getInstance().runNext(); // optional: in epilogue

    // allocator
    void *gpgpu_aligned_alloc(uint32_t alignment, uint64_t size)
    {
        (void)alignment;
        (void)size;
        return nullptr;
    }
    void free(void *addr)
    {
        (void)addr;
    }

    // pci
    uint32_t calculatePCIConfigHeaderAddress(uint8_t bus, uint8_t device, uint8_t function)
    {
        uint32_t address_reg = 0;
        address_reg |= (bus & 0xFF) << 16;
        address_reg |= (device & 0x1F) << 11;
        address_reg |= (function & 0x7) << 8;
        return address_reg;
    }
    uint32_t readPCIConfigSpace(uint32_t addr)
    {
        (void)addr;
        return 0;
    }
    void writePCIConfigSpace(uint32_t address, uint32_t value)
    {
        (void)address;
        (void)value;
    }

    // address model
    void *virt_to_phys(void *addr)
    {
        return addr;
    }

#ifdef __cplusplus
}
#endif // __cplusplus