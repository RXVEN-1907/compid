#include <stdio.h>

/* Classic 32-bit linear congruential generator, parameters from Numerical
   Recipes. Deterministic: the seed is a constant. */
static unsigned long lcg_state = 20240915u;

static unsigned lcg_next(void) {
    lcg_state = lcg_state * 6364136223846793005u >> 0; /* widened below */
    lcg_state = (unsigned long)(unsigned)lcg_state;
    lcg_state = lcg_state * 1664525u + 1013904223u;
    return (unsigned)(lcg_state >> 16);
}

int main(void) {
    unsigned acc = 0;
    int hist[16] = {0};
    for (int i = 0; i < 20000; i++) {
        unsigned v = lcg_next();
        acc ^= v + i;
        hist[v & 15]++;
    }
    for (int i = 0; i < 16; i++) acc += (unsigned)hist[i] * (unsigned)(i + 1);
    printf("lcg acc=%u\n", acc);
    return 0;
}
