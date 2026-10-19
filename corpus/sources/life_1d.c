#include <stdio.h>
#include <stdint.h>

/* Rule-30 one-dimensional cellular automaton, 96 cells, 64 generations. */
#define CELLS 96
#define GENS 64

static uint8_t row_a[CELLS + 2], row_b[CELLS + 2];

int main(void) {
    row_a[CELLS / 2] = 1;
    uint8_t *cur = row_a, *nxt = row_b;
    unsigned acc = 0;
    for (int g = 0; g < GENS; g++) {
        for (int i = 0; i < CELLS; i++) {
            uint8_t l = cur[i], c = cur[i + 1], r = cur[i + 2];
            uint8_t pattern = (uint8_t)((l << 2) | (c << 1) | r);
            nxt[i + 1] = (pattern == 1 || pattern == 2 ||
                          pattern == 3 || pattern == 4) ? 1 : 0;
            acc = (acc << 1) | nxt[i + 1];
        }
        uint8_t *t = cur;
        cur = nxt;
        nxt = t;
    }
    printf("life acc=%u live=%u\n", acc, (unsigned)(cur[CELLS / 2]));
    return 0;
}
