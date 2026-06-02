#include "driver/gpgpu_driver.h"

#ifdef __cplusplus
extern "C"
{
#else
#include <stdbool.h>
#endif // __cplusplus

    /**
     * @brief Init the driver
     *
     * @param interrupt_vector the interrupt number for MSI
     */
    void gpgpu_init(uint8_t interrupt_vector);

    /**
     * @brief checks if a GPU Task is currently running
     *
     * @return true a GPU Task is running
     * @return false  a GPU Task is not running
     */
    bool gpgpu_isGPUTaskRunning(void);

    /**
     * @brief checks if the GPGPU Task queue is empty
     *
     * @return true if there are no Task enqueued
     * @return false if there are still Task in the queue
     */
    bool gpgpu_isTaskQueueEmpty(void);

    /**
     * @brief handle a GPU Interrupt
     *
     */
    void gpgpu_handleInterrupt(void);

    /**
     * @brief run the next Task from task_queue
     *
     */
    void gpgpu_runNext(void);

    /**
     * @brief Get the Interrupt Number
     *
     * @return uint8_t the interrupt number
     */
    uint8_t gpgpu_getInterruptNumber(void);

    /**
     * @brief Set the minimum frequency to save power and lower temps
     *
     */
    void gpgpu_setMinFreq(void);

    /**
     * @brief Set the maximum frequency to gain maximum performance
     *
     */
    void gpgpu_setMaxFreq(void);

    /**
     * @brief runs a kernel
     *
     * @param kernel the kernel configuration
     */
    void gpgpu_run(struct kernel_config *kconf);

    /**
     * @brief enqueues a kernel run
     *
     * @param kernel the kernel configuration
     */
    void gpgpu_enqueueRun(struct kernel_config *kconf);

    /**
     * @brief frees driver memory
     *
     */
    void gpgpu_free(void);

    /**
     * @brief allocate a GPU context
     *
     * @return context* the new context
     */
    struct context *gpgpu_createContext();

    /**
     * @brief free a GPU context
     *
     * @param ctx the context to be freed
     */
    void gpgpu_freeContext(struct context *ctx);

#ifdef __cplusplus
}
#endif // __cplusplus