#include <stdio.h>
#include <stdint.h>

/* djb2 with a small open-addressing table and linear probing. */
#define SLOTS 128

static const char *keys[] = {
    "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf",
    "hotel", "india", "juliet", "kilo", "lima", "mike", "november",
};

static uint32_t djb2(const char *s) {
    uint32_t h = 5381u;
    while (*s) h = h * 33u ^ (unsigned char)*s++;
    return h;
}

int main(void) {
    uint32_t table[SLOTS];
    int used[SLOTS];
    for (int i = 0; i < SLOTS; i++) { table[i] = 0; used[i] = 0; }
    int collisions = 0;
    for (unsigned round = 0; round < 4; round++) {
        for (unsigned k = 0; k < sizeof keys / sizeof keys[0]; k++) {
            uint32_t h = djb2(keys[k]) ^ (round * 2654435761u);
            uint32_t slot = h & (SLOTS - 1);
            while (used[slot] && table[slot] != h) {
                slot = (slot + 1) & (SLOTS - 1);
                collisions++;
            }
            used[slot] = 1;
            table[slot] = h;
        }
    }
    uint32_t acc = 0;
    for (int i = 0; i < SLOTS; i++) acc = acc * 3u + table[i] + (uint32_t)used[i];
    printf("djb2 acc=%u collisions=%d\n", acc, collisions);
    return 0;
}
