#include <stdio.h>
int main(void) {
    int state = 0, ticks = 0;
    const int pattern[3] = {1, 2, 3};
    for (int i = 0; i < 48; i++) {
        if (state == 3) { state = 0; ticks++; }
        if (i % 4 == pattern[state]) state++;
    }
    printf("fixture statemachine cycles=%d\n", ticks);
    return 0;
}
