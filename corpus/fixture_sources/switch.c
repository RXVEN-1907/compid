#include <stdio.h>
static int step(int op, int v) {
    switch (op) {
        case 0: return v + 1;
        case 1: return v * 2;
        case 2: return v - 3;
        case 3: return v ^ 0x2A;
        case 4: return v << 1;
        case 5: return v / 2;
        default: return 0;
    }
}
int main(void) {
    int acc = 0;
    for (int i = 0; i < 64; i++) acc += step(i % 7, i);
    printf("fixture switch acc=%d\n", acc);
    return 0;
}
