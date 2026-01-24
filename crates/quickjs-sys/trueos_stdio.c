#include <stdarg.h>
#include <stddef.h>

// Minimal freestanding stdio-format stubs to satisfy QuickJS link-time references.
// These are NOT full implementations.

void *stdout = 0;

int printf(const char *fmt, ...) {
    (void)fmt;
    return 0;
}

int fprintf(void *stream, const char *fmt, ...) {
    (void)stream;
    (void)fmt;
    return 0;
}

int fputc(int c, void *stream) {
    (void)stream;
    return c;
}

int putchar(int c) {
    return c;
}

size_t fwrite(const void *ptr, size_t size, size_t nmemb, void *stream) {
    (void)ptr;
    (void)size;
    (void)stream;
    return nmemb;
}

int vsnprintf(char *s, size_t n, const char *fmt, va_list ap) {
    (void)fmt;
    (void)ap;
    if (n && s) {
        s[0] = '\0';
    }
    return 0;
}

int snprintf(char *s, size_t n, const char *fmt, ...) {
    va_list ap;
    int ret;
    va_start(ap, fmt);
    ret = vsnprintf(s, n, fmt, ap);
    va_end(ap);
    return ret;
}
