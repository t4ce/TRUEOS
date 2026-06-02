#include "CConnector.h"

extern "C"
{
    void gpgpu_init(uint8_t interrupt_vector)
    {
        GPGPU_Driver::getInstance().init(interrupt_vector);
    }

    bool gpgpu_isGPUTaskRunning()
    {
        return GPGPU_Driver::getInstance().isGPUTaskRunning();
    }

    bool gpgpu_isTaskQueueEmpty()
    {
        return GPGPU_Driver::getInstance().isTaskQueueEmpty();
    }

    void gpgpu_handleInterrupt()
    {
        GPGPU_Driver::getInstance().handleInterrupt();
    }

    void gpgpu_runNext()
    {
        GPGPU_Driver::getInstance().runNext();
    }

    uint8_t gpgpu_getInterruptNumber()
    {
        return GPGPU_Driver::getInstance().getInterruptNumber();
    }

    void gpgpu_setMinFreq()
    {
        GPGPU_Driver::getInstance().setMinFreq();
    }

    void gpgpu_setMaxFreq()
    {
        GPGPU_Driver::getInstance().setMaxFreq();
    }

    void gpgpu_run(struct kernel_config *kconf)
    {
        GPGPU_Driver::getInstance().run(*kconf);
    }

    void gpgpu_enqueueRun(struct kernel_config *kconf)
    {
        GPGPU_Driver::getInstance().enqueueRun(*kconf);
    }

    void gpgpu_free()
    {
        GPGPU_Driver::getInstance().free();
    }

    struct context *gpgpu_createContext()
    {
        return GPGPU_Driver::getInstance().createContext();
    }

    void gpgpu_freeContext(struct context *ctx)
    {
        GPGPU_Driver::getInstance().freeContext(ctx);
    }
}