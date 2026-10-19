#include <stdio.h>

/* In-place heapsort with iterative sift-down. */
static void sift_down(int *a, int start, int end) {
    int root = start;
    for (;;) {
        int child = 2 * root + 1;
        if (child > end) break;
        if (child + 1 <= end && a[child] < a[child + 1]) child++;
        if (a[root] < a[child]) {
            int t = a[root];
            a[root] = a[child];
            a[child] = t;
            root = child;
        } else break;
    }
}

static void heap_sort(int *a, int n) {
    for (int start = n / 2 - 1; start >= 0; start--) sift_down(a, start, n - 1);
    for (int end = n - 1; end > 0; end--) {
        int t = a[0];
        a[0] = a[end];
        a[end] = t;
        sift_down(a, 0, end - 1);
    }
}

int main(void) {
    int a[1024];
    unsigned s = 5150;
    for (int i = 0; i < 1024; i++) {
        s = s * 1664525u + 1013904223u;
        a[i] = (int)(s >> 20);
    }
    heap_sort(a, 1024);
    unsigned acc = 0;
    for (int i = 0; i < 1024; i++) acc ^= (unsigned)a[i] << (i & 7);
    printf("heap acc=%u top=%d\n", acc, a[0]);
    return 0;
}
