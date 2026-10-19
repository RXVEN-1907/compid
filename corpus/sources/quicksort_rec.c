#include <stdio.h>

/* Recursive quicksort with median-of-three pivot selection. */
static void swap(int *a, int *b) { int t = *a; *a = *b; *b = t; }

static void quicksort(int *a, int lo, int hi) {
    while (lo < hi) {
        int mid = lo + (hi - lo) / 2;
        if (a[mid] < a[lo]) swap(&a[mid], &a[lo]);
        if (a[hi] < a[lo]) swap(&a[hi], &a[lo]);
        if (a[hi] < a[mid]) swap(&a[hi], &a[mid]);
        int pivot = a[mid];
        int i = lo, j = hi;
        while (i <= j) {
            while (a[i] < pivot) i++;
            while (a[j] > pivot) j--;
            if (i <= j) { swap(&a[i], &a[j]); i++; j--; }
        }
        if (j - lo < hi - i) { quicksort(a, lo, j); lo = i; }
        else { quicksort(a, i, hi); hi = j; }
    }
}

int main(void) {
    static int a[2048];
    unsigned s = 99173;
    for (int i = 0; i < 2048; i++) {
        s ^= s << 13; s ^= s >> 17; s ^= s << 5;
        a[i] = (int)(s % 100000);
    }
    quicksort(a, 0, 2047);
    unsigned acc = 0;
    for (int i = 0; i < 2048; i += 7) acc += (unsigned)a[i];
    printf("quick acc=%u min=%d max=%d\n", acc, a[0], a[2047]);
    return 0;
}
