#include <stdio.h>
#include <stdint.h>
#include <string.h>

/* FNV-1a 64-bit over a table of strings. */
static const char *words[] = {
    "aurora", "boreal", "cirrus", "nimbus", "zephyr", "sirocco",
    "mistral", "chinook", "harmattan", "tramontana",
};

static uint64_t fnv1a(const char *s) {
    uint64_t h = 0xCBF29CE484222325ull;
    for (; *s; s++) {
        h ^= (unsigned char)*s;
        h *= 0x100000001B3ull;
    }
    return h;
}

int main(void) {
    uint64_t acc = 0;
    char tmp[64];
    for (int i = 0; i < 10; i++) {
        acc ^= fnv1a(words[i]);
        for (int j = 0; j < 10; j++) {
            snprintf(tmp, sizeof tmp, "%s/%d", words[j], i * j);
            acc += fnv1a(tmp) >> 13;
        }
    }
    printf("fnv1a acc=%llu\n", (unsigned long long)acc);
    return 0;
}
