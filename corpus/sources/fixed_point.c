#include <stdio.h>
#include <stdint.h>

/* Q16.16 fixed-point multiply and a Newton-Raphson integer sqrt. */
typedef int32_t fp16;

static fp16 fp_mul(fp16 a, fp16 b) {
    return (fp16)(((int64_t)a * b) >> 16);
}

static uint32_t isqrt(uint32_t n) {
    uint32_t x = n, y = (x + 1) / 2;
    while (y < x) {
        x = y;
        y = (x + n / x) / 2;
    }
    return x;
}

int main(void) {
    fp16 a = 3 * 65536 + 14159 / 10000;  /* ~3.14 */
    fp16 b = 2 * 65536 + 71828 / 10000;  /* ~2.72 */
    fp16 prod = fp_mul(a, b);
    unsigned acc = 0;
    for (uint32_t i = 2; i < 100000; i += 3) acc ^= isqrt(i * 97);
    for (int i = 0; i < 32; i++) acc += (unsigned)fp_mul((fp16)(1 << i), prod);
    printf("fixed prod=%d acc=%u\n", prod, acc);
    return 0;
}
