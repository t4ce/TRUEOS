//! UNIX compatibility import surface for Blueprint REL modules.
//!
//! This layer is intentionally separate from the native TRUEOS CABI. It catches
//! libc/POSIX-shaped imports emitted by Rust std, libc crates, and bundled C
//! artifacts, then forwards them to the current shim implementations.

pub(crate) fn is_unix_import(name: &str) -> bool {
    matches!(
        name,
        "__errno"
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
            | "clock_gettime"
            | "close"
            | "closedir"
            | "dirfd"
            | "dlclose"
            | "dlerror"
            | "dlopen"
            | "dlsym"
            | "errno_location"
            | "fchmod"
            | "fchown"
            | "fcntl"
            | "fcntl64"
            | "free"
            | "freeaddrinfo"
            | "fstat"
            | "fstat64"
            | "fsync"
            | "ftruncate64"
            | "gai_strerror"
            | "getaddrinfo"
            | "getcwd"
            | "geteuid"
            | "getenv"
            | "getpeername"
            | "getpid"
            | "getsockname"
            | "gettimeofday"
            | "listen"
            | "localtime_r"
            | "log"
            | "lseek"
            | "lstat"
            | "lstat64"
            | "malloc"
            | "memchr"
            | "mkdir"
            | "mmap64"
            | "mremap"
            | "munmap"
            | "nanosleep"
            | "open"
            | "open64"
            | "opendir"
            | "posix_memalign"
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
            | "pwrite64"
            | "qsort"
            | "read"
            | "readdir"
            | "readdir_r"
            | "readlink"
            | "readv"
            | "realpath"
            | "realloc"
            | "recv"
            | "rmdir"
            | "sched_yield"
            | "send"
            | "setgid"
            | "setgroups"
            | "setpgid"
            | "setsid"
            | "setsockopt"
            | "setuid"
            | "signal"
            | "socket"
            | "stat"
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
            | "utimes"
            | "waitpid"
            | "write"
            | "writev"
    )
}

pub(crate) fn resolve_import(name: &str) -> Option<usize> {
    match name {
        "__errno" => Some(crate::std_abi_shim::__errno as *const () as usize),
        "__errno_location" => Some(crate::std_abi_shim::__errno_location as *const () as usize),
        "__memcpy_chk" => Some(crate::std_abi_shim::__memcpy_chk as *const () as usize),
        "__memmove_chk" => Some(crate::std_abi_shim::__memmove_chk as *const () as usize),
        "__memset_chk" => Some(crate::std_abi_shim::__memset_chk as *const () as usize),
        "__stack_chk_fail" => Some(crate::std_abi_shim::__stack_chk_fail as *const () as usize),
        "__xpg_strerror_r" => Some(crate::std_abi_shim::__xpg_strerror_r as *const () as usize),
        "accept" => Some(crate::std_abi_shim::accept as *const () as usize),
        "accept4" => Some(crate::std_abi_shim::accept4 as *const () as usize),
        "access" => Some(crate::std_abi_shim::access as *const () as usize),
        "bind" => Some(crate::std_abi_shim::bind as *const () as usize),
        "calloc" => Some(crate::std_abi_shim::calloc as *const () as usize),
        "clock_gettime" => Some(trueos_qjs::trueos_shims::clock_gettime as *const () as usize),
        "close" => Some(crate::std_abi_shim::close as *const () as usize),
        "closedir" => Some(crate::std_abi_shim::closedir as *const () as usize),
        "dirfd" => Some(crate::std_abi_shim::dirfd as *const () as usize),
        "dlclose" => Some(crate::std_abi_shim::dlclose as *const () as usize),
        "dlerror" => Some(crate::std_abi_shim::dlerror as *const () as usize),
        "dlopen" => Some(crate::std_abi_shim::dlopen as *const () as usize),
        "dlsym" => Some(crate::std_abi_shim::dlsym as *const () as usize),
        "errno_location" => Some(crate::std_abi_shim::errno_location as *const () as usize),
        "fchmod" => Some(crate::std_abi_shim::fchmod as *const () as usize),
        "fchown" => Some(crate::std_abi_shim::fchown as *const () as usize),
        "fcntl" => Some(crate::std_abi_shim::fcntl as *const () as usize),
        "fcntl64" => Some(crate::std_abi_shim::fcntl64 as *const () as usize),
        "free" => Some(crate::std_abi_shim::free as *const () as usize),
        "freeaddrinfo" => Some(crate::std_abi_shim::freeaddrinfo as *const () as usize),
        "fstat" => Some(crate::std_abi_shim::fstat as *const () as usize),
        "fstat64" => Some(crate::std_abi_shim::fstat64 as *const () as usize),
        "fsync" => Some(crate::std_abi_shim::fsync as *const () as usize),
        "ftruncate64" => Some(crate::std_abi_shim::ftruncate64 as *const () as usize),
        "gai_strerror" => Some(crate::std_abi_shim::gai_strerror as *const () as usize),
        "getaddrinfo" => Some(crate::std_abi_shim::getaddrinfo as *const () as usize),
        "getcwd" => Some(crate::std_abi_shim::getcwd as *const () as usize),
        "geteuid" => Some(crate::std_abi_shim::geteuid as *const () as usize),
        "getenv" => Some(crate::r::io::env::getenv as *const () as usize),
        "getpeername" => Some(crate::std_abi_shim::getpeername as *const () as usize),
        "getpid" => Some(crate::std_abi_shim::getpid as *const () as usize),
        "getsockname" => Some(crate::std_abi_shim::getsockname as *const () as usize),
        "gettimeofday" => Some(crate::std_abi_shim::gettimeofday as *const () as usize),
        "listen" => Some(crate::std_abi_shim::listen as *const () as usize),
        "localtime_r" => Some(crate::std_abi_shim::localtime_r as *const () as usize),
        "log" => Some(crate::std_abi_shim::log as *const () as usize),
        "lseek" => Some(crate::std_abi_shim::lseek as *const () as usize),
        "lstat" => Some(crate::std_abi_shim::lstat as *const () as usize),
        "lstat64" => Some(crate::std_abi_shim::lstat64 as *const () as usize),
        "malloc" => Some(crate::std_abi_shim::malloc as *const () as usize),
        "memchr" => Some(crate::std_abi_shim::memchr as *const () as usize),
        "mkdir" => Some(crate::std_abi_shim::mkdir as *const () as usize),
        "mmap64" => Some(crate::std_abi_shim::mmap64 as *const () as usize),
        "mremap" => Some(crate::std_abi_shim::mremap as *const () as usize),
        "munmap" => Some(crate::std_abi_shim::munmap as *const () as usize),
        "nanosleep" => Some(crate::std_abi_shim::nanosleep as *const () as usize),
        "open" => Some(crate::std_abi_shim::open as *const () as usize),
        "open64" => Some(crate::std_abi_shim::open64 as *const () as usize),
        "opendir" => Some(crate::std_abi_shim::opendir as *const () as usize),
        "posix_memalign" => Some(crate::std_abi_shim::posix_memalign as *const () as usize),
        "pread64" => Some(crate::std_abi_shim::pread64 as *const () as usize),
        "pthread_attr_destroy" => {
            Some(crate::std_abi_shim::pthread_attr_destroy as *const () as usize)
        }
        "pthread_attr_init" => Some(crate::std_abi_shim::pthread_attr_init as *const () as usize),
        "pthread_attr_setstacksize" => {
            Some(crate::std_abi_shim::pthread_attr_setstacksize as *const () as usize)
        }
        "pthread_cond_broadcast" => {
            Some(crate::std_abi_shim::pthread_cond_broadcast as *const () as usize)
        }
        "pthread_cond_destroy" => {
            Some(crate::std_abi_shim::pthread_cond_destroy as *const () as usize)
        }
        "pthread_cond_init" => Some(crate::std_abi_shim::pthread_cond_init as *const () as usize),
        "pthread_cond_signal" => {
            Some(crate::std_abi_shim::pthread_cond_signal as *const () as usize)
        }
        "pthread_cond_timedwait" => {
            Some(crate::std_abi_shim::pthread_cond_timedwait as *const () as usize)
        }
        "pthread_cond_wait" => Some(crate::std_abi_shim::pthread_cond_wait as *const () as usize),
        "pthread_condattr_destroy" => {
            Some(crate::std_abi_shim::pthread_condattr_destroy as *const () as usize)
        }
        "pthread_condattr_init" => {
            Some(crate::std_abi_shim::pthread_condattr_init as *const () as usize)
        }
        "pthread_condattr_setclock" => {
            Some(crate::std_abi_shim::pthread_condattr_setclock as *const () as usize)
        }
        "pthread_create" => Some(crate::std_abi_shim::pthread_create as *const () as usize),
        "pthread_detach" => Some(crate::std_abi_shim::pthread_detach as *const () as usize),
        "pthread_getspecific" => {
            Some(crate::std_abi_shim::pthread_getspecific as *const () as usize)
        }
        "pthread_join" => Some(crate::std_abi_shim::pthread_join as *const () as usize),
        "pthread_key_create" => Some(crate::std_abi_shim::pthread_key_create as *const () as usize),
        "pthread_key_delete" => Some(crate::std_abi_shim::pthread_key_delete as *const () as usize),
        "pthread_mutex_destroy" => {
            Some(crate::std_abi_shim::pthread_mutex_destroy as *const () as usize)
        }
        "pthread_mutex_init" => Some(crate::std_abi_shim::pthread_mutex_init as *const () as usize),
        "pthread_mutex_lock" => Some(crate::std_abi_shim::pthread_mutex_lock as *const () as usize),
        "pthread_mutex_trylock" => {
            Some(crate::std_abi_shim::pthread_mutex_trylock as *const () as usize)
        }
        "pthread_mutex_unlock" => {
            Some(crate::std_abi_shim::pthread_mutex_unlock as *const () as usize)
        }
        "pthread_mutexattr_destroy" => {
            Some(crate::std_abi_shim::pthread_mutexattr_destroy as *const () as usize)
        }
        "pthread_mutexattr_init" => {
            Some(crate::std_abi_shim::pthread_mutexattr_init as *const () as usize)
        }
        "pthread_mutexattr_settype" => {
            Some(crate::std_abi_shim::pthread_mutexattr_settype as *const () as usize)
        }
        "pthread_self" => Some(crate::std_abi_shim::pthread_self as *const () as usize),
        "pthread_setname_np" => Some(crate::std_abi_shim::pthread_setname_np as *const () as usize),
        "pthread_setspecific" => {
            Some(crate::std_abi_shim::pthread_setspecific as *const () as usize)
        }
        "pwrite64" => Some(crate::std_abi_shim::pwrite64 as *const () as usize),
        "qsort" => Some(crate::std_abi_shim::qsort as *const () as usize),
        "read" => Some(crate::std_abi_shim::read as *const () as usize),
        "readdir" => Some(crate::std_abi_shim::readdir as *const () as usize),
        "readdir_r" => Some(crate::std_abi_shim::readdir_r as *const () as usize),
        "readlink" => Some(crate::std_abi_shim::readlink as *const () as usize),
        "readv" => Some(crate::std_abi_shim::readv as *const () as usize),
        "realpath" => Some(crate::std_abi_shim::realpath as *const () as usize),
        "realloc" => Some(crate::std_abi_shim::realloc as *const () as usize),
        "recv" => Some(crate::std_abi_shim::recv as *const () as usize),
        "rmdir" => Some(crate::std_abi_shim::rmdir as *const () as usize),
        "sched_yield" => Some(crate::std_abi_shim::sched_yield as *const () as usize),
        "send" => Some(crate::std_abi_shim::send as *const () as usize),
        "setgid" => Some(crate::std_abi_shim::setgid as *const () as usize),
        "setgroups" => Some(crate::std_abi_shim::setgroups as *const () as usize),
        "setpgid" => Some(crate::std_abi_shim::setpgid as *const () as usize),
        "setsid" => Some(crate::std_abi_shim::setsid as *const () as usize),
        "setsockopt" => Some(crate::std_abi_shim::setsockopt as *const () as usize),
        "setuid" => Some(crate::std_abi_shim::setuid as *const () as usize),
        "signal" => Some(crate::std_abi_shim::signal as *const () as usize),
        "socket" => Some(crate::std_abi_shim::socket as *const () as usize),
        "stat" => Some(crate::std_abi_shim::stat as *const () as usize),
        "stat64" => Some(crate::std_abi_shim::stat64 as *const () as usize),
        "strchr" => Some(crate::std_abi_shim::strchr as *const () as usize),
        "strcmp" => Some(crate::std_abi_shim::strcmp as *const () as usize),
        "strcspn" => Some(crate::std_abi_shim::strcspn as *const () as usize),
        "strerror_r" => Some(crate::std_abi_shim::strerror_r as *const () as usize),
        "strncmp" => Some(crate::std_abi_shim::strncmp as *const () as usize),
        "strrchr" => Some(crate::std_abi_shim::strrchr as *const () as usize),
        "strspn" => Some(crate::std_abi_shim::strspn as *const () as usize),
        "sysconf" => Some(crate::std_abi_shim::sysconf as *const () as usize),
        "tcgetattr" => Some(crate::std_abi_shim::tcgetattr as *const () as usize),
        "tcsetattr" => Some(crate::std_abi_shim::tcsetattr as *const () as usize),
        "time" => Some(crate::std_abi_shim::time as *const () as usize),
        "unlink" => Some(crate::std_abi_shim::unlink as *const () as usize),
        "utimes" => Some(crate::std_abi_shim::utimes as *const () as usize),
        "waitpid" => Some(crate::std_abi_shim::waitpid as *const () as usize),
        "write" => Some(crate::std_abi_shim::write as *const () as usize),
        "writev" => Some(crate::std_abi_shim::writev as *const () as usize),
        _ => None,
    }
}
