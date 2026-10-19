#include <stdio.h>

/* Insertion sort: short inner loop, branch-predictor friendly. */
static void insertion_sort(int *a, int n) {
    for (int i = 1; i < n; i++) {
        int key = a[i];
        int j = i - 1;
        while (j >= 0 && a[j] > key) {
            a[j + 1] = a[j];
            j--;
        }
        a[j + 1] = key;
    }
}

int main(void) {
    int a[512];
    unsigned s = 777;
    for (int i = 0; i < 512; i++) {
        s = s * 48271u % 2147483647u;
        a[i] = (int)(s % 4096);
    }
    insertion_sort(a, 512);
    long acc = 0;
    for (int i = 0; i < 512; i++) acc += (long)a[i] * (i + 1);
    printf("insertion acc=%ld med=%d\n", acc, a[256]);
    return 0;
}
