#ifndef GPGPU_DRIVER_H
#define GPGPU_DRIVER_H

#include <stdint.h>

#include "gtt.h"
#ifdef __cplusplus
#include "parseGEN.h"
#include "ggtt32.h"
#include "ppgtt32.h"
#include "lib/wf_queue.h"
#else
#include <stdbool.h>
#endif // __cplusplus

struct CrossThreadData_info;
struct kernel_config;

#define MAX_KERNEL_SIZE 0x4000 // max 16KB of binary GPU-Code

#ifdef __cplusplus
extern "C"
{
#endif // __cplusplus

#define CONTEXT_ENTRIES_NOT_EXTENDED 0x1690
#define CONTEXT_ENTRIES_EXTENDED 0x5150

    /**
     * @brief used by the driver
     *
     */
    struct system_buffer
    {
#ifdef __cplusplus
        system_buffer() : m_ringbuff(nullptr), m_base(nullptr), m_batchbuff(nullptr), m_surf(nullptr), m_surf_runner(nullptr),
                          m_dynamic(nullptr), m_indirect(nullptr), m_instr(nullptr), m_ga_ringbuff(0), m_ga_base(0),
                          m_ga_batchbuff(0), m_ga_surf(0), m_ga_dynamic(0), m_ga_indirect(0), m_ga_instr(0)
        {
        }
#endif // __cplusplus

        // phys buffer
        uint32_t *m_ringbuff;
        uint32_t *m_base;
        uint32_t *m_batchbuff;
        uint32_t *m_surf;
        uint32_t *m_surf_runner; // just for setting surfrace state buffer
        uint32_t *m_dynamic;
        uint32_t *m_indirect;
        uint32_t *m_instr;

        // va buffer
        uint32_t m_ga_ringbuff;
        uint32_t m_ga_base;
        uint32_t m_ga_batchbuff;
        uint32_t m_ga_surf;
        uint32_t m_ga_dynamic;
        uint32_t m_ga_indirect;
        uint32_t m_ga_instr;
    };

    struct context
    {
        /// GTT (GGTT32 or PPGTT32)
#ifndef __cplusplus
        void *gtt;
#else
    GTT *gtt;
#endif // __cplusplus

        /// buffer
        struct system_buffer *sys_buffer;
    };

    struct buffer_config
    {
#ifdef __cplusplus
        buffer_config() : buffer(nullptr), buffer_size(0), non_pointer_type(false), ga(0), pos(0)
        {
        }
#endif // __cplusplus

        /// the buffer address (4K aligned)
        void *buffer;

        /// the buffersize in byte
        uint32_t buffer_size;

        /// the type of parameter
        bool non_pointer_type;

#ifdef __cplusplus
    private:
#endif // __cplusplus
        /// the graphics address (set by driver)
        uint32_t ga;

        /// indirect buffer position (used by driver)
        uint32_t pos;

#ifdef __cplusplus
        // provide GPGPU_Driver access to ga
        friend class GPGPU_Driver;

        // provide parseGEN access to pos
        friend void parseGEN(CrossThreadData_info &info, kernel_config &kconf, uint8_t *instr);
#endif // __cplusplus
    };

#ifndef __cplusplus
#define INIT_KERNEL_CONFIG(kconf)                            \
    (kconf) = (struct kernel_config){0};                     \
    (kconf).workgroupsize[1] = (kconf).workgroupsize[2] = 1; \
    (kconf).simd = 32;
#define INIT_BUFFER_CONFIG(bconf) (bconf) = (struct buffer_config){0};
#endif // __cplusplus

    struct kernel_config
#ifdef __cplusplus
        : public Chain
#endif // __cplusplus
    {
#ifndef __cplusplus
        uint8_t ChainDummy[8]; // sizeof(Chain) = 8
#endif                         // __cplusplus

#ifdef __cplusplus
        kernel_config() : kernelName(nullptr), range{0, 0, 0}, workgroupsize{0, 1, 1}, binary(nullptr), buffCount(0), buffConfigs(nullptr),
                          finish_callback(nullptr), ctx(nullptr), finished(false), simd(32), useBarrier(false)
        {
        }
#endif // __cplusplus

        /// the name of kernel to be executed (nullptr => first kernel)
        char *kernelName;

        /// the number of workitems for each dimension
        uint32_t range[3];

        /// the workgroupsize for each dimension (0 for x means auto)
        uint32_t workgroupsize[3];

        /// the GEN Binary
        uint8_t *binary;

        /// the number of buffers
        uint8_t buffCount;

        /// pointer to buffer configs (freed by driver)
#ifndef __cplusplus
        struct
#endif // __cplusplus
            buffer_config *buffConfigs;

        /// callback executed when kernel has finished
        void (*finish_callback)(void);

        /// GPU context
        struct context *ctx;

        /// finished flag for polling
        bool finished;

#ifdef __cplusplus
    private:
#endif // __cplusplus

        /// the simd mode
        uint8_t simd;

        /// using barrier for OpenCL Sync
        bool useBarrier;

#ifdef __cplusplus
        // provide GPGPU_Driver access to buffers, simd and useBarrier
        friend class GPGPU_Driver;

        // provide parseGEN access to simd and useBarrier
        friend void parseGEN(CrossThreadData_info &info, kernel_config &kconf, uint8_t *instr);
#endif // __cplusplus
    };

#ifdef __cplusplus
}
#endif // __cplusplus

#ifdef __cplusplus
// extend the WFQueue implementation with first()
class WFQueueExt : public WFQueue
{
public:
    Chain *first() const
    {
        if (tail == &stub)
            return tail->next;
        return tail;
    }
};

class GPGPU_Driver
{
public:
    static GPGPU_Driver &getInstance()
    {
        static GPGPU_Driver instance;
        return instance;
    }

private:
    GPGPU_Driver() : sys_buffer(), ggtt(), m_ctx(), m_mmadr(0), m_pci_config_header(0), m_int_vec(0x31), m_min_freq(0),
                     m_max_freq(0), m_task_queue(), m_current_kernel(nullptr){};
    GPGPU_Driver(const GPGPU_Driver &copy) = delete;
    GPGPU_Driver &operator=(const GPGPU_Driver &src) = delete;

    /// system buffer for global GTT
    struct system_buffer sys_buffer;

    /// central global GTT
    GGTT32 ggtt;

    /// driver context for global GTT and driver sys buffer
    context m_ctx;

    /// mapped memory address
    uint64_t m_mmadr;

    /// pci header address
    uint32_t m_pci_config_header;

    /// Interrupt Number (default: 0x31)
    uint8_t m_int_vec;

    /// minimum frequency multiplier
    uint8_t m_min_freq;

    /// maximum non-oc frequency multiplier
    uint8_t m_max_freq;

    /// GPGPU Task Queue
    WFQueueExt m_task_queue;

    /// currently running Task
    kernel_config *m_current_kernel;

    /**
     * @brief Sets up the MSIs
     *
     * @param interrupt_vector The Interrupt-Number (0x10 - 0xFE)
     */
    void setupMSI(uint8_t interrupt_vector);

    /**
     * @brief Create a Buffer object
     *
     * @param kconf the kernel config object
     * @return true if all buffer created
     * @return false if out of Memory
     */
    bool createBuffers(kernel_config &kconf);

    /**
     * @brief maps the In and Out Buffers
     *
     * @param kconf the kernel config object
     * @return true if all buffer created
     * @return false if out of Memory
     */
    bool prepareIOBuffers(kernel_config &kconf);

    /**
     * @brief Create the Binding Table. All Buffer objects have to be created at this time!
     *
     * @param kconf the kernel config object
     */
    void createBindingTable(kernel_config &kconf);

    /**
     * @brief prepare a kernel run (buffer allocation, worksize, ...)
     *
     * @param kconf the kernel to be prepared
     * @return true if prepare was successful
     * @return false if not
     */
    bool prepareRun(kernel_config &kconf);

    /**
     * @brief flushes the GPU. This is necessary between GPU Tasks!
     *
     */
    void flush();

    /**
     * @brief print the Error state of the GPU
     *
     * @return true if error occured
     * @return false if not
     */
    bool printErrorState();

    /**
     * @brief allocate system buffers
     *
     * @param _buffer system buffers
     */
    void allocate_system_buffer(struct system_buffer *_buffer);

    /**
     * @brief map system buffers with gtt
     *
     * @param _buffer system buffers
     * @param gtt gtt instance
     */
    void map_system_buffer(struct system_buffer *_buffer, GTT *gtt);

    /**
     * @brief free system buffers
     *
     * @param _buffer system buffers
     */
    void free_system_buffer(struct system_buffer *_buffer);

    /**
     * @brief allocate and init ppgtt for kernel and prepare task execution
     *
     * @param kconf kernel config
     */
    void initPPGTT(kernel_config &kconf);

    /**
     * @brief setup per process GTT
     *
     * @param gtt the per process GTT
     */
    void submitPPGTT(PPGTT32 *gtt);

public:
    /**
     * @brief Init the driver
     *
     * @param interrupt_vector the interrupt number for MSI
     */
    void init(uint8_t interrupt_vector = 0x31);

    /**
     * @brief checks if a GPU Task is currently running
     *
     * @return true a GPU Task is running
     * @return false  a GPU Task is not running
     */
    bool isGPUTaskRunning() const { return m_current_kernel != nullptr; }

    /**
     * @brief checks if the GPGPU Task queue is empty
     *
     * @return true if there are no Task enqueued
     * @return false if there are still Task in the queue
     */
    bool isTaskQueueEmpty() const { return m_task_queue.empty(); }

    /**
     * @brief handle a GPU Interrupt
     *
     */
    void handleInterrupt();

    /**
     * @brief run the next Task from task_queue
     *
     */
    void runNext();

    /**
     * @brief Get the Interrupt Number
     *
     * @return uint8_t the interrupt number
     */
    uint8_t getInterruptNumber() const { return m_int_vec; }

    /**
     * @brief Set the minimum frequency to save power and lower temps
     *
     */
    void setMinFreq();

    /**
     * @brief Set the maximum frequency to gain maximum performance
     *
     */
    void setMaxFreq();

    /**
     * @brief runs a kernel
     *
     * @param kernel the kernel configuration
     */
    void run(kernel_config &kconf);

    /**
     * @brief enqueues a kernel run
     *
     * @param kernel the kernel configuration
     */
    void enqueueRun(kernel_config &kconf);

    /**
     * @brief frees driver memory
     *
     */
    void free();

    /**
     * @brief allocate a GPU context
     *
     * @return context* the new context
     */
    struct context *createContext();

    /**
     * @brief free a GPU context
     *
     * @param ctx the context to be freed
     */
    void freeContext(context *ctx);
};
#endif // __cplusplus

#endif /* !GPGPU_DRIVER_H */
