#include <stdio.h>
#include <stdint.h>

/* MurmurHash3's finalizer plus integer-block hashing. */
static inline uint32_t rotl32(uint32_t x, int r) {
    return (x << r) | (x >> (32 - r));
}

static uint32_t fmix32(uint32_t h) {
    h ^= h >> 16;
    h *= 0x85EBCA6Bu;
    h ^= h >> 13;
    h *= 0xC2B2AE35u;
    h ^= h >> 16;
    return h;
}

static uint32_t murmur3_int(uint32_t k) {
    k *= 0xCC9E2D51u;
    k = rotl32(k, 15);
    k *= 0x1B873593u;
    return fmix32(k);
}

int main(void) {
    uint32_t acc = 0;
    for (uint32_t i = 0; i < 65536; i++) {
        uint32_t h = murmur3_int(i ^ 0xDEADBEEFu);
        acc += rotl32(h, (int)(h & 31)) ^ i;
    }
    printf("murmur3 acc=%u\n", acc);
    return 0;
}
