#include <stdio.h>

/* Bubble sort with the early-exit flag. */
static void bubble_sort(int *a, int n) {
    for (int pass = 0; pass < n - 1; pass++) {
        int swapped = 0;
        for (int i = 0; i < n - 1 - pass; i++) {
            if (a[i] > a[i + 1]) {
                int t = a[i];
                a[i] = a[i + 1];
                a[i + 1] = t;
                swapped = 1;
            }
        }
        if (!swapped) break;
    }
}

int main(void) {
    int a[256];
    unsigned s = 12345;
    for (int i = 0; i < 256; i++) {
        s = s * 1103515245u + 12345u;
        a[i] = (int)(s >> 17) % 1000;
    }
    bubble_sort(a, 256);
    unsigned acc = 0;
    for (int i = 0; i < 256; i++) acc = acc * 131u + (unsigned)a[i];
    printf("bubble acc=%u a0=%d a255=%d\n", acc, a[0], a[255]);
    return 0;
}
