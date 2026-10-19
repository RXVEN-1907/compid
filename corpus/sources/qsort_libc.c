#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* libc qsort over a struct array with a real comparator: exercises the
   indirect calls qsort makes back into this binary. */
typedef struct {
    int key;
    char tag[8];
} rec;

static int cmp_rec(const void *pa, const void *pb) {
    const rec *a = pa, *b = pb;
    if (a->key != b->key) return (a->key < b->key) ? -1 : 1;
    return strcmp(a->tag, b->tag);
}

int main(void) {
    rec recs[512];
    unsigned s = 31337;
    for (int i = 0; i < 512; i++) {
        s = s * 1103515245u + 12345u;
        recs[i].key = (int)(s % 100) - 50;
        snprintf(recs[i].tag, sizeof recs[i].tag, "r%03d", i);
    }
    qsort(recs, 512, sizeof recs[0], cmp_rec);
    int sum = 0;
    for (int i = 0; i < 512; i += 13) sum += recs[i].key;
    printf("qsort sum=%d first=%s last=%s\n", sum, recs[0].tag, recs[511].tag);
    return 0;
}
