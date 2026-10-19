#include <stdio.h>
#include <string.h>

/* Sieve of Eratosthenes to 8192, then a few prime-gap statistics. */
#define LIMIT 8192

int main(void) {
    static unsigned char is_comp[LIMIT + 1];
    memset(is_comp, 0, sizeof is_comp);
    int count = 0, last = 2;
    long gap_sum = 0, gap_max = 0;
    for (int i = 2; i <= LIMIT; i++) {
        if (!is_comp[i]) {
            count++;
            long gap = i - last;
            gap_sum += gap;
            if (gap > gap_max) gap_max = gap;
            last = i;
            for (int j = i * i; j <= LIMIT; j += i) is_comp[j] = 1;
        }
    }
    unsigned acc = 0;
    for (int i = 2; i <= LIMIT; i++) if (!is_comp[i]) acc ^= (unsigned)i;
    printf("sieve count=%d gapsum=%ld gapmax=%ld acc=%u\n",
           count, gap_sum, gap_max, acc);
    return 0;
}
