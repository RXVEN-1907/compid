#include <stdio.h>

/* Sparse switch: widely separated cases, no compact jump table. */
static long handle(unsigned code, long v) {
    switch (code) {
        case 1:       return v + 1000000000L;
        case 99:      return v * 3;
        case 500:     return v - 7;
        case 2500:    return v ^ 0x12345678L;
        case 10000:   return v >> 2;
        case 44444:   return v * v % 1000000007L;
        case 100000:  return ~v;
        case 500000:  return v + (v << 5);
        case 1000000: return (v << 3) - v;
        default:      return v % 13;
    }
}

int main(void) {
    unsigned s = 314159;
    long acc = 0;
    for (int i = 0; i < 40000; i++) {
        s = s * 69069u + 1u;
        acc += handle(s % 1000001, (long)(s >> 8) % 1000);
    }
    printf("sparse acc=%ld\n", acc);
    return 0;
}
