#include <stdio.h>

/* Horner's method for polynomials in float and double, plus a derivative. */
static double poly_eval(const double *c, int deg, double x) {
    double r = c[deg];
    for (int i = deg - 1; i >= 0; i--) r = r * x + c[i];
    return r;
}

static float poly_evalf(const float *c, int deg, float x) {
    float r = c[deg];
    for (int i = deg - 1; i >= 0; i--) r = r * x + c[i];
    return r;
}

int main(void) {
    double cd[17];
    float cf[17];
    for (int i = 0; i <= 16; i++) {
        cd[i] = 1.0 / (double)(i + 1);
        cf[i] = (float)cd[i];
    }
    double acc = 0;
    for (int k = 0; k <= 64; k++) {
        double x = -2.0 + k / 16.0;
        float xf = (float)x;
        acc += poly_eval(cd, 16, x) - poly_evalf(cf, 16, xf);
        acc += 0.5 * poly_eval(cd, 16, x * 0.5);
    }
    printf("horner acc=%.9f\n", acc);
    return 0;
}
