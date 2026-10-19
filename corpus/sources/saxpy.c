#include <stdio.h>

/* SAXPY-style loops in two shapes: one fused, one split by destination. */
#define N 4096

static float x[N], y[N], w[N], v1[N], v2[N];

int main(void) {
    unsigned s = 90210;
    for (int i = 0; i < N; i++) {
        s = s * 22695477u + 1u;
        x[i] = (float)((int)s % 1024) / 64.0f;
        y[i] = (float)(i % 33) - 16.0f;
    }
    for (int rep = 0; rep < 10; rep++) {
        for (int i = 0; i < N; i++) w[i] = 2.5f * x[i] + y[i];
        for (int i = 0; i < N; i++) v1[i] = w[i] * 0.5f;
        for (int i = 0; i < N; i++) v2[i] = v1[i] + 0.125f * y[i];
        for (int i = 0; i < N; i++) x[i] = v2[i] - w[i];
    }
    double acc = 0;
    for (int i = 0; i < N; i += 17) acc += x[i] + w[i];
    printf("saxpy acc=%.6f\n", acc);
    return 0;
}
