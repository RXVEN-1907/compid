#include <stdio.h>

/* Dense switch over 0..15 in a loop: the classic jump-table shape. */
static int classify(int op, int v) {
    switch (op) {
        case 0:  return v + 1;
        case 1:  return v - 1;
        case 2:  return v * 2;
        case 3:  return v / 2;
        case 4:  return v ^ 0x55;
        case 5:  return v << 3;
        case 6:  return v >> 1;
        case 7:  return ~v;
        case 8:  return v | 0x100;
        case 9:  return v & 0x1FF;
        case 10: return -v;
        case 11: return v % 97;
        case 12: return v + 0x400;
        case 13: return v * 31 + 7;
        case 14: return v * 3 / 2;
        case 15: return v * v % 4093;
        default: return 0;
    }
}

int main(void) {
    unsigned s = 1;
    long acc = 0;
    for (int i = 0; i < 30000; i++) {
        s = s * 1664525u + 1013904223u;
        acc += classify((int)(s >> 28) & 15, (int)(s & 0xFFFF));
    }
    printf("switch acc=%ld\n", acc);
    return 0;
}
