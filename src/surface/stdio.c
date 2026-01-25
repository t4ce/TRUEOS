#include <stdarg.h>
#include <stddef.h>
#include <stdint.h>

extern void trueos_cabi_write(uint32_t stream, const uint8_t *bytes, size_t len);
extern void trueos_cabi_write_cstr(uint32_t stream, const uint8_t *cstr);
extern int32_t trueos_cabi_copy_cstr_into(uint8_t *dst, size_t cap, const uint8_t *cstr);

static int _out;
static int _err;
static int _in;

void *stdout = &_out;
void *stderr = &_err;
void *stdin = &_in;

int printf(const char *fmt, ...) {
    trueos_cabi_write_cstr(1, (const uint8_t *)fmt);
    return 0;
}

int fprintf(void *stream, const char *fmt, ...) {
    uint32_t sid = (stream == stderr) ? 2 : 1;
    trueos_cabi_write_cstr(sid, (const uint8_t *)fmt);
    return 0;
}

int fputs(const char *s, void *stream) {
    uint32_t sid = (stream == stderr) ? 2 : 1;
    trueos_cabi_write_cstr(sid, (const uint8_t *)s);
    return 0;
}

int puts(const char *s) {
    static const char nl = '\n';
    trueos_cabi_write_cstr(1, (const uint8_t *)s);
    trueos_cabi_write(1, (const uint8_t *)&nl, 1);
    return 0;
}

int fputc(int c, void *stream) {
    char ch = (char)c;
    uint32_t sid = (stream == stderr) ? 2 : 1;
    trueos_cabi_write(sid, (const uint8_t *)&ch, 1);
    return c;
}

int putchar(int c) {
    char ch = (char)c;
    trueos_cabi_write(1, (const uint8_t *)&ch, 1);
    return c;
}

size_t fwrite(const void *ptr, size_t size, size_t nmemb, void *stream) {
    if (ptr && size && nmemb) {
        size_t len = size * nmemb;
        uint32_t sid = (stream == stderr) ? 2 : 1;
        trueos_cabi_write(sid, (const uint8_t *)ptr, len);
    }
    return nmemb;
}

int vsnprintf(char *s, size_t n, const char *fmt, va_list ap) {
    (void)ap;
    return (int)trueos_cabi_copy_cstr_into((uint8_t *)s, n, (const uint8_t *)fmt);
}

int snprintf(char *s, size_t n, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    int ret = vsnprintf(s, n, fmt, ap);
    va_end(ap);
    return ret;
}
