#include <stdio.h>

/* Linear scans: min/max in one pass, a sentinel search, and a running
   branchless-style update (compare-and-select). */
static void scan(const int *a, int n, int *out_min, int *out_max, int needle) {
    int mn = a[0], mx = a[0], found = -1;
    for (int i = 1; i < n; i++) {
        int v = a[i];
        mn = (v < mn) ? v : mn;
        mx = (v > mx) ? v : mx;
        if (found < 0 && v == needle) found = i;
    }
    *out_min = mn;
    *out_max = mx;
    (void)found;
}

int main(void) {
    int a[4096];
    unsigned s = 42;
    for (int i = 0; i < 4096; i++) {
        s = s * 6364136223846793005u + 1442695040888963407u;
        a[i] = (int)((s >> 33) % 100000);
    }
    int mn, mx;
    long acc = 0;
    for (int rep = 0; rep < 8; rep++) {
        scan(a, 4096, &mn, &mx, 4242 + rep);
        acc += mn + mx;
    }
    printf("linear acc=%ld min=%d max=%d\n", acc, mn, mx);
    return 0;
}
