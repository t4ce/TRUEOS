#ifndef __LIBMX_WF_QUEUE_H__
#define __LIBMX_WF_QUEUE_H__

#include "chain.h"

class alignas(64) WFQueue
{
private:
    WFQueue(const WFQueue &copy);

public:
    alignas(64) volatile int32_t ready;
    alignas(64) uint16_t id;
    alignas(64) uint16_t numa_id;
    alignas(64) Chain *volatile head;

protected:
    alignas(64) Chain *tail;
    alignas(32) Chain stub;

public:
    WFQueue() : ready(-1), id(0), numa_id(0), head(&stub), tail(&stub), stub() {}
    inline void enqueue(Chain *item);
    inline Chain *dequeue();
    inline bool empty() const { return tail == &stub && tail->next == nullptr; }
};

// see
// http://www.1024cores.net/home/lock-free-algorithms/queues/intrusive-mpsc-node-based-queue

inline void WFQueue::enqueue(Chain *item)
{
    item->next = nullptr;
    Chain *prev = __sync_lock_test_and_set(&head, item);
    prev->next = item;
}

inline Chain *WFQueue::dequeue()
{
    Chain *t = (Chain *)tail;
    Chain *n = t->next;
    if (t == &stub)
    {
        if (nullptr == n)
            return nullptr;
        tail = n;
        t = n;
        n = n->next;
    }
    if (n)
    {
        tail = n;
        t->next = nullptr;
        return t;
    }
    volatile Chain *h = head;
    if (t != h)
        return nullptr;
    enqueue(&stub);
    n = t->next;
    if (n)
    {
        tail = n;
        t->next = nullptr;
        return t;
    }
    return nullptr;
}

#endif //__LIBMX_WF_QUEUE_H__
