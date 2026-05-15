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
