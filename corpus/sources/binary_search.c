#include <stdio.h>

/* Lower-bound binary search plus membership checks. */
static int lower_bound(const int *a, int n, int key) {
    int lo = 0, hi = n;
    while (lo < hi) {
        int mid = lo + (hi - lo) / 2;
        if (a[mid] < key) lo = mid + 1;
        else hi = mid;
    }
    return lo;
}

int main(void) {
    int a[1024];
    for (int i = 0; i < 1024; i++) a[i] = i * 3 + 1;
    long acc = 0;
    for (int q = -5; q < 3100; q += 11) {
        int idx = lower_bound(a, 1024, q);
        acc += (idx < 1024 && a[idx] == q) ? idx : -idx;
    }
    for (int q = 0; q < 1024; q++) acc += lower_bound(a, 1024, a[q]) == q;
    printf("binsearch acc=%ld\n", acc);
    return 0;
}
