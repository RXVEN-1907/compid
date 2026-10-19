#include <stdio.h>

/* Reduction loops: a plain dot product and a strided variant. */
#define N 2048

static double a[N], b[N];

static double dot(const double *u, const double *v, int n) {
    double s = 0.0;
    for (int i = 0; i < n; i++) s += u[i] * v[i];
    return s;
}

static double dot_strided(const double *u, const double *v, int n, int stride) {
    double s = 0.0;
    for (int i = 0; i < n; i += stride) s += u[i] * v[i];
    return s;
}

int main(void) {
    for (int i = 0; i < N; i++) {
        a[i] = (i % 64) * 0.015625 - 0.5;
        b[i] = ((i * 7919) % 128) / 128.0;
    }
    double d1 = 0, d2 = 0;
    for (int rep = 0; rep < 4; rep++) {
        d1 += dot(a, b, N);
        d2 += dot_strided(a, b, N, 2 + rep);
    }
    printf("dot d1=%.9f d2=%.9f\n", d1, d2);
    return 0;
}
