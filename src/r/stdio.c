#include <stdarg.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
// im not sure if we use this in some meaningful way entire file imean.
extern void trueos_cabi_write(uint32_t stream, const uint8_t *bytes, size_t len);
extern void trueos_cabi_write_cstr(uint32_t stream, const uint8_t *cstr);
extern int32_t trueos_cabi_copy_cstr_into(uint8_t *dst, size_t cap, const uint8_t *cstr);

static int _out;
static int _err;
static int _in;

void *stdout = &_out;
void *stderr = &_err;
void *stdin = &_in;

static void out_char(char *dst, size_t cap, size_t *total, char ch) {
    if (cap != 0 && *total + 1 < cap) {
        dst[*total] = ch;
    }
    (*total)++;
}

static void out_bytes(char *dst, size_t cap, size_t *total, const char *src, size_t len) {
    for (size_t i = 0; i < len; i++) {
        out_char(dst, cap, total, src[i]);
    }
}

static size_t cstr_len(const char *s) {
    if (!s) {
        return 0;
    }
    size_t n = 0;
    while (s[n] != '\0') {
        n++;
    }
    return n;
}

static void out_u64_base(char *dst, size_t cap, size_t *total, uint64_t value, uint32_t base, bool upper) {
    char tmp[32];
    size_t i = 0;

    if (value == 0) {
        out_char(dst, cap, total, '0');
        return;
    }

    while (value != 0 && i < sizeof(tmp)) {
        uint32_t digit = (uint32_t)(value % base);
        if (digit < 10) {
            tmp[i++] = (char)('0' + digit);
        } else {
            tmp[i++] = (char)((upper ? 'A' : 'a') + (digit - 10));
        }
        value /= base;
    }

    while (i > 0) {
        out_char(dst, cap, total, tmp[--i]);
    }
}

static int kvsnprintf(char *dst, size_t cap, const char *fmt, va_list ap) {
    size_t total = 0;

    for (size_t i = 0; fmt && fmt[i] != '\0'; i++) {
        if (fmt[i] != '%') {
            out_char(dst, cap, &total, fmt[i]);
            continue;
        }

        i++;
        if (fmt[i] == '\0') {
            break;
        }
        if (fmt[i] == '%') {
            out_char(dst, cap, &total, '%');
            continue;
        }

        while (fmt[i] == '-' || fmt[i] == '+' || fmt[i] == ' ' || fmt[i] == '#' || fmt[i] == '0') {
            i++;
        }

        if (fmt[i] == '*') {
            (void)va_arg(ap, int);
            i++;
        } else {
            while (fmt[i] >= '0' && fmt[i] <= '9') {
                i++;
            }
        }

        if (fmt[i] == '.') {
            i++;
            if (fmt[i] == '*') {
                (void)va_arg(ap, int);
                i++;
            } else {
                while (fmt[i] >= '0' && fmt[i] <= '9') {
                    i++;
                }
            }
        }

        enum { LEN_NONE, LEN_L, LEN_LL, LEN_Z } len = LEN_NONE;
        if (fmt[i] == 'l') {
            if (fmt[i + 1] == 'l') {
                len = LEN_LL;
                i += 2;
            } else {
                len = LEN_L;
                i++;
            }
        } else if (fmt[i] == 'z') {
            len = LEN_Z;
            i++;
        } else if (fmt[i] == 'h') {
            i++;
            if (fmt[i] == 'h') {
                i++;
            }
        }

        char spec = fmt[i];
        switch (spec) {
            case 's': {
                const char *s = va_arg(ap, const char *);
                if (!s) {
                    s = "(null)";
                }
                out_bytes(dst, cap, &total, s, cstr_len(s));
                break;
            }
            case 'c': {
                int ch = va_arg(ap, int);
                out_char(dst, cap, &total, (char)ch);
                break;
            }
            case 'd':
            case 'i': {
                int64_t v;
                if (len == LEN_LL) {
                    v = va_arg(ap, long long);
                } else if (len == LEN_L) {
                    v = va_arg(ap, long);
                } else if (len == LEN_Z) {
                    v = (int64_t)va_arg(ap, ptrdiff_t);
                } else {
                    v = va_arg(ap, int);
                }
                if (v < 0) {
                    out_char(dst, cap, &total, '-');
                    uint64_t uv = (uint64_t)(-(v + 1)) + 1;
                    out_u64_base(dst, cap, &total, uv, 10, false);
                } else {
                    out_u64_base(dst, cap, &total, (uint64_t)v, 10, false);
                }
                break;
            }
            case 'u': {
                uint64_t v;
                if (len == LEN_LL) {
                    v = va_arg(ap, unsigned long long);
                } else if (len == LEN_L) {
                    v = va_arg(ap, unsigned long);
                } else if (len == LEN_Z) {
                    v = (uint64_t)va_arg(ap, size_t);
                } else {
                    v = va_arg(ap, unsigned int);
                }
                out_u64_base(dst, cap, &total, v, 10, false);
                break;
            }
            case 'x':
            case 'X': {
                bool upper = (spec == 'X');
                uint64_t v;
                if (len == LEN_LL) {
                    v = va_arg(ap, unsigned long long);
                } else if (len == LEN_L) {
                    v = va_arg(ap, unsigned long);
                } else if (len == LEN_Z) {
                    v = (uint64_t)va_arg(ap, size_t);
                } else {
                    v = va_arg(ap, unsigned int);
                }
                out_u64_base(dst, cap, &total, v, 16, upper);
                break;
            }
            case 'p': {
                uintptr_t p = (uintptr_t)va_arg(ap, void *);
                out_bytes(dst, cap, &total, "0x", 2);
                out_u64_base(dst, cap, &total, (uint64_t)p, 16, false);
                break;
            }
            default: {
                out_char(dst, cap, &total, '%');
                out_char(dst, cap, &total, spec);
                break;
            }
        }
    }

    if (cap != 0) {
        size_t term = (total < cap) ? total : (cap - 1);
        dst[term] = '\0';
    }

    return (int)total;
}

int vsnprintf(char *s, size_t n, const char *fmt, va_list ap) {
    return kvsnprintf(s, n, fmt, ap);
}

int vfprintf(void *stream, const char *fmt, va_list ap) {
    char buf[1024];
    int needed = kvsnprintf(buf, sizeof(buf), fmt, ap);
    if (needed < 0) {
        return needed;
    }

    uint32_t sid = (stream == stderr) ? 2 : 1;
    size_t to_write = (size_t)needed;
    if (to_write >= sizeof(buf)) {
        to_write = sizeof(buf) - 1;
    }
    if (to_write != 0) {
        trueos_cabi_write(sid, (const uint8_t *)buf, to_write);
    }
    return needed;
}

int vprintf(const char *fmt, va_list ap) {
    return vfprintf(stdout, fmt, ap);
}

int printf(const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    int ret = vfprintf(stdout, fmt, ap);
    va_end(ap);
    return ret;
}

int fprintf(void *stream, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    int ret = vfprintf(stream, fmt, ap);
    va_end(ap);
    return ret;
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

int snprintf(char *s, size_t n, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    int ret = vsnprintf(s, n, fmt, ap);
    va_end(ap);
    return ret;
}
