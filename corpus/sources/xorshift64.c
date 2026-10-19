#include <stdio.h>
#include <stdint.h>

/* xorshift64* (Vigna). Exercises 64-bit multiplies and shifts. */
static uint64_t xs = 0x9E3779B97F4A7C15ull;

static uint64_t next(void) {
    xs ^= xs >> 12;
    xs ^= xs << 25;
    xs ^= xs >> 27;
    return xs * 0x2545F4914F6CDD1Dull;
}

int main(void) {
    uint64_t acc = 0;
    for (int i = 0; i < 50000; i++) {
        uint64_t v = next();
        acc += (v >> 33) ^ (v & 0xff);
        if ((v & 7) == 0) acc = (acc << 1) | (acc >> 63);
    }
    printf("xorshift64 acc=%llu\n", (unsigned long long)acc);
    return 0;
}
