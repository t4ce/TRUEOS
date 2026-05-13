#define _GNU_SOURCE

#include <dirent.h>
#include <dlfcn.h>
#include <errno.h>
#include <execinfo.h>
#include <fcntl.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/syscall.h>
#include <sys/sysmacros.h>
#include <time.h>
#include <unistd.h>

#include <drm/drm.h>
#include <drm/i915_drm.h>

#define MAX_FDS 4096
#define MAX_BOS 16384
#define MAX_MAPS 16384
#define STACK_DEPTH_MAX 128

extern char **environ;

typedef struct FdInfo {
    int seen;
    int is_drm;
    char path[256];
} FdInfo;

typedef struct BoInfo {
    uint32_t handle;
    uint64_t size;
    uint64_t mmap_offset;
    void *addr;
    size_t map_len;
    uint64_t map_bo_offset;
    int dumped_pre_exec;
} BoInfo;

typedef struct MapInfo {
    void *addr;
    size_t len;
    int fd;
    uint64_t offset;
    uint32_t handle;
} MapInfo;

static int (*real_open_fn)(const char *, int, ...) = NULL;
static int (*real_open64_fn)(const char *, int, ...) = NULL;
static int (*real_openat_fn)(int, const char *, int, ...) = NULL;
static int (*real_close_fn)(int) = NULL;
static int (*real_ioctl_fn)(int, unsigned long, void *) = NULL;
static void *(*real_mmap_fn)(void *, size_t, int, int, int, off_t) = NULL;
static void *(*real_mmap64_fn)(void *, size_t, int, int, int, off64_t) = NULL;
static int (*real_munmap_fn)(void *, size_t) = NULL;

static FILE *log_file = NULL;
static char out_dir[512] = ".codex_tmp/intel_userland_oracle/latest";
static char dump_dir[512] = ".codex_tmp/intel_userland_oracle/latest/dumps";
static FdInfo fds[MAX_FDS];
static BoInfo bos[MAX_BOS];
static MapInfo maps[MAX_MAPS];
static uint64_t seq_no = 0;
static size_t max_dump_bytes = 0;
static int trace_stacks = 1;
static int trace_stack_depth = 64;
static int trace_snapshots = 1;
static __thread int in_hook = 0;

static void mkdir_one(const char *path) {
    if (mkdir(path, 0755) != 0 && errno != EEXIST) {
        return;
    }
}

static void mkdir_p(const char *path) {
    char tmp[512];
    snprintf(tmp, sizeof(tmp), "%s", path);
    for (char *p = tmp + 1; *p; ++p) {
        if (*p == '/') {
            *p = '\0';
            mkdir_one(tmp);
            *p = '/';
        }
    }
    mkdir_one(tmp);
}

static uint64_t now_ns(void) {
    struct timespec ts;
    clock_gettime(CLOCK_MONOTONIC, &ts);
    return (uint64_t)ts.tv_sec * 1000000000ull + (uint64_t)ts.tv_nsec;
}

static void trace_log(const char *fmt, ...) {
    if (!log_file || in_hook) {
        return;
    }
    in_hook = 1;
    fprintf(log_file, "trace seq=%llu t_ns=%llu ",
            (unsigned long long)++seq_no, (unsigned long long)now_ns());
    va_list ap;
    va_start(ap, fmt);
    vfprintf(log_file, fmt, ap);
    va_end(ap);
    fputc('\n', log_file);
    fflush(log_file);
    in_hook = 0;
}

static void sanitize_token(const char *src, char *dst, size_t dst_len) {
    if (!dst_len) {
        return;
    }
    size_t j = 0;
    for (size_t i = 0; src && src[i] && j + 1 < dst_len; ++i) {
        char c = src[i];
        if ((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') ||
            (c >= '0' && c <= '9') || c == '_' || c == '-' || c == '.') {
            dst[j++] = c;
        } else {
            dst[j++] = '_';
        }
    }
    dst[j] = '\0';
}

static void trace_stack(const char *tag) {
    if (!log_file || !trace_stacks || in_hook) {
        return;
    }

    in_hook = 1;
    void *frames[STACK_DEPTH_MAX];
    int depth = trace_stack_depth;
    if (depth <= 0 || depth > STACK_DEPTH_MAX) {
        depth = STACK_DEPTH_MAX;
    }
    int count = backtrace(frames, depth);
    char **symbols = backtrace_symbols(frames, count);
    uint64_t seq = ++seq_no;
    fprintf(log_file,
            "trace seq=%llu t_ns=%llu stack tag=%s pid=%d tid=%ld depth=%d\n",
            (unsigned long long)seq,
            (unsigned long long)now_ns(),
            tag,
            getpid(),
            (long)syscall(SYS_gettid),
            count);
    for (int i = 0; i < count; ++i) {
        fprintf(log_file,
                "trace-stack seq=%llu tag=%s frame=%d pc=%p symbol=\"%s\"\n",
                (unsigned long long)seq,
                tag,
                i,
                frames[i],
                symbols ? symbols[i] : "?");
    }
    free(symbols);
    fflush(log_file);
    in_hook = 0;
}

static void log_artifact_direct(const char *tag, const char *file) {
    if (!log_file) {
        return;
    }
    fprintf(log_file,
            "trace seq=%llu t_ns=%llu artifact tag=%s file=\"dumps/%s\"\n",
            (unsigned long long)++seq_no,
            (unsigned long long)now_ns(),
            tag,
            file);
    fflush(log_file);
}

static void copy_proc_file_snapshot(const char *tag, const char *proc_name, const char *suffix) {
    char src_path[128];
    char safe_tag[128];
    char name[320];
    char dst_path[1024];

    sanitize_token(tag, safe_tag, sizeof(safe_tag));
    snprintf(src_path, sizeof(src_path), "/proc/self/%s", proc_name);
    snprintf(name,
             sizeof(name),
             "%06llu_%s_%s.txt",
             (unsigned long long)seq_no,
             safe_tag,
             suffix);
    snprintf(dst_path, sizeof(dst_path), "%s/%s", dump_dir, name);

    FILE *src = fopen(src_path, "rb");
    if (!src) {
        return;
    }
    FILE *dst = fopen(dst_path, "wb");
    if (!dst) {
        fclose(src);
        return;
    }

    char buf[16384];
    size_t n;
    while ((n = fread(buf, 1, sizeof(buf), src)) > 0) {
        fwrite(buf, 1, n, dst);
    }
    fclose(dst);
    fclose(src);
    log_artifact_direct(tag, name);
}

static void write_environ_snapshot(const char *tag) {
    char safe_tag[128];
    char name[320];
    char path[1024];

    sanitize_token(tag, safe_tag, sizeof(safe_tag));
    snprintf(name,
             sizeof(name),
             "%06llu_%s_environ.txt",
             (unsigned long long)seq_no,
             safe_tag);
    snprintf(path, sizeof(path), "%s/%s", dump_dir, name);

    FILE *f = fopen(path, "wb");
    if (!f) {
        return;
    }
    for (char **e = environ; e && *e; ++e) {
        fprintf(f, "%s\n", *e);
    }
    fclose(f);
    log_artifact_direct(tag, name);
}

static void write_fd_snapshot(const char *tag) {
    char safe_tag[128];
    char name[320];
    char path[1024];

    sanitize_token(tag, safe_tag, sizeof(safe_tag));
    snprintf(name,
             sizeof(name),
             "%06llu_%s_fd.txt",
             (unsigned long long)seq_no,
             safe_tag);
    snprintf(path, sizeof(path), "%s/%s", dump_dir, name);

    DIR *dir = opendir("/proc/self/fd");
    if (!dir) {
        return;
    }
    FILE *f = fopen(path, "wb");
    if (!f) {
        closedir(dir);
        return;
    }

    struct dirent *de;
    while ((de = readdir(dir)) != NULL) {
        if (de->d_name[0] == '.') {
            continue;
        }
        char link_path[512];
        char target[512];
        snprintf(link_path, sizeof(link_path), "/proc/self/fd/%s", de->d_name);
        ssize_t len = readlink(link_path, target, sizeof(target) - 1);
        if (len < 0) {
            snprintf(target, sizeof(target), "? errno=%d", errno);
        } else {
            target[len] = '\0';
        }
        fprintf(f, "fd=%s target=\"%s\"\n", de->d_name, target);
    }
    fclose(f);
    closedir(dir);
    log_artifact_direct(tag, name);
}

static void snapshot_process_state(const char *tag, int include_heavy) {
    if (!trace_snapshots || in_hook) {
        return;
    }
    in_hook = 1;
    copy_proc_file_snapshot(tag, "status", "proc_status");
    copy_proc_file_snapshot(tag, "maps", "proc_maps");
    copy_proc_file_snapshot(tag, "limits", "proc_limits");
    copy_proc_file_snapshot(tag, "mountinfo", "proc_mountinfo");
    write_fd_snapshot(tag);
    if (include_heavy) {
        copy_proc_file_snapshot(tag, "smaps", "proc_smaps");
        write_environ_snapshot(tag);
    }
    in_hook = 0;
}

static const char *request_name(unsigned long request) {
    switch (request) {
        case DRM_IOCTL_VERSION:
            return "DRM_IOCTL_VERSION";
        case DRM_IOCTL_GEM_CLOSE:
            return "DRM_IOCTL_GEM_CLOSE";
#ifdef DRM_IOCTL_GET_CAP
        case DRM_IOCTL_GET_CAP:
            return "DRM_IOCTL_GET_CAP";
#endif
#ifdef DRM_IOCTL_SYNCOBJ_CREATE
        case DRM_IOCTL_SYNCOBJ_CREATE:
            return "DRM_IOCTL_SYNCOBJ_CREATE";
#endif
#ifdef DRM_IOCTL_SYNCOBJ_DESTROY
        case DRM_IOCTL_SYNCOBJ_DESTROY:
            return "DRM_IOCTL_SYNCOBJ_DESTROY";
#endif
#ifdef DRM_IOCTL_SYNCOBJ_WAIT
        case DRM_IOCTL_SYNCOBJ_WAIT:
            return "DRM_IOCTL_SYNCOBJ_WAIT";
#endif
#ifdef DRM_IOCTL_SYNCOBJ_TIMELINE_WAIT
        case DRM_IOCTL_SYNCOBJ_TIMELINE_WAIT:
            return "DRM_IOCTL_SYNCOBJ_TIMELINE_WAIT";
#endif
#ifdef DRM_IOCTL_SYNCOBJ_TIMELINE_SIGNAL
        case DRM_IOCTL_SYNCOBJ_TIMELINE_SIGNAL:
            return "DRM_IOCTL_SYNCOBJ_TIMELINE_SIGNAL";
#endif
        case DRM_IOCTL_I915_GETPARAM:
            return "DRM_IOCTL_I915_GETPARAM";
#ifdef DRM_IOCTL_I915_QUERY
        case DRM_IOCTL_I915_QUERY:
            return "DRM_IOCTL_I915_QUERY";
#endif
        case DRM_IOCTL_I915_GEM_CREATE:
            return "DRM_IOCTL_I915_GEM_CREATE";
        case DRM_IOCTL_I915_GEM_CREATE_EXT:
            return "DRM_IOCTL_I915_GEM_CREATE_EXT";
        case DRM_IOCTL_I915_GEM_MMAP:
            return "DRM_IOCTL_I915_GEM_MMAP";
        case DRM_IOCTL_I915_GEM_MMAP_GTT:
            return "DRM_IOCTL_I915_GEM_MMAP_GTT";
        case DRM_IOCTL_I915_GEM_MMAP_OFFSET:
            return "DRM_IOCTL_I915_GEM_MMAP_OFFSET";
        case DRM_IOCTL_I915_GEM_USERPTR:
            return "DRM_IOCTL_I915_GEM_USERPTR";
        case DRM_IOCTL_I915_GEM_EXECBUFFER2:
            return "DRM_IOCTL_I915_GEM_EXECBUFFER2";
        case DRM_IOCTL_I915_GEM_EXECBUFFER2_WR:
            return "DRM_IOCTL_I915_GEM_EXECBUFFER2_WR";
        case DRM_IOCTL_I915_GEM_WAIT:
            return "DRM_IOCTL_I915_GEM_WAIT";
        case DRM_IOCTL_I915_GEM_BUSY:
            return "DRM_IOCTL_I915_GEM_BUSY";
        case DRM_IOCTL_I915_GEM_SET_DOMAIN:
            return "DRM_IOCTL_I915_GEM_SET_DOMAIN";
        case DRM_IOCTL_I915_GEM_CONTEXT_CREATE:
            return "DRM_IOCTL_I915_GEM_CONTEXT_CREATE";
        case DRM_IOCTL_I915_GEM_CONTEXT_CREATE_EXT:
            return "DRM_IOCTL_I915_GEM_CONTEXT_CREATE_EXT";
        case DRM_IOCTL_I915_GEM_CONTEXT_DESTROY:
            return "DRM_IOCTL_I915_GEM_CONTEXT_DESTROY";
        case DRM_IOCTL_I915_GEM_CONTEXT_SETPARAM:
            return "DRM_IOCTL_I915_GEM_CONTEXT_SETPARAM";
        case DRM_IOCTL_I915_GEM_CONTEXT_GETPARAM:
            return "DRM_IOCTL_I915_GEM_CONTEXT_GETPARAM";
        default:
            return "UNKNOWN";
    }
}

static BoInfo *find_bo(uint32_t handle) {
    if (handle == 0) {
        return NULL;
    }
    for (size_t i = 0; i < MAX_BOS; ++i) {
        if (bos[i].handle == handle) {
            return &bos[i];
        }
    }
    return NULL;
}

static BoInfo *upsert_bo(uint32_t handle) {
    BoInfo *existing = find_bo(handle);
    if (existing) {
        return existing;
    }
    for (size_t i = 0; i < MAX_BOS; ++i) {
        if (bos[i].handle == 0) {
            bos[i].handle = handle;
            return &bos[i];
        }
    }
    return NULL;
}

static BoInfo *find_bo_by_mmap_offset(uint64_t offset) {
    for (size_t i = 0; i < MAX_BOS; ++i) {
        if (bos[i].handle && bos[i].mmap_offset == offset) {
            return &bos[i];
        }
    }
    return NULL;
}

static void forget_bo(uint32_t handle) {
    for (size_t i = 0; i < MAX_BOS; ++i) {
        if (bos[i].handle == handle) {
            memset(&bos[i], 0, sizeof(bos[i]));
            return;
        }
    }
}

static void track_fd(int fd, const char *path) {
    if (fd < 0 || fd >= MAX_FDS) {
        return;
    }
    fds[fd].seen = 1;
    snprintf(fds[fd].path, sizeof(fds[fd].path), "%s", path ? path : "?");
    fds[fd].is_drm = strstr(fds[fd].path, "/dev/dri/") != NULL;
}

static void discover_fd_path(int fd) {
    if (fd < 0 || fd >= MAX_FDS || fds[fd].seen) {
        return;
    }
    char proc_path[64];
    char target[256];
    snprintf(proc_path, sizeof(proc_path), "/proc/self/fd/%d", fd);
    ssize_t len = readlink(proc_path, target, sizeof(target) - 1);
    if (len > 0) {
        target[len] = '\0';
        track_fd(fd, target);
    }
}

static void write_blob(const char *name, const void *data, size_t len) {
    if (!data || len == 0) {
        return;
    }
    char path[768];
    snprintf(path, sizeof(path), "%s/%s", dump_dir, name);
    FILE *f = fopen(path, "wb");
    if (!f) {
        trace_log("dump-failed path=\"%s\" errno=%d", path, errno);
        return;
    }
    fwrite(data, 1, len, f);
    fclose(f);
}

static void dump_bo_bytes(const char *phase, uint32_t handle, uint64_t offset, void *addr, size_t len) {
    if (!addr || len == 0) {
        return;
    }
    size_t dump_len = (max_dump_bytes == 0 || len < max_dump_bytes) ? len : max_dump_bytes;
    char name[256];
    snprintf(
        name,
        sizeof(name),
        "%06llu_%s_handle_%u_off_0x%llX_len_0x%zX.bin",
        (unsigned long long)seq_no,
        phase,
        handle,
        (unsigned long long)offset,
        dump_len
    );
    write_blob(name, addr, dump_len);
    const uint32_t *words = (const uint32_t *)addr;
    size_t word_count = dump_len / 4;
    trace_log(
        "bo-dump phase=%s handle=%u offset=0x%llX map_len=0x%zX dump_len=0x%zX first_words=0x%08X,0x%08X,0x%08X,0x%08X file=\"dumps/%s\"",
        phase,
        handle,
        (unsigned long long)offset,
        len,
        dump_len,
        word_count > 0 ? words[0] : 0,
        word_count > 1 ? words[1] : 0,
        word_count > 2 ? words[2] : 0,
        word_count > 3 ? words[3] : 0,
        name
    );
}

static size_t round_up_to_page(size_t len) {
    const size_t page_size = 4096;
    return (len + page_size - 1) & ~(page_size - 1);
}

static void dump_temp_mmap_bytes(int fd,
                                 const char *phase,
                                 uint32_t handle,
                                 const BoInfo *bo,
                                 uint64_t start_offset,
                                 size_t dump_len) {
    if (fd < 0 || !phase || !bo || bo->mmap_offset == 0 || bo->size == 0) {
        return;
    }
    if (start_offset >= bo->size) {
        return;
    }

    uint64_t remaining = bo->size - start_offset;
    if (remaining < dump_len) {
        dump_len = (size_t)remaining;
    }
    if (dump_len == 0) {
        return;
    }

    uint64_t page_offset = start_offset & ~0xFFFULL;
    size_t page_delta = (size_t)(start_offset - page_offset);
    size_t map_len = round_up_to_page(page_delta + dump_len);
    void *mapped = real_mmap64_fn(
        NULL,
        map_len,
        PROT_READ,
        MAP_SHARED,
        fd,
        (off64_t)(bo->mmap_offset + page_offset)
    );
    if (mapped == MAP_FAILED) {
        trace_log(
            "bo-temp-map-failed phase=%s handle=%u start_offset=0x%llX fake_offset=0x%llX map_len=0x%zX errno=%d",
            phase,
            handle,
            (unsigned long long)start_offset,
            (unsigned long long)(bo->mmap_offset + page_offset),
            map_len,
            errno
        );
        return;
    }

    dump_bo_bytes(
        phase,
        handle,
        start_offset,
        (void *)((uint8_t *)mapped + page_delta),
        dump_len
    );
    real_munmap_fn(mapped, map_len);
}

static void dump_batch_start_window(int fd,
                                    const char *phase,
                                    const struct drm_i915_gem_execbuffer2 *exec,
                                    const struct drm_i915_gem_exec_object2 *obj,
                                    const BoInfo *bo) {
    if (!exec || !obj || !bo || strcmp(phase, "pre") != 0) {
        return;
    }
    if (fd < 0 || bo->mmap_offset == 0 || bo->size == 0) {
        return;
    }

    uint64_t batch_start = exec->batch_start_offset;
    if (batch_start >= bo->size) {
        trace_log(
            "bo-dump-batch-start-skip handle=%u reason=batch-start-out-of-range batch_start=0x%llX bo_size=0x%llX",
            obj->handle,
            (unsigned long long)batch_start,
            (unsigned long long)bo->size
        );
        return;
    }

    if (bo->addr && batch_start >= bo->map_bo_offset &&
        batch_start < bo->map_bo_offset + bo->map_len) {
        return;
    }

    size_t dump_len = 0x6000;
    dump_temp_mmap_bytes(fd, "pre_exec_batch_start", obj->handle, bo, batch_start, dump_len);
}

static void remember_map(void *addr, size_t len, int fd, uint64_t offset, uint32_t handle) {
    for (size_t i = 0; i < MAX_MAPS; ++i) {
        if (maps[i].addr == NULL) {
            maps[i].addr = addr;
            maps[i].len = len;
            maps[i].fd = fd;
            maps[i].offset = offset;
            maps[i].handle = handle;
            return;
        }
    }
}

static MapInfo *find_map(void *addr) {
    for (size_t i = 0; i < MAX_MAPS; ++i) {
        if (maps[i].addr == addr) {
            return &maps[i];
        }
    }
    return NULL;
}

static void forget_map(void *addr) {
    for (size_t i = 0; i < MAX_MAPS; ++i) {
        if (maps[i].addr == addr) {
            memset(&maps[i], 0, sizeof(maps[i]));
            return;
        }
    }
}

static int ranges_overlap(uintptr_t a, size_t a_len, uintptr_t b, size_t b_len) {
    uintptr_t a_end = a + a_len;
    uintptr_t b_end = b + b_len;
    return a < b_end && b < a_end;
}

static void update_bo_for_munmap(void *addr, size_t len) {
    uintptr_t unmap_start = (uintptr_t)addr;
    for (size_t i = 0; i < MAX_BOS; ++i) {
        if (!bos[i].handle || !bos[i].addr || bos[i].map_len == 0) {
            continue;
        }
        uintptr_t bo_start = (uintptr_t)bos[i].addr;
        if (!ranges_overlap(unmap_start, len, bo_start, bos[i].map_len)) {
            continue;
        }
        if (unmap_start == bo_start && len < bos[i].map_len) {
            bos[i].addr = (void *)(bo_start + len);
            bos[i].map_len -= len;
            bos[i].map_bo_offset += len;
            trace_log(
                "bo-map-trim-front handle=%u new_addr=%p new_map_len=0x%zX new_bo_offset=0x%llX",
                bos[i].handle,
                bos[i].addr,
                bos[i].map_len,
                (unsigned long long)bos[i].map_bo_offset
            );
        } else {
            trace_log("bo-map-clear handle=%u reason=munmap-overlap", bos[i].handle);
            bos[i].addr = NULL;
            bos[i].map_len = 0;
            bos[i].map_bo_offset = 0;
        }
    }
}

static void log_execbuffer(int fd, const struct drm_i915_gem_execbuffer2 *exec, const char *phase) {
    if (!exec) {
        return;
    }
    char tag[64];
    snprintf(tag, sizeof(tag), "execbuffer-%s", phase);
    trace_stack(tag);
    if (strcmp(phase, "pre") == 0) {
        snapshot_process_state("execbuffer-pre", 0);
    }
    trace_log(
        "execbuffer-%s buffers_ptr=0x%llX buffer_count=%u batch_start=0x%X batch_len=0x%X flags=0x%llX rsvd1=0x%llX rsvd2=0x%llX",
        phase,
        (unsigned long long)exec->buffers_ptr,
        exec->buffer_count,
        exec->batch_start_offset,
        exec->batch_len,
        (unsigned long long)exec->flags,
        (unsigned long long)exec->rsvd1,
        (unsigned long long)exec->rsvd2
    );

    if (!exec->buffers_ptr || exec->buffer_count > 4096) {
        trace_log("execbuffer-%s objects-unavailable reason=bad-pointer-or-count", phase);
        return;
    }

    const struct drm_i915_gem_exec_object2 *objects =
        (const struct drm_i915_gem_exec_object2 *)(uintptr_t)exec->buffers_ptr;
    for (uint32_t i = 0; i < exec->buffer_count; ++i) {
        const struct drm_i915_gem_exec_object2 *obj = &objects[i];
        BoInfo *bo = find_bo(obj->handle);
        trace_log(
            "execbuffer-%s object[%u] handle=%u size=0x%llX offset=0x%llX alignment=0x%llX flags=0x%llX reloc_count=%u relocs_ptr=0x%llX mapped=%d map_len=0x%zX",
            phase,
            i,
            obj->handle,
            bo ? (unsigned long long)bo->size : 0ull,
            (unsigned long long)obj->offset,
            (unsigned long long)obj->alignment,
            (unsigned long long)obj->flags,
            obj->relocation_count,
            (unsigned long long)obj->relocs_ptr,
            bo && bo->addr,
            bo ? bo->map_len : 0
        );
        if (bo && bo->addr && !bo->dumped_pre_exec) {
            dump_bo_bytes("pre_exec", obj->handle, bo->map_bo_offset, bo->addr, bo->map_len);
            if (strcmp(phase, "pre") == 0 && i == 0) {
                dump_temp_mmap_bytes(fd, "pre_exec_full_object", obj->handle, bo, 0, (size_t)bo->size);
            }
            dump_batch_start_window(fd, phase, exec, obj, bo);
            bo->dumped_pre_exec = 1;
        }
    }
}

static void init_real(void) {
    if (real_ioctl_fn) {
        return;
    }
    real_open_fn = dlsym(RTLD_NEXT, "open");
    real_open64_fn = dlsym(RTLD_NEXT, "open64");
    real_openat_fn = dlsym(RTLD_NEXT, "openat");
    real_close_fn = dlsym(RTLD_NEXT, "close");
    real_ioctl_fn = dlsym(RTLD_NEXT, "ioctl");
    real_mmap_fn = dlsym(RTLD_NEXT, "mmap");
    real_mmap64_fn = dlsym(RTLD_NEXT, "mmap64");
    real_munmap_fn = dlsym(RTLD_NEXT, "munmap");
}

__attribute__((constructor))
static void trace_ctor(void) {
    in_hook = 1;
    init_real();
    const char *dir = getenv("TRUEOS_ORACLE_LOG_DIR");
    if (dir && dir[0]) {
        snprintf(out_dir, sizeof(out_dir), "%s", dir);
        snprintf(dump_dir, sizeof(dump_dir), "%s/dumps", dir);
    }
    const char *max_dump = getenv("TRUEOS_ORACLE_MAX_DUMP_BYTES");
    if (max_dump && max_dump[0]) {
        max_dump_bytes = strtoull(max_dump, NULL, 0);
    }
    const char *stacks = getenv("TRUEOS_ORACLE_TRACE_STACKS");
    if (stacks && stacks[0]) {
        trace_stacks = atoi(stacks) != 0;
    }
    const char *stack_depth = getenv("TRUEOS_ORACLE_STACK_DEPTH");
    if (stack_depth && stack_depth[0]) {
        trace_stack_depth = atoi(stack_depth);
        if (trace_stack_depth <= 0) {
            trace_stack_depth = 1;
        } else if (trace_stack_depth > STACK_DEPTH_MAX) {
            trace_stack_depth = STACK_DEPTH_MAX;
        }
    }
    const char *snapshots = getenv("TRUEOS_ORACLE_TRACE_SNAPSHOTS");
    if (snapshots && snapshots[0]) {
        trace_snapshots = atoi(snapshots) != 0;
    }
    mkdir_p(out_dir);
    mkdir_p(dump_dir);
    char log_path[768];
    snprintf(log_path, sizeof(log_path), "%s/log.txt", out_dir);
    log_file = fopen(log_path, "a");
    in_hook = 0;
    trace_log("trace-start pid=%d out_dir=\"%s\" max_dump_mode=%s max_dump_cap=0x%zX trace_stacks=%d stack_depth=%d trace_snapshots=%d",
              getpid(),
              out_dir,
              max_dump_bytes ? "capped" : "full",
              max_dump_bytes,
              trace_stacks,
              trace_stack_depth,
              trace_snapshots);
    snapshot_process_state("start", 1);
}

__attribute__((destructor))
static void trace_dtor(void) {
    snapshot_process_state("end", 1);
    trace_log("trace-end pid=%d", getpid());
    if (log_file) {
        fclose(log_file);
        log_file = NULL;
    }
}

int open(const char *path, int flags, ...) {
    init_real();
    mode_t mode = 0;
    if (flags & O_CREAT) {
        va_list ap;
        va_start(ap, flags);
        mode = (mode_t)va_arg(ap, int);
        va_end(ap);
    }
    int fd = (flags & O_CREAT) ? real_open_fn(path, flags, mode) : real_open_fn(path, flags);
    if (!in_hook && fd >= 0) {
        track_fd(fd, path);
        trace_log("open fd=%d flags=0x%X path=\"%s\"", fd, flags, path);
        trace_stack("open");
    }
    return fd;
}

int open64(const char *path, int flags, ...) {
    init_real();
    mode_t mode = 0;
    if (flags & O_CREAT) {
        va_list ap;
        va_start(ap, flags);
        mode = (mode_t)va_arg(ap, int);
        va_end(ap);
    }
    int fd = (flags & O_CREAT) ? real_open64_fn(path, flags, mode) : real_open64_fn(path, flags);
    if (!in_hook && fd >= 0) {
        track_fd(fd, path);
        trace_log("open64 fd=%d flags=0x%X path=\"%s\"", fd, flags, path);
        trace_stack("open64");
    }
    return fd;
}

int openat(int dirfd, const char *path, int flags, ...) {
    init_real();
    mode_t mode = 0;
    if (flags & O_CREAT) {
        va_list ap;
        va_start(ap, flags);
        mode = (mode_t)va_arg(ap, int);
        va_end(ap);
    }
    int fd = (flags & O_CREAT) ? real_openat_fn(dirfd, path, flags, mode) : real_openat_fn(dirfd, path, flags);
    if (!in_hook && fd >= 0) {
        discover_fd_path(fd);
        if (fd < MAX_FDS && !fds[fd].seen) {
            track_fd(fd, path);
        }
        trace_log("openat dirfd=%d fd=%d flags=0x%X path=\"%s\" resolved=\"%s\"",
                  dirfd, fd, flags, path, (fd < MAX_FDS && fds[fd].seen) ? fds[fd].path : "?");
        trace_stack("openat");
    }
    return fd;
}

int close(int fd) {
    init_real();
    if (!in_hook) {
        trace_log("close fd=%d path=\"%s\"", fd, (fd >= 0 && fd < MAX_FDS && fds[fd].seen) ? fds[fd].path : "?");
        trace_stack("close");
    }
    int ret = real_close_fn(fd);
    if (fd >= 0 && fd < MAX_FDS) {
        memset(&fds[fd], 0, sizeof(fds[fd]));
    }
    return ret;
}

void *mmap(void *addr, size_t len, int prot, int flags, int fd, off_t offset) {
    init_real();
    void *result = real_mmap_fn(addr, len, prot, flags, fd, offset);
    if (!in_hook && result != MAP_FAILED) {
        discover_fd_path(fd);
        uint32_t handle = 0;
        BoInfo *bo = find_bo_by_mmap_offset((uint64_t)offset);
        if (bo) {
            handle = bo->handle;
            bo->addr = result;
            bo->map_len = len;
            bo->map_bo_offset = 0;
        }
        remember_map(result, len, fd, (uint64_t)offset, handle);
        trace_log(
            "mmap fd=%d path=\"%s\" addr=%p len=0x%zX prot=0x%X flags=0x%X offset=0x%llX handle=%u",
            fd,
            (fd >= 0 && fd < MAX_FDS && fds[fd].seen) ? fds[fd].path : "?",
            result,
            len,
            prot,
            flags,
            (unsigned long long)(uint64_t)offset,
            handle
        );
        trace_stack("mmap");
    }
    return result;
}

void *mmap64(void *addr, size_t len, int prot, int flags, int fd, off64_t offset) {
    init_real();
    void *result = real_mmap64_fn(addr, len, prot, flags, fd, offset);
    if (!in_hook && result != MAP_FAILED) {
        discover_fd_path(fd);
        uint32_t handle = 0;
        BoInfo *bo = find_bo_by_mmap_offset((uint64_t)offset);
        if (bo) {
            handle = bo->handle;
            bo->addr = result;
            bo->map_len = len;
            bo->map_bo_offset = 0;
        }
        remember_map(result, len, fd, (uint64_t)offset, handle);
        trace_log(
            "mmap64 fd=%d path=\"%s\" addr=%p len=0x%zX prot=0x%X flags=0x%X offset=0x%llX handle=%u",
            fd,
            (fd >= 0 && fd < MAX_FDS && fds[fd].seen) ? fds[fd].path : "?",
            result,
            len,
            prot,
            flags,
            (unsigned long long)(uint64_t)offset,
            handle
        );
        trace_stack("mmap64");
    }
    return result;
}

int munmap(void *addr, size_t len) {
    init_real();
    if (!in_hook) {
        MapInfo *map = find_map(addr);
        if (map && map->handle) {
            dump_bo_bytes("munmap", map->handle, 0, addr, map->len < len ? map->len : len);
        }
        trace_log("munmap addr=%p len=0x%zX handle=%u", addr, len, map ? map->handle : 0);
        trace_stack("munmap");
        update_bo_for_munmap(addr, len);
        forget_map(addr);
    }
    return real_munmap_fn(addr, len);
}

int ioctl(int fd, unsigned long request, ...) {
    init_real();
    void *arg = NULL;
    va_list ap;
    va_start(ap, request);
    arg = va_arg(ap, void *);
    va_end(ap);

    discover_fd_path(fd);
    const char *name = request_name(request);
    if (!in_hook) {
        trace_log(
            "ioctl-enter fd=%d path=\"%s\" request=0x%lX name=%s arg=%p",
            fd,
            (fd >= 0 && fd < MAX_FDS && fds[fd].seen) ? fds[fd].path : "?",
            request,
            name,
            arg
        );
        trace_stack("ioctl-enter");
        if (request == DRM_IOCTL_I915_GEM_EXECBUFFER2 || request == DRM_IOCTL_I915_GEM_EXECBUFFER2_WR) {
            log_execbuffer(fd, (const struct drm_i915_gem_execbuffer2 *)arg, "pre");
        }
    }

    int ret = real_ioctl_fn(fd, request, arg);
    int saved_errno = errno;

    if (!in_hook) {
        trace_log("ioctl-exit fd=%d request=0x%lX name=%s ret=%d errno=%d", fd, request, name, ret, saved_errno);
        trace_stack("ioctl-exit");

        if (ret == 0 && request == DRM_IOCTL_I915_GEM_CREATE) {
            struct drm_i915_gem_create *create = (struct drm_i915_gem_create *)arg;
            BoInfo *bo = upsert_bo(create->handle);
            if (bo) {
                bo->size = create->size;
            }
            trace_log("gem-create handle=%u size=0x%llX", create->handle, (unsigned long long)create->size);
        } else if (ret == 0 && request == DRM_IOCTL_I915_GEM_CREATE_EXT) {
            struct drm_i915_gem_create_ext *create = (struct drm_i915_gem_create_ext *)arg;
            BoInfo *bo = upsert_bo(create->handle);
            if (bo) {
                bo->size = create->size;
            }
            trace_log("gem-create-ext handle=%u size=0x%llX flags=0x%X extensions=0x%llX",
                      create->handle, (unsigned long long)create->size, create->flags,
                      (unsigned long long)create->extensions);
        } else if (ret == 0 && request == DRM_IOCTL_I915_GEM_USERPTR) {
            struct drm_i915_gem_userptr *userptr = (struct drm_i915_gem_userptr *)arg;
            BoInfo *bo = upsert_bo(userptr->handle);
            if (bo) {
                bo->size = userptr->user_size;
                bo->addr = (void *)(uintptr_t)userptr->user_ptr;
                bo->map_len = (size_t)userptr->user_size;
            }
            trace_log("gem-userptr handle=%u user_ptr=0x%llX size=0x%llX flags=0x%X",
                      userptr->handle, (unsigned long long)userptr->user_ptr,
                      (unsigned long long)userptr->user_size, userptr->flags);
        } else if (ret == 0 && request == DRM_IOCTL_I915_GEM_MMAP) {
            struct drm_i915_gem_mmap *m = (struct drm_i915_gem_mmap *)arg;
            BoInfo *bo = upsert_bo(m->handle);
            if (bo) {
                bo->addr = (void *)(uintptr_t)m->addr_ptr;
                bo->map_len = (size_t)m->size;
            }
            trace_log("gem-mmap handle=%u addr=0x%llX offset=0x%llX size=0x%llX flags=0x%llX",
                      m->handle, (unsigned long long)m->addr_ptr,
                      (unsigned long long)m->offset, (unsigned long long)m->size,
                      (unsigned long long)m->flags);
        } else if (ret == 0 && (request == DRM_IOCTL_I915_GEM_MMAP_GTT || request == DRM_IOCTL_I915_GEM_MMAP_OFFSET)) {
            struct drm_i915_gem_mmap_offset *m = (struct drm_i915_gem_mmap_offset *)arg;
            BoInfo *bo = upsert_bo(m->handle);
            if (bo) {
                bo->mmap_offset = m->offset;
            }
            trace_log("gem-mmap-offset handle=%u fake_offset=0x%llX flags=0x%llX",
                      m->handle, (unsigned long long)m->offset, (unsigned long long)m->flags);
        } else if (ret == 0 && (request == DRM_IOCTL_I915_GEM_EXECBUFFER2 || request == DRM_IOCTL_I915_GEM_EXECBUFFER2_WR)) {
            log_execbuffer(fd, (const struct drm_i915_gem_execbuffer2 *)arg, "post");
        } else if (ret == 0 && request == DRM_IOCTL_GEM_CLOSE) {
            struct drm_gem_close *close_arg = (struct drm_gem_close *)arg;
            trace_log("gem-close handle=%u", close_arg->handle);
            forget_bo(close_arg->handle);
        }
    }

    errno = saved_errno;
    return ret;
}
