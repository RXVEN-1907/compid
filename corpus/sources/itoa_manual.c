#include <stdio.h>
#include <string.h>

/* Integer-to-string by hand for bases 10 and 16, into fixed buffers. */
static const char digits_lc[] = "0123456789abcdef";

static size_t utoa_buf(unsigned long v, unsigned base, char *dst, size_t cap) {
    char tmp[72];
    size_t n = 0;
    do {
        tmp[n++] = digits_lc[v % base];
        v /= base;
    } while (v && n < sizeof tmp);
    if (n >= cap) { dst[0] = '\0'; return 0; }
    for (size_t i = 0; i < n; i++) dst[i] = tmp[n - 1 - i];
    dst[n] = '\0';
    return n;
}

int main(void) {
    char a[80], b[80];
    unsigned acc = 0;
    unsigned long vals[] = {0, 1, 15, 16, 255, 4096, 65535, 0xCAFEBABE,
                            0xFFFFFFFFul, 1234567890};
    for (unsigned i = 0; i < sizeof vals / sizeof vals[0]; i++) {
        size_t la = utoa_buf(vals[i], 10, a, sizeof a);
        size_t lb = utoa_buf(vals[i] ^ 0xA5A5A5A5ul, 16, b, sizeof b);
        acc += (unsigned)(la * 100 + lb);
        acc ^= (unsigned)strcmp(a, b);
    }
    printf("itoa acc=%u last=%s\n", acc, b);
    return 0;
}
