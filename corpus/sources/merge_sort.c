#include <stdio.h>

/* Recursive top-down mergesort with a shared scratch buffer. */
static int tmp[1024];

static void merge(int *a, int lo, int mid, int hi) {
    int i = lo, j = mid, k = lo;
    while (i < mid && j < hi)
        tmp[k++] = (a[i] <= a[j]) ? a[i++] : a[j++];
    while (i < mid) tmp[k++] = a[i++];
    while (j < hi) tmp[k++] = a[j++];
    for (i = lo; i < hi; i++) a[i] = tmp[i];
}

static void merge_sort(int *a, int lo, int hi) {
    if (hi - lo < 2) return;
    int mid = lo + (hi - lo) / 2;
    merge_sort(a, lo, mid);
    merge_sort(a, mid, hi);
    merge(a, lo, mid, hi);
}

int main(void) {
    int a[1024];
    unsigned s = 20260101;
    for (int i = 0; i < 1024; i++) {
        s = s * 69069u + 1u;
        a[i] = (int)((s >> 8) % 2048) - 1024;
    }
    merge_sort(a, 0, 1024);
    long acc = 0;
    for (int i = 0; i < 1024; i++) acc += (long)a[i] * i;
    printf("merge acc=%ld inversions_check=%d\n", acc, a[0] <= a[1023]);
    return 0;
}
