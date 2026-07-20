#include <stdarg.h>
#include <stddef.h>

int vsnprintf(char *s, size_t n, const char *fmt, va_list ap) {
    (void)fmt;
    (void)ap;
    if (s && n != 0) {
        s[0] = '\0';
    }
    return 0;
}

int snprintf(char *s, size_t n, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    int ret = vsnprintf(s, n, fmt, ap);
    va_end(ap);
    return ret;
}

int __vsnprintf_chk(char *s, size_t n, int flag, size_t object_size,
                    const char *fmt, va_list ap) {
    (void)flag;
    if (object_size < n) {
        n = object_size;
    }
    return vsnprintf(s, n, fmt, ap);
}

int __snprintf_chk(char *s, size_t n, int flag, size_t object_size,
                   const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    int ret = __vsnprintf_chk(s, n, flag, object_size, fmt, ap);
    va_end(ap);
    return ret;
}
