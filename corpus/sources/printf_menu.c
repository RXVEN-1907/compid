#include <stdio.h>

/* printf-heavy: many distinct format specifiers in one hot function. */
static const char *items[] = {
    "encoder", "decoder", "mux", "demux", "filter", "sampler",
};

int main(void) {
    for (int i = 0; i < 6; i++) {
        printf("[%2d] %-8s rev=%u key=%#x gain=%.3g load=%g%%\n",
               i, items[i], 100u + (unsigned)i * 7u,
               0x5a00u | (unsigned)(i * 33), 0.25 * (i + 1) / 3.0,
               12.5 * i);
    }
    printf("done %d items\n", 6);
    return 0;
}
