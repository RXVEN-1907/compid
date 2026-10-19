#include <stdio.h>
#include <stdint.h>

/* Gray-code encode/decode and a Gray-reflected walk over 10 bits. */
static uint32_t gray_encode(uint32_t n) { return n ^ (n >> 1); }

static uint32_t gray_decode(uint32_t g) {
    uint32_t n = g;
    n ^= n >> 16;
    n ^= n >> 8;
    n ^= n >> 4;
    n ^= n >> 2;
    n ^= n >> 1;
    return n;
}

int main(void) {
    long acc = 0;
    for (uint32_t n = 0; n < 1u << 16; n++) {
        uint32_t g = gray_encode(n);
        if (gray_decode(g) != n) acc += 999;
        acc += __builtin_popcount(g ^ gray_encode(n + 1)) - 1; /* Hamming 1 */
    }
    uint32_t g = 0;
    for (int step = 0; step < 1023; step++) {
        g = gray_encode(g + 1);
        acc ^= g & 0x3FF;
    }
    printf("gray acc=%ld\n", acc);
    return 0;
}
