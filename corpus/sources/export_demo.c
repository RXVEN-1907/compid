/* Export-table probe: four small pure functions behind a portable export
 * macro, plus a main that folds their results into one checksum.
 *
 * On the windows targets EXPORT expands to __declspec(dllexport), so those
 * builds carry an export directory and `compid functions` can enumerate the
 * four functions from it; the source also builds as a windows DLL
 * (export_demo's -dll matrix rows). On ELF and Mach-O builds EXPORT is
 * empty and the same functions appear as regular symbol-table entries.
 * Bodies are deliberately tiny and deterministic: fixed inputs, own
 * arithmetic, no libc beyond printf, and a printed checksum so -O2 cannot
 * delete the work. */
#include <stdio.h>

#if defined(_WIN32)
#define EXPORT __declspec(dllexport)
#else
#define EXPORT
#endif

EXPORT unsigned export_add3(unsigned a, unsigned b, unsigned c) {
    return a + b + c;
}

EXPORT unsigned export_djb2(const char *s) {
    unsigned h = 5381u;
    while (*s) {
        h = h * 33u ^ (unsigned char)*s++;
    }
    return h;
}

EXPORT unsigned export_mix(unsigned x) {
    x ^= x >> 15;
    x *= 0x2c1b3c6du;
    x ^= x >> 12;
    x *= 0x297a2d39u;
    x ^= x >> 15;
    return x;
}

EXPORT long export_scale(long v, long factor) {
    return v * factor - (v >> 3);
}

int main(void) {
    const char *words[3] = {"export", "demo", "corpus"};
    unsigned acc = export_add3(7u, 11u, 13u);
    for (int i = 0; i < 3; i++) {
        acc = export_mix(acc ^ export_djb2(words[i]));
    }
    acc ^= (unsigned)export_scale(12345L, -3L);
    printf("export demo acc=%u\n", acc);
    return 0;
}
