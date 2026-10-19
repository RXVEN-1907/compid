#include <stdio.h>
#include <stdarg.h>

/* Variadic functions: a sentinel-terminated int sum and a mixed-type fold. */
static int sum_all(int first, ...) {
    va_list ap;
    va_start(ap, first);
    int total = first;
    int v;
    while ((v = va_arg(ap, int)) != -1) total += v * 3 % 97;
    va_end(ap);
    return total;
}

static double mix_all(int n, ...) {
    va_list ap;
    va_start(ap, n);
    double acc = 0.0;
    for (int i = 0; i < n; i++) {
        if (i & 1) acc += va_arg(ap, double);
        else acc += va_arg(ap, int) * 0.125;
    }
    va_end(ap);
    return acc;
}

int main(void) {
    int t = 0;
    for (int i = 1; i <= 12; i++)
        t += sum_all(i, i + 1, i * 2, -1);
    double d = mix_all(10, 1, 2.5, 3, 4.5, 5, 6.5, 7, 8.5, 9, 10.5);
    printf("varargs t=%d d=%.6f\n", t, d);
    return 0;
}
