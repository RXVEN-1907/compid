#include <stdio.h>
#include <stdarg.h>
#include <string.h>

/* A miniature printf supporting %d, %x and %s, built on va_arg. */
static void emit(char *out, size_t *n, size_t cap, const char *s) {
    while (*s && *n + 1 < cap) out[(*n)++] = *s++;
}

static void mini_vsprintf(char *out, size_t cap, const char *fmt, va_list ap) {
    size_t n = 0;
    for (const char *p = fmt; *p; p++) {
        if (*p != '%') { if (n + 1 < cap) out[n++] = *p; continue; }
        p++;
        char tmp[24];
        if (*p == 'd') {
            int v = va_arg(ap, int);
            unsigned u = (v < 0) ? (unsigned)(-(long)v) : (unsigned)v;
            int i = 0;
            do { tmp[i++] = (char)('0' + u % 10u); u /= 10u; } while (u);
            if (v < 0 && n + 1 < cap) out[n++] = '-';
            while (i && n + 1 < cap) out[n++] = tmp[--i];
        } else if (*p == 'x') {
            unsigned v = va_arg(ap, unsigned);
            const char *hex = "0123456789abcdef";
            int i = 0;
            do { tmp[i++] = hex[v & 15u]; v >>= 4; } while (v);
            while (i && n + 1 < cap) out[n++] = tmp[--i];
        } else if (*p == 's') {
            emit(out, &n, cap, va_arg(ap, const char *));
        } else {
            if (n + 1 < cap) out[n++] = *p ? *p : '%';
        }
    }
    out[n] = '\0';
}

static void mini_sprintf(char *out, size_t cap, const char *fmt, ...) {
    va_list ap;
    va_start(ap, fmt);
    mini_vsprintf(out, cap, fmt, ap);
    va_end(ap);
}

int main(void) {
    char buf[256];
    unsigned acc = 0;
    const char *tags[] = {"alpha", "beta", "gamma", "delta"};
    for (int i = 0; i < 24; i++) {
        mini_sprintf(buf, sizeof buf, "%s:%d:%x|", tags[i & 3], i * 13, i * 7 + 1);
        for (const char *p = buf; *p; p++) acc = acc * 31u + (unsigned char)*p;
    }
    printf("minivp acc=%u len=%zu\n", acc, strlen(buf));
    return 0;
}
