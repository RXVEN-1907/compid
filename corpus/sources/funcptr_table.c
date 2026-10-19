#include <stdio.h>

/* Dispatch through a table of function pointers chosen by a data stream. */
typedef int (*op_fn)(int);

static int op_inc(int x) { return x + 3; }
static int op_sq(int x) { return x * x % 100003; }
static int op_fold(int x) { return (x >> 1) ^ (x << 1); }
static int op_rev(int x) {
    int r = 0;
    while (x) { r = r * 10 + x % 10; x /= 10; }
    return r % 100003;
}
static int op_mix(int x) { return op_sq(x) ^ op_fold(x); }

static const op_fn ops[] = { op_inc, op_sq, op_fold, op_rev, op_mix };
static const char *op_names[] = { "inc", "sq", "fold", "rev", "mix" };

int main(void) {
    unsigned s = 6060;
    long acc = 0;
    for (int i = 0; i < 20000; i++) {
        s = s * 48271u % 2147483647u;
        int which = (int)(s % 5);
        int v = (int)(s % 50000);
        acc += ops[which](v);
        if ((i & 4095) == 0) acc += op_names[which][0];
    }
    printf("funcptr acc=%ld\n", acc);
    return 0;
}
