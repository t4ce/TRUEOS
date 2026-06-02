#include "../stubs.h"
#include "ringbuffer.h"
#include "registers.h"

void RingBuffer::init(uint64_t mmadr, uint32_t *physaddress, uint32_t graphicsaddress, uint32_t size)
{
    m_mmadr = mmadr;
    m_size = size;
    m_start = physaddress;
    m_cur = physaddress;

    // set register pointers
    volatile uint32_t *ring_start = (uint32_t *)(mmadr + RING_BUFFER_START);
    m_tail = (uint32_t *)(mmadr + RING_BUFFER_TAIL);
    m_head = (uint32_t *)(mmadr + RING_BUFFER_HEAD);
    m_ctl = (uint32_t *)(mmadr + RING_BUFFER_CTL);
    m_mode = (uint32_t *)(mmadr + MI_MODE);

    // stop ring first
    stop();

    // set ring start address
    *ring_start = graphicsaddress;

    // set ringbuffer length in m_ctl
    *m_ctl |= ((m_size / 0x1000) - 1) << 12;

    // enable and restart ring
    enable();
    start();
}

void RingBuffer::enable()
{
    // set enable bit
    *m_ctl |= 0x1;
}

void RingBuffer::start()
{
    // clear stop bit
    *m_mode = 0x1000000;
}

void RingBuffer::stop()
{
    // stop the ring
    *m_mode = 0x1000100;

    // wait for the buffer to be in idle mode
    while (!isIdle())
        ;
}

void RingBuffer::enqueue(uint32_t dword)
{
    // add dword
    *m_cur++ = dword;

    // if qword aligned
    if (m_pad)
    {
        // increment qword counter
        m_cnt++;
    }

    // flip padding flag
    m_pad = !m_pad;

    // check for wrap
    if (m_cur == m_start + (m_size / sizeof(uint32_t)))
    {
        // start from beginning
        m_cur = m_start;
        *m_tail = 0;

        // reset counter
        m_cnt = 0;
    }
}

uint32_t RingBuffer::getCurrentTailOffset() const
{
    return (*m_tail >> 3);
}

uint32_t RingBuffer::getCurrentHeadOffset() const
{
    return (*m_head >> 3);
}

void RingBuffer::submit()
{
    // increment tail offset
    uint32_t offset = getCurrentTailOffset();
    offset += m_cnt;
    *m_tail = offset << 3;

    // reset counter
    m_cnt = 0;
}

bool RingBuffer::wait()
{
    // submit commands
    submit();

    // wait loop
    do
    {
        // get error flags
        volatile uint32_t *status = (uint32_t *)(m_mmadr + ESR);

        // first bit means instruction error
        if (*status & 0x1)
        {
            // get active head pointer
            volatile uint32_t *h = (uint32_t *)(m_mmadr + ACTHD);
            uint32_t *errInstr = (m_start + (*h / 4));

            // print error
            printk("Instruction Error: HEAD: 0x%x TAIL: 0x%x Instruction: 0x%x\n", *m_head, *m_tail, *errInstr);
            return false;
        }
    } while ((*m_head & 0x1FFFFF) != *m_tail); // head offset without wrap count != tail offset

    return true;
}