// Tiny single-threaded executor demo.
//
// Build:
//   gcc -std=c11 -Wall -Wextra -O2 tools/mini_executor.c -o /tmp/mini_executor
//
// Run:
//   /tmp/mini_executor
//
// This is deliberately "executor-shaped", not a real Embassy port:
// - tasks are hand-written state machines
// - "await" means: save state, arrange a wakeup, return PENDING
// - the executor polls only ready tasks
// - the platform sleep is just nanosleep()

#define _POSIX_C_SOURCE 200809L

#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <time.h>

#define MAX_TASKS 8
#define NO_DEADLINE UINT64_MAX

typedef enum {
    TASK_PENDING,
    TASK_DONE,
} task_status_t;

struct executor;
struct task;

typedef task_status_t (*task_fn_t)(struct executor *ex, struct task *task);

typedef struct task {
    const char *name;
    task_fn_t poll;
    int state;
    int value;
    bool ready;
    bool done;
    uint64_t deadline_ms;
} task_t;

typedef struct executor {
    task_t tasks[MAX_TASKS];
    size_t task_count;
} executor_t;

static uint64_t now_ms(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint64_t)ts.tv_sec * 1000u + (uint64_t)ts.tv_nsec / 1000000u;
}

static void sleep_ms(uint64_t ms) {
    struct timespec ts;
    ts.tv_sec = (time_t)(ms / 1000u);
    ts.tv_nsec = (long)((ms % 1000u) * 1000000u);
    nanosleep(&ts, NULL);
}

static task_t *spawn(executor_t *ex, const char *name, task_fn_t poll) {
    if (ex->task_count >= MAX_TASKS) {
        return NULL;
    }

    task_t *task = &ex->tasks[ex->task_count++];
    memset(task, 0, sizeof(*task));
    task->name = name;
    task->poll = poll;
    task->ready = true;
    task->deadline_ms = NO_DEADLINE;
    return task;
}

static void wake_task(task_t *task) {
    if (!task->done) {
        task->ready = true;
    }
}

static task_status_t task_sleep(task_t *task, uint64_t delay_ms, int next_state) {
    task->deadline_ms = now_ms() + delay_ms;
    task->state = next_state;
    return TASK_PENDING;
}

static void wake_expired_timers(executor_t *ex) {
    uint64_t now = now_ms();

    for (size_t i = 0; i < ex->task_count; i++) {
        task_t *task = &ex->tasks[i];
        if (!task->done && task->deadline_ms != NO_DEADLINE && task->deadline_ms <= now) {
            task->deadline_ms = NO_DEADLINE;
            wake_task(task);
        }
    }
}

static bool has_live_tasks(const executor_t *ex) {
    for (size_t i = 0; i < ex->task_count; i++) {
        if (!ex->tasks[i].done) {
            return true;
        }
    }
    return false;
}

static bool has_ready_tasks(const executor_t *ex) {
    for (size_t i = 0; i < ex->task_count; i++) {
        if (!ex->tasks[i].done && ex->tasks[i].ready) {
            return true;
        }
    }
    return false;
}

static uint64_t ms_until_next_deadline(const executor_t *ex) {
    uint64_t now = now_ms();
    uint64_t best = NO_DEADLINE;

    for (size_t i = 0; i < ex->task_count; i++) {
        const task_t *task = &ex->tasks[i];
        if (!task->done && task->deadline_ms < best) {
            best = task->deadline_ms;
        }
    }

    if (best == NO_DEADLINE || best <= now) {
        return 0;
    }
    return best - now;
}

static void executor_run(executor_t *ex) {
    while (has_live_tasks(ex)) {
        wake_expired_timers(ex);

        for (size_t i = 0; i < ex->task_count; i++) {
            task_t *task = &ex->tasks[i];
            if (task->done || !task->ready) {
                continue;
            }

            task->ready = false;
            task_status_t status = task->poll(ex, task);
            if (status == TASK_DONE) {
                task->done = true;
                printf("[executor] %s done\n", task->name);
            }
        }

        if (!has_ready_tasks(ex)) {
            uint64_t idle_ms = ms_until_next_deadline(ex);
            if (idle_ms > 50) {
                idle_ms = 50;
            }
            if (idle_ms > 0) {
                printf("[executor] idle for %llu ms\n", (unsigned long long)idle_ms);
                sleep_ms(idle_ms);
            }
        }
    }
}

static task_status_t blink_task(executor_t *ex, task_t *task) {
    (void)ex;

    switch (task->state) {
    case 0:
        if (task->value >= 5) {
            return TASK_DONE;
        }
        printf("[blink] on  #%d\n", task->value + 1);
        return task_sleep(task, 150, 1);

    case 1:
        printf("[blink] off #%d\n", task->value + 1);
        task->value++;
        return task_sleep(task, 250, 0);

    default:
        return TASK_DONE;
    }
}

static task_status_t fake_network_task(executor_t *ex, task_t *task) {
    (void)ex;

    switch (task->state) {
    case 0:
        printf("[net] start request\n");
        return task_sleep(task, 500, 1);

    case 1:
        printf("[net] dns complete, opening socket\n");
        return task_sleep(task, 350, 2);

    case 2:
        printf("[net] response received\n");
        return TASK_DONE;

    default:
        return TASK_DONE;
    }
}

static task_status_t spawner_task(executor_t *ex, task_t *task) {
    switch (task->state) {
    case 0:
        printf("[spawner] waiting before spawning late task\n");
        return task_sleep(task, 700, 1);

    case 1:
        printf("[spawner] spawn late blink\n");
        if (spawn(ex, "late-blink", blink_task) == NULL) {
            printf("[spawner] spawn failed\n");
        }
        return TASK_DONE;

    default:
        return TASK_DONE;
    }
}

int main(void) {
    executor_t ex = {0};

    spawn(&ex, "blink", blink_task);
    spawn(&ex, "fake-net", fake_network_task);
    spawn(&ex, "spawner", spawner_task);

    executor_run(&ex);
    return 0;
}
