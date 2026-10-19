#include <stdio.h>

/* Sine lookup table built at startup, then linear interpolation. */
#define LUT_SIZE 1024

static float lut[LUT_SIZE + 1];

int main(void) {
    for (int i = 0; i <= LUT_SIZE; i++)
        lut[i] = (float)__builtin_sin(6.283185307179586 * i / LUT_SIZE);
    double acc = 0;
    for (int i = 0; i < 20000; i++) {
        double t = (i * 13) % 4096;
        double frac = t / 4096.0 * LUT_SIZE;
        int idx = (int)frac;
        double f = frac - idx;
        double s = lut[idx] + f * (lut[idx + 1] - lut[idx]);
        acc += s * s;
    }
    printf("sinlut acc=%.9f lutmax=%.6f\n", acc, lut[LUT_SIZE / 4]);
    return 0;
}
