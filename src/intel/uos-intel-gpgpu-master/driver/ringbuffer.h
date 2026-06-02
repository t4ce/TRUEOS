#ifndef RINGBUFFER_H
#define RINGBUFFER_H

#include <stdint.h>

class RingBuffer
{
public:
    static RingBuffer &getInstance()
    {
        static RingBuffer instance;
        return instance;
    }

    /**
     * @brief this method has to be called before using the RingBuffer!
     *
     * @param mmadr the mmio base address
     * @param physaddress the physical start address of the buffer
     * @param graphicsaddress the GTT address of the Buffer
     * @param size the size of the buffer in MiB
     */
    void init(uint64_t mmadr, uint32_t *physaddress, uint32_t graphicsaddress, uint32_t size);

    /**
     * @brief start the execution
     *
     */
    void start();

    /**
     * @brief stop the execution
     *
     */
    void stop();

    /**
     * @brief submits the commands to the card
     *
     */
    void submit();

    /**
     * @brief checks if the execution is currently idleing
     *
     * @return true execution is idleing
     * @return false execution is not idleing
     */
    bool isIdle() const { return *m_mode & 0x200; }

    /**
     * @brief checks if the execution is stopped
     *
     * @return true execution is stopped
     * @return false execution is not stopped
     */
    bool isStopped() const { return *m_mode & 0x100; }

    /**
     * Enqueues the dword in the Buffer. The Ringbuffer has to be stopped for this!
     * Make sure to always enqueue even number of dwords to algin the buffer to qwords!
     *
     * @param dword the dword to be enqueued
     */
    void enqueue(uint32_t dword);

    /**
     * @brief submits all commands and blocks until all enqueued dwords executed. (Busy waiting)
     *
     */
    bool wait();

    /**
     * @brief Get the Current Tail offset
     *
     * @return uint32_t offset
     */
    uint32_t getCurrentTailOffset() const;

    /**
     * @brief Get the Current Head offset
     *
     * @return uint32_t offset
     */
    uint32_t getCurrentHeadOffset() const;

private:
    RingBuffer() : m_mmadr(0), m_size(0), m_start(nullptr), m_cur(nullptr), m_pad(false), m_cnt(0),
                   m_head(nullptr), m_tail(nullptr), m_ctl(nullptr), m_mode(nullptr){};
    RingBuffer(const RingBuffer &copy) = delete;
    RingBuffer &operator=(const RingBuffer &src) = delete;

    /**
     * @brief enables the RingBuffer
     *
     */
    void enable();

    /// mmio base address
    uint64_t m_mmadr;

    /// RingBuffer size in MiB
    uint32_t m_size;

    /// physical start address
    uint32_t *m_start;

    /// current write address
    volatile uint32_t *m_cur;

    /// padding flag
    bool m_pad;

    /// qword counter
    uint32_t m_cnt;

    /// head register of the ring
    volatile uint32_t *m_head;

    /// tail register of the ring
    volatile uint32_t *m_tail;

    /// control register of the ring
    volatile uint32_t *m_ctl;

    /// control register of the ring
    volatile uint32_t *m_mode;
};

#endif /* !RINGBUFFER_H */
