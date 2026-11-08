//! Historical Unix-shaped import classifier and retirement diagnostics.
//!
//! Only freestanding byte/string/math helpers still bind. No libc/POSIX
//! allocation, descriptor, process, signal, clock or pthread provider remains.

pub(crate) fn unsupported_unix_import_reason(name: &str) -> Option<&'static str> {
    if is_unix_import(name) && resolve_import(name).is_none() {
        Some(
            "Unix/POSIX compatibility ABI removed; rebuild against native TRUEOS allocation, I/O, time and execution APIs",
        )
    } else {
        None
    }
}

pub(crate) fn is_unix_import(name: &str) -> bool {
    matches!(
        name,
        "abs"
            | "iswspace"
            | "iswalpha"
            | "iswdigit"
            | "iswalnum"
            | "__ctype_b_loc"
            | "__errno"
            | "__errno_location"
            | "__memcpy_chk"
            | "__memmove_chk"
            | "__memset_chk"
            | "__stack_chk_fail"
            | "__xpg_strerror_r"
            | "accept"
            | "accept4"
            | "access"
            | "bind"
            | "calloc"
            | "cfmakeraw"
            | "chdir"
            | "chroot"
            | "clock_gettime"
            | "close"
            | "closedir"
            | "connect"
            | "dirfd"
            | "dladdr"
            | "dlclose"
            | "dlerror"
            | "dlopen"
            | "dlsym"
            | "dup"
            | "dup2"
            | "environ"
            | "errno_location"
            | "execvp"
            | "fchmod"
            | "fchown"
            | "fcntl"
            | "fcntl64"
            | "flock"
            | "fdatasync"
            | "fdopendir"
            | "fork"
            | "free"
            | "freeaddrinfo"
            | "fstat"
            | "fstat64"
            | "fstatat"
            | "fstatat64"
            | "fsync"
            | "ftruncate"
            | "ftruncate64"
            | "futimens"
            | "futimes"
            | "gai_strerror"
            | "getaddrinfo"
            | "getcwd"
            | "geteuid"
            | "getenv"
            | "setenv"
            | "unsetenv"
            | "getpeername"
            | "getpid"
            | "getsockname"
            | "getsockopt"
            | "gettimeofday"
            | "ioctl"
            | "isatty"
            | "killpg"
            | "ldexp"
            | "linkat"
            | "listen"
            | "localtime_r"
            | "log"
            | "lseek"
            | "lstat"
            | "lstat64"
            | "malloc"
            | "memchr"
            | "mkdir"
            | "mkdirat"
            | "mlock"
            | "mmap"
            | "mmap64"
            | "mprotect"
            | "mremap"
            | "munmap"
            | "nanosleep"
            | "open"
            | "open64"
            | "openat"
            | "opendir"
            | "pipe"
            | "poll"
            | "posix_memalign"
            | "pread"
            | "pread64"
            | "pthread_attr_destroy"
            | "pthread_attr_init"
            | "pthread_attr_setstacksize"
            | "pthread_cond_broadcast"
            | "pthread_cond_destroy"
            | "pthread_cond_init"
            | "pthread_cond_signal"
            | "pthread_cond_timedwait"
            | "pthread_cond_wait"
            | "pthread_condattr_destroy"
            | "pthread_condattr_init"
            | "pthread_condattr_setclock"
            | "pthread_create"
            | "pthread_detach"
            | "pthread_getspecific"
            | "pthread_join"
            | "pthread_key_create"
            | "pthread_key_delete"
            | "pthread_kill"
            | "pthread_mutex_destroy"
            | "pthread_mutex_init"
            | "pthread_mutex_lock"
            | "pthread_mutex_trylock"
            | "pthread_mutex_unlock"
            | "pthread_mutexattr_destroy"
            | "pthread_mutexattr_init"
            | "pthread_mutexattr_settype"
            | "pthread_self"
            | "pthread_setname_np"
            | "pthread_setspecific"
            | "pwrite"
            | "pwrite64"
            | "qsort"
            | "raise"
            | "read"
            | "readdir"
            | "readdir_r"
            | "readlink"
            | "readv"
            | "realpath"
            | "realloc"
            | "recv"
            | "recvfrom"
            | "rename"
            | "renameat"
            | "rmdir"
            | "sched_yield"
            | "send"
            | "sendto"
            | "setgid"
            | "setgroups"
            | "setpgid"
            | "setsid"
            | "setsockopt"
            | "shutdown"
            | "setuid"
            | "sigaction"
            | "signal"
            | "socket"
            | "socketpair"
            | "stat"
            | "statvfs"
            | "stat64"
            | "strchr"
            | "strcmp"
            | "strcspn"
            | "strerror_r"
            | "strncmp"
            | "strrchr"
            | "strspn"
            | "sysconf"
            | "tcgetattr"
            | "tcsetattr"
            | "time"
            | "unlink"
            | "unlinkat"
            | "uname"
            | "utimes"
            | "waitpid"
            | "write"
            | "writev"
    )
}

pub(crate) fn resolve_import(name: &str) -> Option<usize> {
    match name {
        "abs" => Some(crate::blueprint_shims::abs as *const () as usize),
        "ldexp" => Some(crate::blueprint_shims::ldexp as *const () as usize),
        "iswspace" => Some(crate::blueprint_shims::ctype::iswspace as *const () as usize),
        "iswalpha" => Some(crate::blueprint_shims::ctype::iswalpha as *const () as usize),
        "iswdigit" => Some(crate::blueprint_shims::ctype::iswdigit as *const () as usize),
        "iswalnum" => Some(crate::blueprint_shims::ctype::iswalnum as *const () as usize),
        "__ctype_b_loc" => Some(crate::blueprint_shims::ctype::__ctype_b_loc as *const () as usize),
        "__memcpy_chk" => Some(crate::blueprint_shims::__memcpy_chk as *const () as usize),
        "__memmove_chk" => Some(crate::blueprint_shims::__memmove_chk as *const () as usize),
        "__memset_chk" => Some(crate::blueprint_shims::__memset_chk as *const () as usize),
        "memchr" => Some(crate::blueprint_shims::memchr as *const () as usize),
        "strchr" => Some(crate::blueprint_shims::strchr as *const () as usize),
        "strcmp" => Some(crate::blueprint_shims::strcmp as *const () as usize),
        "strcspn" => Some(crate::blueprint_shims::strcspn as *const () as usize),
        "strncmp" => Some(crate::blueprint_shims::strncmp as *const () as usize),
        "strrchr" => Some(crate::blueprint_shims::strrchr as *const () as usize),
        "strspn" => Some(crate::blueprint_shims::strspn as *const () as usize),
        "qsort" => Some(crate::blueprint_shims::qsort as *const () as usize),
        "log" => Some(trueos_math::log as *const () as usize),
        _ => None,
    }
}
