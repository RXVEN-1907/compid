#include <stdio.h>

/* Vectorizable elementwise loops over float arrays. */
#define N 1024

static float x[N], y[N], z[N];

int main(void) {
    unsigned s = 2024;
    for (int i = 0; i < N; i++) {
        s = s * 1664525u + 1013904223u;
        x[i] = (float)((int)(s >> 16) % 256) / 16.0f - 8.0f;
        y[i] = (float)((i * i) % 97) * 0.25f;
    }
    for (int rep = 0; rep < 16; rep++) {
        for (int i = 0; i < N; i++) z[i] = x[i] + 3.0f * y[i];
        for (int i = 0; i < N; i++) x[i] = 0.5f * (z[i] - x[i]);
    }
    double acc = 0;
    for (int i = 0; i < N; i += 3) acc += z[i] - x[i];
    printf("vecadd acc=%.6f\n", acc);
    return 0;
}
