#include <stdio.h>

/* Collatz lengths plus Euclid gcd and modular exponentiation. */
static int collatz_len(unsigned long n) {
    int steps = 0;
    while (n != 1) {
        n = (n & 1) ? 3 * n + 1 : n / 2;
        steps++;
    }
    return steps;
}

static unsigned long gcd(unsigned long a, unsigned long b) {
    while (b) { unsigned long t = a % b; a = b; b = t; }
    return a;
}

static unsigned long modpow(unsigned long b, unsigned long e, unsigned long m) {
    unsigned long r = 1;
    b %= m;
    while (e) {
        if (e & 1) r = r * b % m;
        b = b * b % m;
        e >>= 1;
    }
    return r;
}

int main(void) {
    long acc = 0;
    for (unsigned long n = 1; n <= 5000; n++) acc += collatz_len(n);
    for (unsigned long i = 1; i < 500; i++)
        acc += (long)gcd(2197 * i + 1, 3861 * i + 2);
    for (unsigned long i = 1; i < 200; i++)
        acc += (long)modpow(7 + i, 101, 1009 * 3);
    printf("collatz acc=%ld\n", acc);
    return 0;
}
