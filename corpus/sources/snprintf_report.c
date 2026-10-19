#include <stdio.h>
#include <string.h>

/* Builds a report with repeated snprintf calls and manual truncation. */
static void stat_line(char *dst, size_t cap, const char *name,
                      double mean, double var, int n) {
    int len = snprintf(dst, cap, "%-10s mean=%9.4f var=%9.4f n=%d",
                       name, mean, var, n);
    if (len < 0) dst[0] = '\0';
    if ((size_t)len >= cap) dst[cap - 1] = '\0';
}

int main(void) {
    char buf[12][96];
    double x[64];
    for (int i = 0; i < 64; i++) x[i] = (i * 37) % 101 * 0.125 - 4.0;
    for (int r = 0; r < 12; r++) {
        double mean = 0.0, var = 0.0;
        for (int i = 0; i < 64; i++) mean += x[(i + r * 5) % 64];
        mean /= 64.0;
        for (int i = 0; i < 64; i++) {
            double d = x[(i + r * 5) % 64] - mean;
            var += d * d;
        }
        stat_line(buf[r], sizeof buf[0] - 4, "series", mean, var / 63.0, 64);
        strcat(buf[r], "|ok");
    }
    unsigned long acc = 0;
    for (int r = 0; r < 12; r++)
        for (const char *p = buf[r]; *p; p++) acc = acc * 31u + (unsigned char)*p;
    printf("report acc=%lu len0=%zu\n", acc, strlen(buf[0]));
    return 0;
}
