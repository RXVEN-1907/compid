#include <stdio.h>
int main(void) {
    unsigned s = 12345, acc = 0;
    for (int i = 0; i < 512; i++) {
        s = s * 1664525u + 1013904223u;
        acc += s >> 24;
    }
    printf("fixture loop acc=%u\n", acc);
    return 0;
}
