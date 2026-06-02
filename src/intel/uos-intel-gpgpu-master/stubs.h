#ifndef STUBS_H
#define STUBS_H

#include <stdint.h>

#ifdef __cplusplus
extern "C"
{
#endif // __cplusplus

    // printing (optional)
    int printk(const char *__restrict, ...);

    // allocator
    void *gpgpu_aligned_alloc(uint32_t alignment, uint64_t size);
    void free(void *addr);

    // pci
    uint32_t calculatePCIConfigHeaderAddress(uint8_t bus, uint8_t device, uint8_t function);
    uint32_t readPCIConfigSpace(uint32_t addr);
    void writePCIConfigSpace(uint32_t address, uint32_t value);

    // address model
    void *virt_to_phys(void *addr);

#ifdef __cplusplus
}
#endif // __cplusplus

#endif // STUBS_H