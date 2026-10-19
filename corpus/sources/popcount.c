#include <stdio.h>
#include <stdint.h>

/* Population count three ways, cross-checked against the builtin. */
static int pop_naive(uint32_t v) {
    int c = 0;
    while (v) { c += v & 1u; v >>= 1; }
    return c;
}

static int pop_swar(uint32_t v) {
    v = v - ((v >> 1) & 0x55555555u);
    v = (v & 0x33333333u) + ((v >> 2) & 0x33333333u);
    v = (v + (v >> 4)) & 0x0F0F0F0Fu;
    return (int)((v * 0x01010101u) >> 24);
}

int main(void) {
    long acc = 0;
    uint32_t s = 0xC0FFEE11u;
    for (int i = 0; i < 65536; i++) {
        s ^= s << 7; s ^= s >> 3; s ^= s << 12;
        int a = pop_naive(s), b = pop_swar(s), c = __builtin_popcount(s);
        acc += a + b + c;
        if (a != b || b != c) acc += 1000;
    }
    printf("popcount acc=%ld\n", acc);
    return 0;
}
